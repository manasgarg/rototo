use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use serde_json::{Value as JsonValue, json};

use super::api::{
    ApiError, ApiResult, SharedState, require_github_token, require_user, source_token,
};
use super::capabilities::{classify_workspace_source, workspace_capabilities};
use super::inventory::{
    WorkspaceInventory, inspect_workspace_inventory, read_workspace_definition,
};
use super::resolve_preview::{
    SavedContextInput, qualifier_context_evaluations, resolve_saved_contexts,
};
use super::store::{SessionUser, WorkspaceRecord};
use super::workspace_source::{
    runtime_workspace_for_base, semantic_workspace_for_base, workspace_source_for_base,
};

const MAX_PREVIEW_CONTEXTS: usize = 4;
const WORKSPACE_SUMMARY_CONCURRENCY: usize = 4;
/// Compare calls are one GitHub request per branch; keep the scan bounded.
const MAX_COMPARED_BRANCHES: usize = 25;
const BRANCH_CANDIDATE_COMPARE_CONCURRENCY: usize = 8;

/// Ordered workspace summary result from a bounded background task.
///
/// The index preserves the user's source tree/workspace ordering after concurrent
/// staging and lint work finishes.
type WorkspaceSummaryResult = (usize, JsonValue);

/// Ordered GitHub branch comparison result used while scanning branch candidates.
///
/// Each value is produced by one GitHub compare request and discarded after the
/// route has filtered it into a small browser-facing branch summary.
type BranchCandidateCompareResult = (
    usize,
    String,
    super::github::GitHubResult<super::github::RefComparison>,
);

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/workspaces/summaries", get(workspace_summaries))
        .route("/workspaces/{workspace_id}/lint", get(workspace_lint))
        .route("/workspaces/{workspace_id}/summary", get(workspace_summary))
        .route("/workspaces/{workspace_id}/data", get(workspace_data))
        .route("/workspaces/{workspace_id}/entity", get(workspace_entity))
        .route(
            "/workspaces/{workspace_id}/branch-candidates",
            get(branch_candidates),
        )
}

