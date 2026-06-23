use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use serde_json::{Value as JsonValue, json};
use std::collections::HashSet;

use super::api::{
    ApiError, ApiResult, SharedState, fixed_source_package_ids, fixed_source_scope,
    package_belongs_to_fixed_source, require_github_token, require_user, source_token,
};
use super::capabilities::{classify_package_source, package_capabilities};
use super::inventory::{PackageInventory, inspect_package_inventory, read_package_definition};
use super::package_source::{
    github_repo_for_package, package_source_for_base, runtime_package_for_base,
    semantic_package_for_base,
};
use super::resolve_preview::{
    SavedContextInput, qualifier_context_evaluations, resolve_saved_contexts,
};
use super::store::{PackageRecord, SessionUser};

const MAX_PREVIEW_CONTEXTS: usize = 4;
const PACKAGE_SUMMARY_CONCURRENCY: usize = 4;
/// Compare calls are one GitHub request per branch; keep the scan bounded.
const MAX_COMPARED_BRANCHES: usize = 25;
const BRANCH_CANDIDATE_COMPARE_CONCURRENCY: usize = 8;

/// Ordered package summary result from a bounded background task.
///
/// The index preserves the user's source tree/package ordering after concurrent
/// staging and lint work finishes.
type PackageSummaryResult = (usize, JsonValue);

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
        .route("/packages/summaries", get(package_summaries))
        .route("/packages/{package_id}/lint", get(package_lint))
        .route("/packages/{package_id}/summary", get(package_summary))
        .route("/packages/{package_id}/data", get(package_data))
        .route("/packages/{package_id}/entity", get(package_entity))
        .route(
            "/packages/{package_id}/branch-candidates",
            get(branch_candidates),
        )
}

pub async fn load_package(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    package_id: &str,
) -> ApiResult<PackageRecord> {
    let fixed_source = fixed_source_scope(state, &user.principal_id).await?;
    let package = state
        .store
        .get_package_for_user(package_id, &user.principal_id)
        .await?
        .ok_or_else(|| ApiError::not_found("package not found"))?;
    if let Some(source_tree) = fixed_source.as_ref()
        && !package_belongs_to_fixed_source(&package, source_tree)
    {
        return Err(ApiError::not_found("package not found"));
    }
    Ok(package)
}

/// `{root, diagnostics}` with the wire shape the TypeScript SDK produced.
pub fn lint_json(lint: &crate::model::PackageLint) -> JsonValue {
    json!({
        "root": lint.root.display().to_string(),
        "diagnostics": lint.diagnostics,
    })
}

pub fn lint_error_json(root: &str, error: &str) -> JsonValue {
    json!({ "root": root, "diagnostics": [], "error": error })
}

pub fn package_capabilities_json(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    package: &PackageRecord,
) -> JsonValue {
    let source_kind = classify_package_source(&package.source);
    json!({
        "sourceKind": source_kind,
        "capabilities": package_capabilities(
            source_kind,
            state.write_policy,
            &state.deployment,
            user.github_token.is_some(),
        ),
    })
}

