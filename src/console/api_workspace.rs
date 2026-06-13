use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use serde_json::{Value as JsonValue, json};

use super::api::{ApiError, ApiResult, SharedState, require_user};
use super::inventory::{
    WorkspaceInventory, inspect_workspace_inventory, read_workspace_definition,
};
use super::resolve_preview::{
    SavedContextInput, qualifier_context_evaluations, resolve_saved_contexts,
};
use super::store::{SessionUser, WorkspaceRecord};

const MAX_PREVIEW_CONTEXTS: usize = 4;
/// Compare calls are one GitHub request per branch; keep the scan bounded.
const MAX_COMPARED_BRANCHES: usize = 25;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/workspaces/{workspace_id}/lint", get(workspace_lint))
        .route("/workspaces/{workspace_id}/summary", get(workspace_summary))
        .route("/workspaces/{workspace_id}/data", get(workspace_data))
        .route("/workspaces/{workspace_id}/entity", get(workspace_entity))
        .route(
            "/workspaces/{workspace_id}/draft-candidates",
            get(draft_candidates),
        )
}

pub async fn load_workspace(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    workspace_id: &str,
) -> ApiResult<WorkspaceRecord> {
    state
        .store
        .get_workspace_for_user(workspace_id, &user.github_user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace not found"))
}

/// `{root, diagnostics}` with the wire shape the TypeScript SDK produced.
pub fn lint_json(lint: &crate::model::WorkspaceLint) -> JsonValue {
    json!({
        "root": lint.root.display().to_string(),
        "diagnostics": lint.diagnostics,
    })
}

pub fn lint_error_json(root: &str, error: &str) -> JsonValue {
    json!({ "root": root, "diagnostics": [], "error": error })
}

async fn workspace_lint(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    let inspected = state
        .stage
        .inspect(&user.github_token, &workspace.source)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?;
    let lint = inspected
        .lint()
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?;
    Ok(Json(
        json!({ "workspace": workspace, "lint": lint_json(&lint) }),
    ))
}

async fn workspace_summary(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    match workspace_inventory(&state, &user, &workspace).await {
        Ok((_, inventory)) => Ok(Json(json!({
            "variables": inventory.variables.len(),
            "qualifiers": inventory.qualifiers.len(),
            "resources": inventory.resources.len() + inventory.resource_objects.len(),
            "schemas": inventory.schemas.len(),
            "error": JsonValue::Null,
        }))),
        Err(error) => Ok(Json(json!({
            "variables": 0,
            "qualifiers": 0,
            "resources": 0,
            "schemas": 0,
            "error": error,
        }))),
    }
}

async fn workspace_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    let drafts = state
        .store
        .list_draft_sessions_for_workspace(&workspace.id, &user.github_user_id)
        .await?;

    let staged = state
        .stage
        .semantic_model(&user.github_token, &workspace.source)
        .await;
    let (inventory, inventory_error, lint, model) = match staged {
        Ok((inspected, model)) => {
            let inventory = inspect_workspace_inventory(&workspace, &model, inspected.root()).await;
            let lint = match inspected.lint().await {
                Ok(lint) => lint_json(&lint),
                Err(err) => lint_error_json(&workspace.source, &err.to_string()),
            };
            match inventory {
                Ok(inventory) => (
                    serde_json::to_value(inventory).expect("inventory serializes"),
                    JsonValue::Null,
                    lint,
                    serde_json::to_value(model.as_ref()).expect("model serializes"),
                ),
                Err(err) => (
                    serde_json::to_value(WorkspaceInventory::default())
                        .expect("inventory serializes"),
                    json!(err.to_string()),
                    lint,
                    serde_json::to_value(model.as_ref()).expect("model serializes"),
                ),
            }
        }
        Err(err) => {
            let message = err.to_string();
            (
                serde_json::to_value(WorkspaceInventory::default()).expect("inventory serializes"),
                json!(message.clone()),
                lint_error_json(&workspace.source, &message),
                JsonValue::Null,
            )
        }
    };

    Ok(Json(json!({
        "workspace": workspace,
        "drafts": drafts,
        "inventory": inventory,
        "inventoryError": inventory_error,
        "lint": lint,
        "model": model,
    })))
}

#[derive(serde::Deserialize)]
pub struct EntityQuery {
    pub path: String,
}