pub async fn load_workspace(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    workspace_id: &str,
) -> ApiResult<WorkspaceRecord> {
    if let Some(source) = state.fixed_workspace_source.as_deref() {
        super::register_fixed_workspace(state, &user.principal_id, source).await?;
    }
    state
        .store
        .get_workspace_for_user(workspace_id, &user.principal_id)
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

pub fn workspace_capabilities_json(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
) -> JsonValue {
    let source_kind = classify_workspace_source(&workspace.source);
    json!({
        "sourceKind": source_kind,
        "capabilities": workspace_capabilities(
            source_kind,
            state.write_policy,
            user.github_token.is_some(),
        ),
    })
}

async fn workspace_lint(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    let workspace_source =
        workspace_source_for_base(&state, &user.principal_id, source_token(&user), &workspace)
            .await?;
    let inspected = state
        .stage
        .get_inspected_workspace(workspace_source, source_token(&user))
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

/// Query string for the workspace summaries endpoint.
///
/// The optional source tree id scopes a request to one registered source tree. It is
/// parsed per request and never stored; discovery state remains in SQLite.
#[derive(serde::Deserialize, Default)]
struct WorkspaceSummariesQuery {
    #[serde(rename = "sourceTreeId")]
    source_tree_id: Option<String>,
}

async fn workspace_summaries(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceSummariesQuery>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let mut workspaces = state
        .store
        .list_workspaces_for_user(&user.principal_id)
        .await?;
    if let Some(source_tree_id) = query.source_tree_id.as_deref() {
        workspaces.retain(|workspace| workspace.source_tree_id == source_tree_id);
    }

    let mut summaries = Vec::new();
    let mut jobs = tokio::task::JoinSet::new();
    for (index, workspace) in workspaces.into_iter().enumerate() {
        let state = state.clone();
        let user = user.clone();
        jobs.spawn(async move {
            (
                index,
                workspace_summary_json(&state, &user, &workspace).await,
            )
        });
        if jobs.len() >= WORKSPACE_SUMMARY_CONCURRENCY {
            collect_workspace_summary(&mut jobs, &mut summaries).await;
        }
    }
    while !jobs.is_empty() {
        collect_workspace_summary(&mut jobs, &mut summaries).await;
    }
    summaries.sort_by_key(|(index, _)| *index);
    let summaries: Vec<JsonValue> = summaries.into_iter().map(|(_, summary)| summary).collect();

    Ok(Json(json!({ "summaries": summaries })))
}

async fn workspace_summary(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    Ok(Json(
        workspace_summary_json(&state, &user, &workspace).await,
    ))
}

async fn collect_workspace_summary(
    jobs: &mut tokio::task::JoinSet<WorkspaceSummaryResult>,
    summaries: &mut Vec<WorkspaceSummaryResult>,
) {
    let Some(joined) = jobs.join_next().await else {
        return;
    };
    if let Ok(summary) = joined {
        summaries.push(summary);
    }
}

async fn workspace_summary_json(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
) -> JsonValue {
    match workspace_inventory(state, user, workspace).await {
        Ok((_, inventory)) => workspace_summary_success_json(workspace, &inventory),
        Err(error) => json!({
            "workspaceId": workspace.id,
            "workspaceSlug": workspace.slug,
            "variables": 0,
            "qualifiers": 0,
            "catalogs": 0,
            "schemas": 0,
            "error": error,
        }),
    }
}

fn workspace_summary_success_json(
    workspace: &WorkspaceRecord,
    inventory: &WorkspaceInventory,
) -> JsonValue {
    json!({
        "workspaceId": workspace.id,
        "workspaceSlug": workspace.slug,
        "variables": inventory.variables.len(),
        "qualifiers": inventory.qualifiers.len(),
        "catalogs": inventory.catalogs.len(),
        "schemas": inventory.schemas.len(),
        "error": JsonValue::Null,
    })
}

async fn workspace_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    let branches = state
        .store
        .list_active_branches_for_workspace(&workspace.id, &user.principal_id)
        .await?;

    let staged =
        semantic_workspace_for_base(&state, &user.principal_id, source_token(&user), &workspace)
            .await;
    let (inventory, inventory_error, lint, model) = match staged {
        Ok(semantic) => {
            let inventory =
                inspect_workspace_inventory(&workspace, &semantic.model, semantic.workspace.root())
                    .await;
            let lint = match semantic.workspace.lint().await {
                Ok(lint) => lint_json(&lint),
                Err(err) => lint_error_json(&workspace.source, &err.to_string()),
            };
            match inventory {
                Ok(inventory) => (
                    serde_json::to_value(inventory).expect("inventory serializes"),
                    JsonValue::Null,
                    lint,
                    serde_json::to_value(semantic.model.as_ref()).expect("model serializes"),
                ),
                Err(err) => (
                    serde_json::to_value(WorkspaceInventory::default())
                        .expect("inventory serializes"),
                    json!(err.to_string()),
                    lint,
                    serde_json::to_value(semantic.model.as_ref()).expect("model serializes"),
                ),
            }
        }
        Err(err) => {
            let message = err.message;
            (
                serde_json::to_value(WorkspaceInventory::default()).expect("inventory serializes"),
                json!(message.clone()),
                lint_error_json(&workspace.source, &message),
                JsonValue::Null,
            )
        }
    };

    let capabilities = workspace_capabilities_json(&state, &user, &workspace);
    Ok(Json(json!({
        "workspace": workspace,
        "branches": branches,
        "inventory": inventory,
        "inventoryError": inventory_error,
        "lint": lint,
        "model": model,
        "sourceKind": capabilities["sourceKind"].clone(),
        "capabilities": capabilities["capabilities"].clone(),
    })))
}

/// Query string used to fetch one workspace file for inspect/edit screens.
///
/// The path is a repository-relative workspace path from the inventory. The
/// route validates it by resolving through the staged workspace root before
/// reading text.
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
    let semantic =
        semantic_workspace_for_base(&state, &user.principal_id, source_token(&user), &workspace)
            .await?;
    let inventory =
        inspect_workspace_inventory(&workspace, &semantic.model, semantic.workspace.root())
            .await
            .map_err(|err| ApiError::internal(err.to_string()))?;

    let (definition, definition_error) =
        match read_workspace_definition(&workspace, semantic.workspace.root(), &query.path).await {
            Ok(definition) => (
                serde_json::to_value(definition).expect("definition serializes"),
                JsonValue::Null,
            ),
            Err(err) => (JsonValue::Null, json!(err.to_string())),
        };

    let contexts = load_saved_contexts(
        &workspace,
        semantic.workspace.root(),
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
        && let Ok(runtime) =
            runtime_workspace_for_base(&state, &user.principal_id, source_token(&user), &workspace)
                .await
    {
        let resolutions =
            resolve_saved_contexts(&runtime, &semantic.model, &variable.id, &contexts).await;
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
        && let Ok(runtime) =
            runtime_workspace_for_base(&state, &user.principal_id, source_token(&user), &workspace)
                .await
    {
        let evaluations =
            qualifier_context_evaluations(&runtime, &semantic.model, &qualifier.id, &contexts)
                .await;
        qualifier_evaluations = serde_json::to_value(evaluations).expect("evaluations serialize");
    }

    Ok(Json(json!({
        "definition": definition,
        "definitionError": definition_error,
        "contextResolutions": context_resolutions,
        "qualifierEvaluations": qualifier_evaluations,
    })))
}

async fn branch_candidates(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    if !matches!(
        classify_workspace_source(&workspace.source),
        super::capabilities::WorkspaceSourceKind::GitHubArchive
            | super::capabilities::WorkspaceSourceKind::GitHubGit
    ) {
        return Err(ApiError::bad_request(
            "only GitHub source trees support branch discovery",
        ));
    }
    let token = require_github_token(&user, "Scanning branches")?;
    let (owner, name) = github_repo_for_workspace(&workspace)?;

    let known_branches: std::collections::HashSet<String> = state
        .store
        .list_active_branches_for_workspace(&workspace.id, &user.principal_id)
        .await?
        .into_iter()
        .map(|branch| branch.branch)
        .collect();
    let branches: Vec<String> = state
        .github
        .list_branches(token, &owner, &name)
        .await
        .map_err(|err| ApiError::github(&err, "Scanning branches"))?
        .into_iter()
        .filter(|branch| *branch != workspace.revision && !known_branches.contains(branch))
        .collect();
    let compared = &branches[..branches.len().min(MAX_COMPARED_BRANCHES)];
    let prefix = if workspace.path == "." {
        String::new()
    } else {
        format!("{}/", workspace.path)
    };

    let mut candidates = Vec::new();
    let mut comparisons = tokio::task::JoinSet::new();
    for (index, branch) in compared.iter().cloned().enumerate() {
        let github = state.github.clone();
        let token = token.to_owned();
        let owner = owner.clone();
        let name = name.clone();
        let base = workspace.revision.clone();
        comparisons.spawn(async move {
            let comparison = github
                .compare_refs(&token, &owner, &name, &base, &branch)
                .await;
            (index, branch, comparison)
        });
        if comparisons.len() >= BRANCH_CANDIDATE_COMPARE_CONCURRENCY {
            collect_branch_candidate(&mut comparisons, &prefix, &mut candidates).await;
        }
    }
    while !comparisons.is_empty() {
        collect_branch_candidate(&mut comparisons, &prefix, &mut candidates).await;
    }
    candidates.sort_by_key(|(index, _)| *index);
    let candidates: Vec<JsonValue> = candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();

    Ok(Json(json!({
        "candidates": candidates,
        "scanned": compared.len(),
        "skipped": branches.len() - compared.len(),
    })))
}

fn github_repo_for_workspace(workspace: &WorkspaceRecord) -> ApiResult<(String, String)> {
    super::github::parse_repo_spec(&workspace.source)
        .map_err(|err| ApiError::bad_request(err.to_string()))
}

async fn collect_branch_candidate(
    comparisons: &mut tokio::task::JoinSet<BranchCandidateCompareResult>,
    prefix: &str,
    candidates: &mut Vec<(usize, JsonValue)>,
) {
    let Some(joined) = comparisons.join_next().await else {
        return;
    };
    // A branch that cannot be compared is not a candidate.
    let Ok((index, branch, Ok(comparison))) = joined else {
        return;
    };
    if let Some(candidate) = branch_candidate_json(index, branch, comparison, prefix) {
        candidates.push(candidate);
    }
}

fn branch_candidate_json(
    index: usize,
    branch: String,
    comparison: super::github::RefComparison,
    prefix: &str,
) -> Option<(usize, JsonValue)> {
    let workspace_only = comparison.ahead_by > 0
        && !comparison.files.is_empty()
        && comparison.files.iter().all(|file| file.starts_with(prefix));
    workspace_only.then(|| {
        (
            index,
            json!({
                "branch": branch,
                "aheadBy": comparison.ahead_by,
                "filesChanged": comparison.files.len(),
            }),
        )
    })
}

pub async fn workspace_inventory(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
) -> std::result::Result<(std::sync::Arc<crate::sdk::Workspace>, WorkspaceInventory), String> {
    let semantic =
        semantic_workspace_for_base(state, &user.principal_id, source_token(user), workspace)
            .await
            .map_err(|err| err.message)?;
    let inventory =
        inspect_workspace_inventory(workspace, &semantic.model, semantic.workspace.root())
            .await
            .map_err(|err| err.to_string())?;
    Ok((semantic.workspace, inventory))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> WorkspaceRecord {
        WorkspaceRecord {
            id: "workspace-id".to_owned(),
            slug: "configs".to_owned(),
            source_tree_id: "repo-id".to_owned(),
            source_tree_label: "octo/configs".to_owned(),
            path: ".".to_owned(),
            revision: "main".to_owned(),
            source: "https://api.github.com/repos/octo/configs/tarball/main".to_owned(),
            discovered_at: "2026-06-13T00:00:00Z".to_owned(),
        }
    }

    fn catalog(id: &str) -> super::super::inventory::CatalogInventoryItem {
        super::super::inventory::CatalogInventoryItem {
            id: id.to_owned(),
            path: format!("catalogs/{id}.toml"),
            description: None,
            schema: None,
            schema_reference: None,
            entry_count: 0,
        }
    }

    fn catalog_entry(
        catalog_id: &str,
        key: &str,
    ) -> super::super::inventory::CatalogEntryInventoryItem {
        super::super::inventory::CatalogEntryInventoryItem {
            catalog_id: catalog_id.to_owned(),
            key: key.to_owned(),
            id: format!("{catalog_id}/{key}"),
            path: format!("catalogs/{catalog_id}-values/{key}.toml"),
        }
    }

    fn comparison(ahead_by: i64, files: &[&str]) -> super::super::github::RefComparison {
        super::super::github::RefComparison {
            ahead_by,
            files: files.iter().map(|file| (*file).to_owned()).collect(),
        }
    }

    #[test]
    fn workspace_summary_counts_catalogs_without_entries() {
        let inventory = WorkspaceInventory {
            catalogs: vec![catalog("plans"), catalog("limits")],
            catalog_entries: vec![
                catalog_entry("plans", "free"),
                catalog_entry("plans", "pro"),
                catalog_entry("limits", "enterprise"),
            ],
            ..WorkspaceInventory::default()
        };

        let summary = workspace_summary_success_json(&workspace(), &inventory);

        assert_eq!(summary["catalogs"].as_u64(), Some(2));
    }

    #[test]
    fn branch_candidate_accepts_root_workspace_changes() {
        let candidate = branch_candidate_json(
            2,
            "feature".to_owned(),
            comparison(3, &["variables/checkout.toml", "README.md"]),
            "",
        );

        assert_eq!(
            candidate,
            Some((
                2,
                serde_json::json!({
                    "branch": "feature",
                    "aheadBy": 3,
                    "filesChanged": 2,
                })
            ))
        );
    }

    #[test]
    fn branch_candidate_rejects_empty_or_unrelated_changes() {
        assert!(
            branch_candidate_json(
                0,
                "empty".to_owned(),
                comparison(0, &["examples/basic/variables/checkout.toml"]),
                "examples/basic/",
            )
            .is_none()
        );
        assert!(
            branch_candidate_json(0, "empty".to_owned(), comparison(1, &[]), "examples/basic/",)
                .is_none()
        );
        assert!(
            branch_candidate_json(
                0,
                "mixed".to_owned(),
                comparison(1, &["examples/basic/variables/checkout.toml", "README.md"],),
                "examples/basic/",
            )
            .is_none()
        );
    }
}