async fn package_lint(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(package_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let package = load_package(&state, &user, &package_id).await?;
    let package_source =
        package_source_for_base(&state, &user.principal_id, source_token(&user), &package).await?;
    let inspected = state
        .stage
        .get_inspected_package(package_source, source_token(&user))
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?;
    let lint = inspected
        .lint()
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?;
    Ok(Json(
        json!({ "package": package, "lint": lint_json(&lint) }),
    ))
}

/// Query string for the package summaries endpoint.
///
/// The optional source tree id scopes a request to one registered source tree. It is
/// parsed per request and never stored; discovery state remains in SQLite.
#[derive(serde::Deserialize, Default)]
struct PackageSummariesQuery {
    #[serde(rename = "sourceTreeId")]
    source_tree_id: Option<String>,
}

async fn package_summaries(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<PackageSummariesQuery>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let fixed_source = fixed_source_scope(&state, &user.principal_id).await?;
    let mut packages = state
        .store
        .list_packages_for_user(&user.principal_id)
        .await?;
    if let Some(source_tree) = fixed_source.as_ref() {
        let package_ids = fixed_source_package_ids(source_tree);
        packages.retain(|package| package_ids.contains(&package.id));
    }
    if let Some(source_tree_id) = query.source_tree_id.as_deref() {
        packages.retain(|package| package.source_tree_id == source_tree_id);
    }

    let mut summaries = Vec::new();
    let mut jobs = tokio::task::JoinSet::new();
    for (index, package) in packages.into_iter().enumerate() {
        let state = state.clone();
        let user = user.clone();
        jobs.spawn(async move { (index, package_summary_json(&state, &user, &package).await) });
        if jobs.len() >= PACKAGE_SUMMARY_CONCURRENCY {
            collect_package_summary(&mut jobs, &mut summaries).await;
        }
    }
    while !jobs.is_empty() {
        collect_package_summary(&mut jobs, &mut summaries).await;
    }
    summaries.sort_by_key(|(index, _)| *index);
    let summaries: Vec<JsonValue> = summaries.into_iter().map(|(_, summary)| summary).collect();

    Ok(Json(json!({ "summaries": summaries })))
}

async fn package_summary(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(package_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let package = load_package(&state, &user, &package_id).await?;
    Ok(Json(package_summary_json(&state, &user, &package).await))
}

async fn collect_package_summary(
    jobs: &mut tokio::task::JoinSet<PackageSummaryResult>,
    summaries: &mut Vec<PackageSummaryResult>,
) {
    let Some(joined) = jobs.join_next().await else {
        return;
    };
    if let Ok(summary) = joined {
        summaries.push(summary);
    }
}

async fn package_summary_json(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    package: &PackageRecord,
) -> JsonValue {
    match package_inventory(state, user, package).await {
        Ok((_, inventory)) => package_summary_success_json(package, &inventory),
        Err(error) => json!({
            "packageId": package.id,
            "packageSlug": package.slug,
            "variables": 0,
            "qualifiers": 0,
            "catalogs": 0,
            "error": error,
        }),
    }
}

fn package_summary_success_json(
    package: &PackageRecord,
    inventory: &PackageInventory,
) -> JsonValue {
    json!({
        "packageId": package.id,
        "packageSlug": package.slug,
        "variables": inventory.variables.len(),
        "qualifiers": inventory.qualifiers.len(),
        "catalogs": inventory.catalogs.len(),
        "error": JsonValue::Null,
    })
}

async fn package_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(package_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let package = load_package(&state, &user, &package_id).await?;
    let branches = state
        .store
        .list_active_branches_for_package(&package.id, &user.principal_id)
        .await?;

    let staged =
        semantic_package_for_base(&state, &user.principal_id, source_token(&user), &package).await;
    let (inventory, inventory_error, lint, model, definitions) = match staged {
        Ok(semantic) => {
            let inventory =
                inspect_package_inventory(&package, &semantic.model, semantic.package.root()).await;
            let lint = match semantic.package.lint().await {
                Ok(lint) => lint_json(&lint),
                Err(err) => lint_error_json(&package.source, &err.to_string()),
            };
            match inventory {
                Ok(inventory) => {
                    let definitions =
                        package_definitions(&package, semantic.package.root(), &inventory).await;
                    (
                        serde_json::to_value(inventory).expect("inventory serializes"),
                        JsonValue::Null,
                        lint,
                        serde_json::to_value(semantic.model.as_ref()).expect("model serializes"),
                        definitions,
                    )
                }
                Err(err) => (
                    serde_json::to_value(PackageInventory::default())
                        .expect("inventory serializes"),
                    json!(err.to_string()),
                    lint,
                    serde_json::to_value(semantic.model.as_ref()).expect("model serializes"),
                    JsonValue::Array(Vec::new()),
                ),
            }
        }
        Err(err) => {
            let message = err.message;
            (
                serde_json::to_value(PackageInventory::default()).expect("inventory serializes"),
                json!(message.clone()),
                lint_error_json(&package.source, &message),
                JsonValue::Null,
                JsonValue::Array(Vec::new()),
            )
        }
    };

    let capabilities = package_capabilities_json(&state, &user, &package);
    Ok(Json(json!({
        "package": package,
        "branches": branches,
        "inventory": inventory,
        "inventoryError": inventory_error,
        "definitions": definitions,
        "lint": lint,
        "model": model,
        "sourceKind": capabilities["sourceKind"].clone(),
        "capabilities": capabilities["capabilities"].clone(),
    })))
}

async fn package_definitions(
    package: &PackageRecord,
    staged_root: &std::path::Path,
    inventory: &PackageInventory,
) -> JsonValue {
    let mut seen = HashSet::new();
    let mut definitions = Vec::new();
    for path in package_definition_paths(inventory) {
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Ok(definition) = read_package_definition(package, staged_root, &path).await {
            definitions.push(serde_json::to_value(definition).expect("definition serializes"));
        }
    }
    JsonValue::Array(definitions)
}

fn package_definition_paths(inventory: &PackageInventory) -> Vec<String> {
    let mut paths = Vec::new();
    paths.extend(inventory.variables.iter().map(|item| item.path.clone()));
    paths.extend(inventory.qualifiers.iter().map(|item| item.path.clone()));
    paths.extend(inventory.catalogs.iter().map(|item| item.path.clone()));
    paths.extend(
        inventory
            .catalog_entries
            .iter()
            .map(|item| item.path.clone()),
    );
    paths.extend(
        inventory
            .linters
            .iter()
            .filter_map(|item| item.path.clone()),
    );
    paths.extend(
        inventory
            .context
            .request_contexts
            .iter()
            .map(|item| item.path.clone()),
    );
    paths.extend(
        inventory
            .context
            .entries
            .iter()
            .map(|item| item.path.clone()),
    );
    paths
}

/// Query string used to fetch one package file for inspect/edit screens.
///
/// The path is a repository-relative package path from the inventory. The
/// route validates it by resolving through the staged package root before
/// reading text.
#[derive(serde::Deserialize)]
pub struct EntityQuery {
    pub path: String,
}

async fn package_entity(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(package_id): Path<String>,
    Query(query): Query<EntityQuery>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let package = load_package(&state, &user, &package_id).await?;
    let semantic =
        semantic_package_for_base(&state, &user.principal_id, source_token(&user), &package)
            .await?;
    let inventory = inspect_package_inventory(&package, &semantic.model, semantic.package.root())
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?;

    let (definition, definition_error) =
        match read_package_definition(&package, semantic.package.root(), &query.path).await {
            Ok(definition) => (
                serde_json::to_value(definition).expect("definition serializes"),
                JsonValue::Null,
            ),
            Err(err) => (JsonValue::Null, json!(err.to_string())),
        };

    let contexts = load_saved_contexts(
        &package,
        semantic.package.root(),
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
            runtime_package_for_base(&state, &user.principal_id, source_token(&user), &package)
                .await
    {
        let contexts = compatible_variable_contexts(&semantic.model, &variable.id, &contexts);
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
            runtime_package_for_base(&state, &user.principal_id, source_token(&user), &package)
                .await
    {
        let contexts = compatible_qualifier_contexts(&semantic.model, &qualifier.id, &contexts);
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

fn compatible_variable_contexts(
    model: &crate::lint::PackageSemanticModel,
    variable_id: &str,
    contexts: &[SavedContextInput],
) -> Vec<SavedContextInput> {
    let Some(compatibility) = model
        .variable_request_contexts
        .iter()
        .find(|compatibility| compatibility.variable == variable_id)
    else {
        return Vec::new();
    };
    contexts
        .iter()
        .filter(|context| {
            compatibility
                .request_contexts
                .iter()
                .any(|id| id == &context.request_context)
        })
        .cloned()
        .collect()
}

fn compatible_qualifier_contexts(
    model: &crate::lint::PackageSemanticModel,
    qualifier_id: &str,
    contexts: &[SavedContextInput],
) -> Vec<SavedContextInput> {
    let Some(compatibility) = model
        .qualifier_request_contexts
        .iter()
        .find(|compatibility| compatibility.qualifier == qualifier_id)
    else {
        return Vec::new();
    };
    contexts
        .iter()
        .filter(|context| {
            compatibility
                .request_contexts
                .iter()
                .any(|id| id == &context.request_context)
        })
        .cloned()
        .collect()
}

async fn branch_candidates(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(package_id): Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let package = load_package(&state, &user, &package_id).await?;
    if !matches!(
        classify_package_source(&package.source),
        super::capabilities::PackageSourceKind::GitHubArchive
            | super::capabilities::PackageSourceKind::GitHubGit
    ) {
        return Err(ApiError::bad_request(
            "only GitHub configuration sources support branch discovery",
        ));
    }
    let token = require_github_token(&user, "Scanning branches")?;
    let github_repo =
        github_repo_for_package(&package).map_err(|err| ApiError::bad_request(err.to_string()))?;

    let known_branches: std::collections::HashSet<String> = state
        .store
        .list_active_branches_for_package(&package.id, &user.principal_id)
        .await?
        .into_iter()
        .map(|branch| branch.branch)
        .collect();
    let branches: Vec<String> = state
        .github
        .list_branches(token, &github_repo.owner, &github_repo.name)
        .await
        .map_err(|err| ApiError::github(&err, "Scanning branches"))?
        .into_iter()
        .filter(|branch| *branch != package.revision && !known_branches.contains(branch))
        .collect();
    let compared = &branches[..branches.len().min(MAX_COMPARED_BRANCHES)];
    let prefix = if package.path == "." {
        String::new()
    } else {
        format!("{}/", package.path)
    };

    let mut candidates = Vec::new();
    let mut comparisons = tokio::task::JoinSet::new();
    for (index, branch) in compared.iter().cloned().enumerate() {
        let github = state.github.clone();
        let token = token.to_owned();
        let github_repo = github_repo.clone();
        let base = package.revision.clone();
        comparisons.spawn(async move {
            let comparison = github
                .compare_refs(
                    &token,
                    &github_repo.owner,
                    &github_repo.name,
                    &base,
                    &branch,
                )
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
    let package_only = comparison.ahead_by > 0
        && !comparison.files.is_empty()
        && comparison.files.iter().all(|file| file.starts_with(prefix));
    package_only.then(|| {
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

pub async fn package_inventory(
    state: &super::api::ConsoleState,
    user: &SessionUser,
    package: &PackageRecord,
) -> std::result::Result<(std::sync::Arc<crate::sdk::Package>, PackageInventory), String> {
    let semantic =
        semantic_package_for_base(state, &user.principal_id, source_token(user), package)
            .await
            .map_err(|err| err.message)?;
    let inventory = inspect_package_inventory(package, &semantic.model, semantic.package.root())
        .await
        .map_err(|err| err.to_string())?;
    Ok((semantic.package, inventory))
}

/// Reads up to `limit` saved request contexts from the staged checkout.
pub async fn load_saved_contexts(
    package: &PackageRecord,
    staged_root: &std::path::Path,
    inventory: &PackageInventory,
    limit: usize,
) -> Vec<SavedContextInput> {
    let mut contexts = Vec::new();
    for entry in inventory.context.entries.iter().take(limit) {
        let Ok(definition) = read_package_definition(package, staged_root, &entry.path).await
        else {
            continue;
        };
        contexts.push(SavedContextInput {
            name: entry.key.clone(),
            request_context: entry.request_context_id.clone(),
            path: entry.path.clone(),
            text: definition.text,
        });
    }
    contexts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> PackageRecord {
        PackageRecord {
            id: "package-id".to_owned(),
            slug: "configs".to_owned(),
            source_tree_id: "repo-id".to_owned(),
            source_tree_label: "octo/configs".to_owned(),
            display_path: ".".to_owned(),
            path: ".".to_owned(),
            revision: "main".to_owned(),
            source: "https://api.github.com/repos/octo/configs/tarball/main".to_owned(),
            discovered_at: "2026-06-13T00:00:00Z".to_owned(),
        }
    }

    fn catalog(id: &str) -> super::super::inventory::CatalogInventoryItem {
        super::super::inventory::CatalogInventoryItem {
            id: id.to_owned(),
            path: format!("catalogs/{id}.schema.json"),
            description: None,
            schema: None,
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
            path: format!("catalogs/{catalog_id}-entries/{key}.toml"),
        }
    }

    fn comparison(ahead_by: i64, files: &[&str]) -> super::super::github::RefComparison {
        super::super::github::RefComparison {
            ahead_by,
            files: files.iter().map(|file| (*file).to_owned()).collect(),
        }
    }

    #[test]
    fn package_summary_counts_catalogs_without_entries() {
        let inventory = PackageInventory {
            catalogs: vec![catalog("plans"), catalog("limits")],
            catalog_entries: vec![
                catalog_entry("plans", "free"),
                catalog_entry("plans", "pro"),
                catalog_entry("limits", "enterprise"),
            ],
            ..PackageInventory::default()
        };

        let summary = package_summary_success_json(&package(), &inventory);

        assert_eq!(summary["catalogs"].as_u64(), Some(2));
    }

    #[test]
    fn branch_candidate_accepts_root_package_changes() {
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
