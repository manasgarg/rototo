use std::time::Instant;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde_json::{Value as JsonValue, json};

use crate::console::github;

use super::{
    ApiError, ApiResult, SharedState, fixed_source_scope, require_github_token, require_user,
    source_token, source_tree_management_allowed,
};

pub(super) async fn source_trees_list(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let source_trees = match fixed_source_scope(&state, &user.principal_id).await? {
        Some(source_tree) => vec![source_tree],
        None => {
            state
                .store
                .list_source_trees_for_user(&user.principal_id)
                .await?
        }
    };
    Ok(Json(json!({ "sourceTrees": source_trees })))
}

/// Source tree registration request body from the console form.
///
/// It exists to keep user input distinct from a verified GitHub repository.
/// The route trims and validates it, discovers packages, then persists the
/// resulting source tree/package records through `Store`.
#[derive(serde::Deserialize)]
pub(super) struct RegisterSourceTreeBody {
    #[serde(rename = "sourceTree")]
    source_tree: Option<String>,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
}

pub(super) async fn source_trees_register(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<RegisterSourceTreeBody>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    source_tree_management_allowed(&state)?;
    let source_tree = body
        .source_tree
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("configuration source is required"))?;
    if should_register_as_github(source_tree).await {
        tracing::info!(
            operation = "source_tree.register",
            principal_id = %user.principal_id,
            backend = "github",
            requested_ref = ?body.git_ref.as_deref(),
            "console source tree registration selected GitHub backend"
        );
        return register_github_source_tree(state, user, source_tree, body.git_ref).await;
    }
    tracing::info!(
        operation = "source_tree.register",
        principal_id = %user.principal_id,
        backend = "read_only",
        requested_ref = ?body.git_ref.as_deref(),
        "console source tree registration selected read-only backend"
    );
    register_read_only_source_tree(state, user, source_tree, body.git_ref).await
}

async fn register_github_source_tree(
    state: SharedState,
    user: crate::console::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<Json<JsonValue>> {
    let (stored, token) = upsert_github_source_tree(&state, &user, source_tree, git_ref).await?;
    warm_registered_packages(
        state.clone(),
        user.principal_id.clone(),
        token,
        stored.packages.clone(),
    );
    Ok(Json(json!({ "sourceTree": stored })))
}

async fn upsert_github_source_tree(
    state: &SharedState,
    user: &crate::console::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<(crate::console::store::SourceTreeWithPackages, String)> {
    let token = require_github_token(user, "Registering the configuration source")?;
    let (owner, name) = github::parse_repo_spec(source_tree)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let github_repo = state
        .github
        .repo(token, &owner, &name)
        .await
        .map_err(|err| ApiError::github(&err, "Registering the configuration source"))?;
    let requested_ref = git_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| source_tree_ref_hint(source_tree));
    let git_ref = requested_ref.unwrap_or_else(|| github_repo.default_branch.clone());
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        kind = "github",
        repository = %format!("{}/{}", github_repo.owner.login, github_repo.name),
        git_ref = %git_ref,
        "console source tree package discovery starting"
    );
    let packages = state
        .github
        .discover_packages(token, &owner, &name, &git_ref)
        .await
        .map_err(|err| ApiError::github(&err, "Discovering packages"))?;
    let stored = state
        .store
        .upsert_source_tree_with_packages(crate::console::store::RegisterSourceTreeInput {
            principal_id: user.principal_id.clone(),
            kind: crate::console::store::SourceTreeKind::GitHub,
            source: format!(
                "git+https://github.com/{}/{}.git#{}",
                github_repo.owner.login, github_repo.name, git_ref
            ),
            display_name: format!("{}/{}", github_repo.owner.login, github_repo.name),
            default_revision: git_ref.clone(),
            packages: packages
                .into_iter()
                .map(|package| crate::console::store::DiscoveredPackageInput {
                    path: package.path,
                    revision: package.git_ref,
                    source: package.source,
                })
                .collect(),
        })
        .await?;
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        source_tree_id = %stored.source_tree.id,
        kind = "github",
        packages = stored.packages.len(),
        "console source tree upserted"
    );
    Ok((stored, token.to_owned()))
}

async fn register_read_only_source_tree(
    state: SharedState,
    user: crate::console::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<Json<JsonValue>> {
    let (stored, token) = upsert_read_only_source_tree(&state, &user, source_tree, git_ref).await?;
    warm_registered_packages(
        state.clone(),
        user.principal_id.clone(),
        token,
        stored.packages.clone(),
    );
    Ok(Json(json!({ "sourceTree": stored })))
}

pub(super) async fn upsert_read_only_source_tree(
    state: &SharedState,
    user: &crate::console::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<(crate::console::store::SourceTreeWithPackages, String)> {
    let source = read_only_registration_source(source_tree, git_ref.as_deref())?;
    let registration = crate::console::fixed_package::registration(&source)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        kind = ?registration.kind,
        packages = registration.packages.len(),
        "console read-only source tree registration resolved"
    );
    let stored = state
        .store
        .upsert_source_tree_with_packages(crate::console::store::RegisterSourceTreeInput {
            principal_id: user.principal_id.clone(),
            kind: registration.kind,
            source: registration.source,
            display_name: registration.display_name,
            default_revision: registration.default_revision,
            packages: registration.packages,
        })
        .await?;
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        source_tree_id = %stored.source_tree.id,
        kind = ?stored.source_tree.kind,
        packages = stored.packages.len(),
        "console source tree upserted"
    );
    Ok((stored, source_token(user).to_owned()))
}