async fn workspace_entity(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Query(query): Query<EntityQuery>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    let (inspected, model) = state
        .stage
        .semantic_model(&user.github_token, &workspace.source)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?;
    let inventory = inspect_workspace_inventory(&workspace, &model, inspected.root())
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?;

    let (definition, definition_error) =
        match read_workspace_definition(&workspace, inspected.root(), &query.path).await {
            Ok(definition) => (
                serde_json::to_value(definition).expect("definition serializes"),
                JsonValue::Null,
            ),
            Err(err) => (JsonValue::Null, json!(err.to_string())),
        };

    let contexts = load_saved_contexts(
        &workspace,
        inspected.root(),
        &inventory,
        MAX_PREVIEW_CONTEXTS,
    )
    .await;

    // Variables resolve against each saved context so the screen shows the
    // actual pathway, not just the declared rules.
    let mut context_resolutions = JsonValue::Array(Vec::new());
    if let Some(variable) = inventory
        .variables
        .iter()
        .find(|variable| variable.path == query.path)
        && !contexts.is_empty()
        && let Ok(runtime) = state
            .stage
            .runtime(&user.github_token, &workspace.source)
            .await
    {
        let resolutions = resolve_saved_contexts(&runtime, &model, &variable.id, &contexts).await;
        context_resolutions = serde_json::to_value(resolutions).expect("resolutions serialize");
    }

    // Qualifiers evaluate (with any nested qualifier references) against each
    // saved context.
    let mut qualifier_evaluations = JsonValue::Array(Vec::new());
    if let Some(qualifier) = inventory
        .qualifiers
        .iter()
        .find(|qualifier| qualifier.path == query.path)
        && !contexts.is_empty()
        && let Ok(runtime) = state
            .stage
            .runtime(&user.github_token, &workspace.source)
            .await
    {
        let evaluations =
            qualifier_context_evaluations(&runtime, &model, &qualifier.id, &contexts).await;
        qualifier_evaluations = serde_json::to_value(evaluations).expect("evaluations serialize");
    }

    Ok(Json(json!({
        "definition": definition,
        "definitionError": definition_error,
        "contextResolutions": context_resolutions,
        "qualifierEvaluations": qualifier_evaluations,
    })))
}

async fn draft_candidates(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;

    let known_branches: std::collections::HashSet<String> = state
        .store
        .list_draft_sessions_for_workspace(&workspace.id, &user.github_user_id)
        .await?
        .into_iter()
        .map(|draft| draft.branch)
        .collect();
    let branches: Vec<String> = state
        .github
        .list_branches(&user.github_token, &workspace.owner, &workspace.name)
        .await
        .map_err(|err| ApiError::github(&err, "Scanning branches"))?
        .into_iter()
        .filter(|branch| *branch != workspace.git_ref && !known_branches.contains(branch))
        .collect();
    let compared = &branches[..branches.len().min(MAX_COMPARED_BRANCHES)];
    let prefix = if workspace.path == "." {
        String::new()
    } else {
        format!("{}/", workspace.path)
    };

    let mut candidates = Vec::new();
    for branch in compared {
        // A branch that cannot be compared is not a candidate.
        let Ok(comparison) = state
            .github
            .compare_refs(
                &user.github_token,
                &workspace.owner,
                &workspace.name,
                &workspace.git_ref,
                branch,
            )
            .await
        else {
            continue;
        };
        let workspace_only = comparison.ahead_by > 0
            && !comparison.files.is_empty()
            && comparison
                .files
                .iter()
                .all(|file| file.starts_with(&prefix));
        if workspace_only {
            candidates.push(json!({
                "branch": branch,
                "aheadBy": comparison.ahead_by,
                "filesChanged": comparison.files.len(),
            }));
        }
    }

    Ok(Json(json!({
        "candidates": candidates,
        "scanned": compared.len(),
        "skipped": branches.len() - compared.len(),
    })))
}

pub async fn workspace_inventory(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
) -> std::result::Result<(std::sync::Arc<crate::sdk::Workspace>, WorkspaceInventory), String> {
    let (inspected, model) = state
        .stage
        .semantic_model(&user.github_token, &workspace.source)
        .await
        .map_err(|err| err.to_string())?;
    let inventory = inspect_workspace_inventory(workspace, &model, inspected.root())
        .await
        .map_err(|err| err.to_string())?;
    Ok((inspected, inventory))
}

/// Reads up to `limit` saved request contexts from the staged checkout.
pub async fn load_saved_contexts(
    workspace: &WorkspaceRecord,
    staged_root: &std::path::Path,
    inventory: &WorkspaceInventory,
    limit: usize,
) -> Vec<SavedContextInput> {
    let mut contexts = Vec::new();
    for example_path in inventory.context.examples.iter().take(limit) {
        let name = example_path
            .rsplit('/')
            .next()
            .unwrap_or(example_path)
            .to_owned();
        let Ok(definition) = read_workspace_definition(workspace, staged_root, example_path).await
        else {
            continue;
        };
        contexts.push(SavedContextInput {
            name,
            path: example_path.clone(),
            text: definition.text,
        });
    }
    contexts
}