pub(super) async fn should_register_as_github(source_tree: &str) -> bool {
    if source_tree.starts_with("file://")
        || source_tree.starts_with("git+file://")
        || source_tree.starts_with('/')
        || source_tree.starts_with('.')
        || source_tree.starts_with('~')
    {
        return false;
    }
    if tokio::fs::metadata(source_tree).await.is_ok() {
        return false;
    }
    github::parse_repo_spec(source_tree).is_ok()
}

pub(super) fn source_tree_ref_hint(source_tree: &str) -> Option<String> {
    let git_ref = if let Some(fragment) = source_tree.split_once('#').map(|(_, fragment)| fragment)
    {
        fragment
            .split_once(':')
            .map(|(git_ref, _)| git_ref)
            .unwrap_or(fragment)
            .trim()
    } else if let Some(rest) = source_tree.strip_prefix("https://api.github.com/repos/") {
        rest.split('/').nth(3).unwrap_or("").trim()
    } else {
        ""
    };
    (!git_ref.is_empty()).then(|| git_ref.to_owned())
}

fn read_only_registration_source(source_tree: &str, git_ref: Option<&str>) -> ApiResult<String> {
    let source = source_tree.trim();
    let git_ref = git_ref.map(str::trim).filter(|value| !value.is_empty());
    if let Some(git_ref) = git_ref {
        if source.starts_with("git+") && !source.contains('#') {
            return Ok(format!("{source}#{git_ref}"));
        }
        if !source.starts_with("git+") {
            return Err(ApiError::bad_request(
                "ref only applies to GitHub or git configuration sources",
            ));
        }
    }
    Ok(source.to_owned())
}

fn warm_registered_packages(
    state: SharedState,
    principal_id: String,
    token: String,
    packages: Vec<crate::console::store::PackageRecord>,
) {
    if packages.is_empty() {
        return;
    }

    tokio::spawn(async move {
        for package in packages {
            let started = Instant::now();
            match crate::console::package_source::semantic_package_for_base(
                &state,
                &principal_id,
                &token,
                &package,
            )
            .await
            {
                Ok(_) => {
                    tracing::debug!(
                        operation = "package.warm",
                        package_id = %package.id,
                        source = %package.source,
                        latency_ms = started.elapsed().as_millis(),
                        "console package warm-up completed"
                    );
                }
                Err(err) => {
                    tracing::debug!(
                        operation = "package.warm",
                        package_id = %package.id,
                        source = %package.source,
                        error = %err.message,
                        latency_ms = started.elapsed().as_millis(),
                        "console package warm-up failed"
                    );
                }
            }
        }
    });
}

pub(super) async fn source_tree_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::extract::Path(source_tree_id): axum::extract::Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    source_tree_management_allowed(&state)?;
    let removed = state
        .store
        .delete_source_tree_for_user(&source_tree_id, &user.principal_id)
        .await?;
    if !removed {
        return Err(ApiError::not_found("configuration source not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn source_tree_refresh(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::extract::Path(source_tree_id): axum::extract::Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let stored = refresh_source_tree_for_user(&state, &user, &source_tree_id).await?;
    Ok(Json(json!({ "sourceTree": stored })))
}

pub(super) async fn refresh_source_tree_for_user(
    state: &SharedState,
    user: &crate::console::store::SessionUser,
    source_tree_id: &str,
) -> ApiResult<crate::console::store::SourceTreeWithPackages> {
    let fixed_source = fixed_source_scope(state, &user.principal_id).await?;
    if let Some(source_tree) = fixed_source.as_ref()
        && source_tree.source_tree.id != source_tree_id
    {
        return Err(ApiError::not_found("configuration source not found"));
    }
    let existing = state
        .store
        .get_source_tree_for_user(source_tree_id, &user.principal_id)
        .await?
        .ok_or_else(|| ApiError::not_found("configuration source not found"))?;
    let source_tree = existing.source_tree;
    tracing::info!(
        operation = "source_tree.refresh",
        principal_id = %user.principal_id,
        source_tree_id,
        kind = ?source_tree.kind,
        "console source tree refresh selected backend"
    );
    let (stored, token) = match source_tree.kind {
        crate::console::store::SourceTreeKind::GitHub => {
            upsert_github_source_tree(
                state,
                user,
                &source_tree.source,
                Some(source_tree.default_revision.clone()),
            )
            .await?
        }
        crate::console::store::SourceTreeKind::GitRemote => {
            upsert_read_only_source_tree(
                state,
                user,
                &source_tree.source,
                Some(source_tree.default_revision.clone()),
            )
            .await?
        }
        crate::console::store::SourceTreeKind::LocalFolder
        | crate::console::store::SourceTreeKind::Archive => {
            upsert_read_only_source_tree(state, user, &source_tree.source, None).await?
        }
    };
    warm_registered_packages(
        state.clone(),
        user.principal_id.clone(),
        token,
        stored.packages.clone(),
    );
    Ok(stored)
}
