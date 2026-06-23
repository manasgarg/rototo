use std::collections::{BTreeSet, HashSet};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, patch, post};
use serde::Serialize;
use serde_json::{Value as JsonValue, json};

use crate::error::Result;

use super::api::{
    ApiError, ApiResult, ConsoleState, SharedState, require_github_token, require_user,
    source_token,
};
use super::api_package::{
    EntityQuery, lint_error_json, lint_json, load_package, load_saved_contexts,
    package_capabilities_json,
};
use super::capabilities::{
    DeploymentType, PackageSourceKind, WritePolicy, classify_package_source,
};
use super::github::package_repo_path;
use super::inventory::{
    PackageInventory, inspect_package_inventory, language_for_path, read_package_definition,
};
use super::local_git;
use super::package_edit::{
    EntityKind, belongs_to_package, branch_pr_body, branch_pr_title, console_branch_name,
    entity_template_files, expected_variable_file_path, parse_entity_id, parse_variable_type,
    variable_default_target_path,
};
use super::package_source::{
    github_repo_for_package, package_source_for_branch, runtime_package_for_base,
};
use super::resolve_preview::edit_context_previews;
use super::stage::{BranchName, GitRefName, SourceTreeRevision};
use super::store::{
    ActiveBranchRecord, ActiveBranchStatus, BranchPullRequestInput, PackageRecord,
    SelectBranchInput, SessionUser,
};
use super::variable_toml::update_primitive_variable_default;

const PR_SYNC_FRESH: Duration = Duration::from_secs(60);
const MAX_PREVIEW_CONTEXTS: usize = 4;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/packages/{package_id}/branches", post(branch_select))
        .route(
            "/packages/{package_id}/branches/{branch_id}",
            patch(branch_rename),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/data",
            get(branch_data),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/entity",
            get(branch_entity),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/sync-pr",
            post(branch_sync_pr),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/publish",
            post(branch_publish),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/archive",
            post(branch_archive),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/variables",
            post(branch_variable_save),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/files",
            post(branch_file_save).delete(branch_file_delete),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/entities",
            post(branch_entity_create),
        )
        .route(
            "/packages/{package_id}/branches/{branch_id}/lsp",
            post(branch_lsp),
        )
}

struct BranchContext {
    user: SessionUser,
    package: PackageRecord,
    branch: ActiveBranchRecord,
    github_repo: Option<super::github::GitHubRepoIdentity>,
}

enum BranchBackend<'a> {
    GitHub { token: &'a str, direct: bool },
    LocalWorkingTree,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchFileChange {
    id: String,
    file_path: String,
}

fn branch_backend<'a>(
    state: &ConsoleState,
    user: &'a SessionUser,
    package: &PackageRecord,
    action: &str,
) -> ApiResult<BranchBackend<'a>> {
    let kind = classify_package_source(&package.source);
    match state.write_policy {
        WritePolicy::Disabled => Err(ApiError::bad_request(format!(
            "{action} is disabled for this console"
        ))),
        WritePolicy::PullRequest => match kind {
            PackageSourceKind::GitHubArchive | PackageSourceKind::GitHubGit => {
                let token = require_github_token(user, action)?;
                Ok(BranchBackend::GitHub {
                    token,
                    direct: false,
                })
            }
            _ => Err(ApiError::bad_request(
                "only GitHub configuration sources support pull-request edits",
            )),
        },
        WritePolicy::DirectPush => match kind {
            PackageSourceKind::GitHubArchive | PackageSourceKind::GitHubGit => {
                let token = require_github_token(user, action)?;
                Ok(BranchBackend::GitHub {
                    token,
                    direct: true,
                })
            }
            PackageSourceKind::LocalPath | PackageSourceKind::FileUrl
                if state.deployment == DeploymentType::Local =>
            {
                Ok(BranchBackend::LocalWorkingTree)
            }
            PackageSourceKind::LocalPath | PackageSourceKind::FileUrl => Err(
                ApiError::bad_request("local folder edits require a local console deployment"),
            ),
            _ => Err(ApiError::bad_request(
                "only GitHub or local folder configuration sources support direct edits",
            )),
        },
    }
}

fn context_is_github_package(package: &PackageRecord) -> bool {
    matches!(
        classify_package_source(&package.source),
        PackageSourceKind::GitHubArchive | PackageSourceKind::GitHubGit
    )
}

fn context_is_local_package(package: &PackageRecord) -> bool {
    matches!(
        classify_package_source(&package.source),
        PackageSourceKind::LocalPath | PackageSourceKind::FileUrl
    )
}

fn context_github_repo(context: &BranchContext) -> ApiResult<&super::github::GitHubRepoIdentity> {
    context
        .github_repo
        .as_ref()
        .ok_or_else(|| ApiError::bad_request("GitHub repository not found"))
}

async fn load_branch(
    state: &ConsoleState,
    headers: &HeaderMap,
    package_id: &str,
    branch_id: &str,
    require_active: bool,
) -> ApiResult<BranchContext> {
    let user = require_user(state, headers).await?;
    let package = load_package(state, &user, package_id).await?;
    let branch = state
        .store
        .get_active_branch_for_user(branch_id, &package.id, &user.principal_id)
        .await?
        .ok_or_else(|| ApiError::not_found("branch not found"))?;
    if require_active && branch.status != ActiveBranchStatus::Active {
        return Err(ApiError::bad_request("branch is not active"));
    }
    Ok(BranchContext {
        user,
        github_repo: if context_is_github_package(&package) {
            Some(
                github_repo_for_package(&package)
                    .map_err(|err| ApiError::bad_request(err.to_string()))?,
            )
        } else {
            None
        },
        package,
        branch,
    })
}

async fn invalidate_branch(
    state: &ConsoleState,
    user: &SessionUser,
    package: &PackageRecord,
    branch: &ActiveBranchRecord,
) {
    state.lsp.drop_sessions_for_branch(&branch.id).await;
    if let Ok(source) = package_source_for_branch(
        state,
        &user.principal_id,
        source_token(user),
        package,
        &branch.branch,
    )
    .await
    {
        if source.package.source_tree.revision == SourceTreeRevision::LocalWorkingTree {
            state.stage.invalidate_package(&source).await;
        } else if let Ok(cached_tree) = source.cached_source_tree_origin() {
            state
                .stage
                .invalidate_branch(&cached_tree, &branch.branch)
                .await;
        } else {
            state.stage.invalidate_package(&source).await;
        }
    }
}

async fn inspect_branch_package(
    state: &ConsoleState,
    user: &SessionUser,
    package: &PackageRecord,
    branch: &ActiveBranchRecord,
) -> ApiResult<Arc<crate::sdk::Package>> {
    let token = source_token(user);
    let package_source =
        package_source_for_branch(state, &user.principal_id, token, package, &branch.branch)
            .await?;
    state
        .stage
        .get_inspected_package(package_source, token)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))
}

#[derive(serde::Deserialize, Default)]
struct BranchSelectBody {
    branch: Option<String>,
}

async fn branch_select(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(package_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let package = load_package(&state, &user, &package_id).await?;
    let requested_branch = parse_branch_select_body(&body)?
        .branch
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty());
    tracing::info!(
        operation = "branch.select",
        principal_id = %user.principal_id,
        package_id = %package.id,
        requested_branch = ?requested_branch.as_deref(),
        "console branch selection requested"
    );

    let base_ref = package.revision.clone();
    let backend = branch_backend(&state, &user, &package, "Opening a branch")?;
    let github_repo = if context_is_github_package(&package) {
        Some(
            github_repo_for_package(&package)
                .map_err(|err| ApiError::bad_request(err.to_string()))?,
        )
    } else {
        None
    };
    let target = branch_selection_target(
        &state,
        &user,
        &package,
        github_repo.as_ref(),
        &backend,
        requested_branch,
        &base_ref,
    )
    .await?;

    if let Some(existing) = state
        .store
        .find_active_branch_for_source_tree_branch(&package.id, &user.principal_id, &target.branch)
        .await?
    {
        let existing = state
            .store
            .ensure_active_branch_package(&existing.id, &package.id, &user.principal_id)
            .await?;
        tracing::info!(
            operation = "branch.select",
            principal_id = %user.principal_id,
            package_id = %package.id,
            branch_id = %existing.id,
            branch = %existing.branch,
            outcome = "existing",
            "console branch selection reused existing branch"
        );
        return Ok(Json(json!({ "branch": existing })));
    }

    let branch = state
        .store
        .select_branch(SelectBranchInput {
            package_id: package.id.clone(),
            principal_id: user.principal_id.clone(),
            branch: target.branch,
            base_ref,
            base_commit: target.base_commit,
            last_seen_commit: target.last_seen_commit,
        })
        .await?;
    tracing::info!(
        operation = "branch.select",
        principal_id = %user.principal_id,
        package_id = %package.id,
        branch_id = %branch.id,
        branch = %branch.branch,
        outcome = "selected",
        "console branch selection stored active branch"
    );
    Ok(Json(json!({ "branch": branch })))
}

struct BranchSelectionTarget {
    branch: String,
    base_commit: Option<String>,
    last_seen_commit: Option<String>,
}

async fn branch_selection_target<'a>(
    state: &ConsoleState,
    user: &SessionUser,
    package: &PackageRecord,
    github_repo: Option<&super::github::GitHubRepoIdentity>,
    backend: &BranchBackend<'a>,
    requested_branch: Option<String>,
    base_ref: &str,
) -> ApiResult<BranchSelectionTarget> {
    match backend {
        BranchBackend::GitHub {
            token,
            direct: true,
        } => {
            tracing::info!(
                operation = "branch.selection_target",
                principal_id = %user.principal_id,
                package_id = %package.id,
                mode = "direct_push",
                base_ref,
                requested_branch = ?requested_branch.as_deref(),
                "console branch target resolving direct-push branch"
            );
            let github_repo =
                github_repo.ok_or_else(|| ApiError::bad_request("GitHub repository not found"))?;
            if let Some(requested) = requested_branch.as_deref()
                && requested != base_ref
            {
                return Err(ApiError::bad_request(format!(
                    "direct-push branches write to configured ref {base_ref}, not {requested}"
                )));
            }
            state
                .github
                .assert_repo_write_access(token, &github_repo.owner, &github_repo.name)
                .await
                .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
            let sha = state
                .github
                .branch_head_sha(token, &github_repo.owner, &github_repo.name, base_ref)
                .await
                .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
            Ok(BranchSelectionTarget {
                branch: base_ref.to_owned(),
                base_commit: Some(sha.clone()),
                last_seen_commit: Some(sha),
            })
        }
        BranchBackend::GitHub {
            token,
            direct: false,
        } => {
            tracing::info!(
                operation = "branch.selection_target",
                principal_id = %user.principal_id,
                package_id = %package.id,
                mode = "pull_request",
                base_ref,
                requested_branch = ?requested_branch.as_deref(),
                "console branch target resolving pull-request branch"
            );
            let github_repo =
                github_repo.ok_or_else(|| ApiError::bad_request("GitHub repository not found"))?;
            state
                .github
                .assert_repo_write_access(token, &github_repo.owner, &github_repo.name)
                .await
                .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
            let base_sha = state
                .github
                .branch_head_sha(token, &github_repo.owner, &github_repo.name, base_ref)
                .await
                .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
            let branch = match requested_branch {
                Some(branch) => {
                    tracing::info!(
                        operation = "branch.selection_target",
                        principal_id = %user.principal_id,
                        package_id = %package.id,
                        branch = %branch,
                        outcome = "requested_existing",
                        "console branch target validating requested branch"
                    );
                    if branch == base_ref {
                        return Err(ApiError::bad_request(format!(
                            "Editing {base_ref} directly would skip review. Pick another branch."
                        )));
                    }
                    state
                        .github
                        .branch_head_sha(token, &github_repo.owner, &github_repo.name, &branch)
                        .await
                        .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
                    branch
                }
                None => {
                    let branch = console_branch_name(&user.identity.display_login(), package);
                    tracing::info!(
                        operation = "branch.selection_target",
                        principal_id = %user.principal_id,
                        package_id = %package.id,
                        branch = %branch,
                        outcome = "create",
                        "console branch target creating new branch"
                    );
                    state
                        .github
                        .create_branch(
                            token,
                            &github_repo.owner,
                            &github_repo.name,
                            &branch,
                            &base_sha,
                        )
                        .await
                        .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
                    branch
                }
            };
            let last_seen_commit = state
                .github
                .branch_head_sha(token, &github_repo.owner, &github_repo.name, &branch)
                .await
                .ok();
            Ok(BranchSelectionTarget {
                branch,
                base_commit: Some(base_sha),
                last_seen_commit,
            })
        }
        BranchBackend::LocalWorkingTree => {
            if requested_branch.is_some() {
                return Err(ApiError::bad_request(
                    "local folder edits use the current working tree branch",
                ));
            }
            let root = local_source_root(state, package).await?;
            let branch = local_git::current_branch_at(&root)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            let head = local_git::head_commit(&root)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            Ok(BranchSelectionTarget {
                branch,
                base_commit: Some(head.clone()),
                last_seen_commit: Some(head),
            })
        }
    }
}

fn parse_branch_select_body(body: &[u8]) -> ApiResult<BranchSelectBody> {
    if body.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(BranchSelectBody::default());
    }
    serde_json::from_slice(body)
        .map_err(|err| ApiError::bad_request(format!("invalid JSON body: {err}")))
}

#[derive(serde::Deserialize)]
struct BranchRenameBody {
    branch: Option<String>,
}

async fn branch_rename(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
    Json(body): Json<BranchRenameBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, true).await?;
    let Some(branch) = body
        .branch
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty())
    else {
        return Err(ApiError::bad_request("branch is required"));
    };
    if branch == context.branch.branch {
        return Ok(Json(json!({ "branch": context.branch })));
    }
    let BranchBackend::GitHub {
        token,
        direct: false,
    } = branch_backend(
        &state,
        &context.user,
        &context.package,
        "Renaming the branch",
    )?
    else {
        return Err(ApiError::bad_request(
            "branch rename only applies to GitHub pull-request branches",
        ));
    };
    let github_repo = context_github_repo(&context)?;
    let renamed = state
        .github
        .rename_branch(
            token,
            &github_repo.owner,
            &github_repo.name,
            &context.branch.branch,
            &branch,
        )
        .await
        .map_err(|err| ApiError::github(&err, "Renaming the branch"))?;
    let updated = state
        .store
        .rename_active_branch(&context.branch.id, &renamed)
        .await?;
    Ok(Json(json!({ "branch": updated })))
}

async fn branch_sync_pr(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, false).await?;
    let BranchBackend::GitHub {
        token: _,
        direct: false,
    } = branch_backend(
        &state,
        &context.user,
        &context.package,
        "Syncing the pull request",
    )?
    else {
        return Err(ApiError::bad_request(
            "pull request sync only applies to GitHub pull-request branches",
        ));
    };
    let Some(pr_number) = context
        .branch
        .pr_number
        .or_else(|| pull_request_number_from_url(context.branch.pr_url.as_deref()))
    else {
        return Err(ApiError::bad_request("branch does not have a pull request"));
    };
    let branch = sync_pull_request(
        &state,
        &context.user,
        context_github_repo(&context)?,
        &context.branch,
        pr_number,
    )
    .await
    .map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "branch": branch })))
}

async fn sync_pull_request(
    state: &ConsoleState,
    user: &SessionUser,
    github_repo: &super::github::GitHubRepoIdentity,
    branch: &ActiveBranchRecord,
    pr_number: i64,
) -> std::result::Result<ActiveBranchRecord, String> {
    let token = user
        .github_token
        .as_deref()
        .ok_or_else(|| "Syncing the pull request requires a GitHub credential".to_owned())?;
    let pr = state
        .github
        .pull_request(token, &github_repo.owner, &github_repo.name, pr_number)
        .await
        .map_err(|err| super::github::github_error_message(&err, "Syncing the pull request"))?;
    state
        .store
        .update_active_branch_pull_request_state(BranchPullRequestInput {
            branch_id: branch.id.clone(),
            pr_number: pr.number,
            pr_state: pull_request_state(pr.state.as_deref(), pr.merged_at.as_deref()),
            pr_url: pr.html_url.clone(),
            pr_merged_at: pr.merged_at.clone(),
        })
        .await
        .map_err(|err| err.to_string())
}

async fn branch_publish(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, true).await?;
    let changed_paths =
        branch_changed_paths(&state, &context.user, &context.package, &context.branch).await?;
    tracing::info!(
        operation = "branch.publish",
        principal_id = %context.user.principal_id,
        package_id = %context.package.id,
        branch_id = %context.branch.id,
        changed_files = changed_paths.len(),
        "console branch publish requested"
    );
    if changed_paths.is_empty() {
        return Err(ApiError::bad_request("branch has no changed files"));
    }

    let mut lint_packages = state
        .store
        .list_packages_for_active_branch(&context.branch.id)
        .await?;
    if lint_packages.is_empty() {
        lint_packages.push(context.package.clone());
    }
    let mut errors = 0;
    let mut warnings = 0;
    for package in &lint_packages {
        let inspected =
            inspect_branch_package(&state, &context.user, package, &context.branch).await?;
        let lint = inspected
            .lint()
            .await
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
        let package_errors = lint
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::diagnostics::Severity::Error)
            .count();
        errors += package_errors;
        warnings += lint.diagnostics.len() - package_errors;
    }
    if errors > 0 {
        tracing::info!(
            operation = "branch.publish",
            principal_id = %context.user.principal_id,
            package_id = %context.package.id,
            branch_id = %context.branch.id,
            errors,
            warnings,
            outcome = "lint_failed",
            "console branch publish blocked by lint errors"
        );
        return Err(ApiError::bad_request(format!(
            "branch has {errors} lint error(s) across included package(s); fix lint before publishing"
        )));
    }

    match branch_backend(
        &state,
        &context.user,
        &context.package,
        "Publishing the branch",
    )? {
        BranchBackend::GitHub {
            token,
            direct: false,
        } => {
            let github_repo = context_github_repo(&context)?;
            tracing::info!(
                operation = "branch.publish",
                principal_id = %context.user.principal_id,
                package_id = %context.package.id,
                branch_id = %context.branch.id,
                mode = "pull_request",
                warnings,
                "console branch publish creating pull request"
            );
            let pr = state
                .github
                .create_pull_request(
                    token,
                    &github_repo.owner,
                    &github_repo.name,
                    &branch_pr_title(&context.package),
                    &branch_pr_body(
                        &context.package,
                        &context.branch,
                        &changed_paths,
                        errors,
                        warnings,
                    ),
                    &context.branch.branch,
                    &context.branch.base_ref,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Publishing the branch"))?;
            let pr_state = if pr.merged_at.is_some() {
                "merged".to_owned()
            } else {
                pr.state.clone().unwrap_or_else(|| "open".to_owned())
            };
            let branch = state
                .store
                .update_active_branch_pull_request_state(BranchPullRequestInput {
                    branch_id: context.branch.id.clone(),
                    pr_number: pr.number,
                    pr_state,
                    pr_url: pr.html_url.clone(),
                    pr_merged_at: pr.merged_at.clone(),
                })
                .await?;
            tracing::info!(
                operation = "branch.publish",
                principal_id = %context.user.principal_id,
                package_id = %context.package.id,
                branch_id = %context.branch.id,
                pr_number = pr.number,
                outcome = "pull_request_created",
                "console branch publish completed"
            );
            Ok(Json(json!({
                "branch": branch,
                "pullRequest": {
                    "html_url": pr.html_url,
                    "number": pr.number,
                    "state": pr.state,
                    "merged_at": pr.merged_at,
                }
            })))
        }
        BranchBackend::GitHub {
            token: _,
            direct: true,
        } => {
            tracing::info!(
                operation = "branch.publish",
                principal_id = %context.user.principal_id,
                package_id = %context.package.id,
                branch_id = %context.branch.id,
                mode = "direct_push",
                "console branch publish recorded direct-push edit"
            );
            let branch = state
                .store
                .record_active_branch_edit(&context.branch.id, None)
                .await?;
            Ok(Json(json!({
                "branch": branch,
                "directPush": { "backend": "githubApi" },
            })))
        }
        BranchBackend::LocalWorkingTree => {
            tracing::info!(
                operation = "branch.publish",
                principal_id = %context.user.principal_id,
                package_id = %context.package.id,
                branch_id = %context.branch.id,
                mode = "local_working_tree",
                "console branch publish validated local working tree edits"
            );
            let branch = state
                .store
                .record_active_branch_edit(&context.branch.id, None)
                .await?;
            Ok(Json(json!({
                "branch": branch,
                "workingTree": { "backend": "localWorkingTree" },
            })))
        }
    }
}

async fn branch_archive(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, false).await?;
    let branch = state
        .store
        .archive_active_branch(&context.branch.id)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    state.lsp.drop_sessions_for_branch(&context.branch.id).await;
    tracing::info!(
        operation = "branch.archive",
        principal_id = %context.user.principal_id,
        package_id = %context.package.id,
        branch_id = %context.branch.id,
        branch = %context.branch.branch,
        "console branch archived"
    );
    Ok(Json(json!({ "branch": branch })))
}

#[derive(serde::Deserialize)]
struct VariableSaveBody {
    #[serde(rename = "variableId")]
    variable_id: Option<String>,
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    value: Option<String>,
}

async fn branch_variable_save(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
    Json(body): Json<VariableSaveBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, true).await?;
    let variable_id = body
        .variable_id
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty());
    let file_path = body
        .file_path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty());
    let (Some(variable_id), Some(file_path), Some(value)) = (variable_id, file_path, body.value)
    else {
        return Err(ApiError::bad_request(
            "variableId, filePath, and value are required",
        ));
    };
    let expected = expected_variable_file_path(&context.package, &variable_id);
    if file_path != expected {
        return Err(ApiError::bad_request(
            "variable file path does not match package",
        ));
    }

    let backend = branch_backend(
        &state,
        &context.user,
        &context.package,
        "Saving the branch change",
    )?;
    let (current_text, sha) =
        branch_file_text_and_sha(&state, &context, &file_path, &backend).await?;
    let update = update_primitive_variable_default(&current_text, &value)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    if update.before_literal != update.after_literal {
        tracing::info!(
            operation = "branch.variable_save",
            principal_id = %context.user.principal_id,
            package_id = %context.package.id,
            branch_id = %context.branch.id,
            variable_id = %variable_id,
            file_path = %file_path,
            changed = true,
            "console branch variable save writing change"
        );
        write_branch_file(
            &state,
            &context,
            &file_path,
            &update.text,
            sha.as_deref(),
            &format!("Update {variable_id} default value"),
        )
        .await?;
        record_branch_edit(&state, &context, None).await?;
        invalidate_branch(&state, &context.user, &context.package, &context.branch).await;
    } else {
        tracing::info!(
            operation = "branch.variable_save",
            principal_id = %context.user.principal_id,
            package_id = %context.package.id,
            branch_id = %context.branch.id,
            variable_id = %variable_id,
            file_path = %file_path,
            changed = false,
            "console branch variable save skipped unchanged value"
        );
    }

    Ok(Json(json!({
        "ok": true,
        "targetPath": variable_default_target_path(),
    })))
}

#[derive(serde::Deserialize)]
struct FileSaveBody {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    content: Option<String>,
}

async fn branch_file_save(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
    Json(body): Json<FileSaveBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, true).await?;
    let file_path = body
        .file_path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty());
    let (Some(file_path), Some(content)) = (file_path, body.content) else {
        return Err(ApiError::bad_request("filePath and content are required"));
    };
    if !belongs_to_package(&context.package.path, &file_path) {
        return Err(ApiError::bad_request(
            "file path does not belong to package",
        ));
    }

    let backend = branch_backend(
        &state,
        &context.user,
        &context.package,
        "Saving the branch file",
    )?;
    let (current_text, sha) =
        branch_file_text_and_sha(&state, &context, &file_path, &backend).await?;
    if current_text != content {
        tracing::info!(
            operation = "branch.file_save",
            principal_id = %context.user.principal_id,
            package_id = %context.package.id,
            branch_id = %context.branch.id,
            file_path = %file_path,
            changed = true,
            "console branch file save writing change"
        );
        write_branch_file(
            &state,
            &context,
            &file_path,
            &content,
            sha.as_deref(),
            &format!("Update {file_path}"),
        )
        .await?;
        record_branch_edit(&state, &context, None).await?;
        invalidate_branch(&state, &context.user, &context.package, &context.branch).await;
    } else {
        tracing::info!(
            operation = "branch.file_save",
            principal_id = %context.user.principal_id,
            package_id = %context.package.id,
            branch_id = %context.branch.id,
            file_path = %file_path,
            changed = false,
            "console branch file save skipped unchanged content"
        );
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
struct FileDeleteBody {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
}

async fn branch_file_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
    Json(body): Json<FileDeleteBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, true).await?;
    let Some(file_path) = body
        .file_path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
    else {
        return Err(ApiError::bad_request("filePath is required"));
    };
    if !belongs_to_package(&context.package.path, &file_path) {
        return Err(ApiError::bad_request(
            "file path does not belong to package",
        ));
    }

    match branch_backend(
        &state,
        &context.user,
        &context.package,
        "Deleting the branch file",
    )? {
        BranchBackend::GitHub { token, .. } => {
            let github_repo = context_github_repo(&context)?;
            tracing::info!(
                operation = "branch.file_delete",
                principal_id = %context.user.principal_id,
                package_id = %context.package.id,
                branch_id = %context.branch.id,
                file_path = %file_path,
                "console branch file delete writing change"
            );
            let file = state
                .github
                .file(
                    token,
                    &github_repo.owner,
                    &github_repo.name,
                    &file_path,
                    &context.branch.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Deleting the branch file"))?;
            state
                .github
                .delete_file(
                    token,
                    &github_repo.owner,
                    &github_repo.name,
                    &file_path,
                    &context.branch.branch,
                    &file.sha,
                    &format!("Delete {file_path}"),
                )
                .await
                .map_err(|err| ApiError::github(&err, "Deleting the branch file"))?;
        }
        BranchBackend::LocalWorkingTree => {
            tracing::info!(
                operation = "branch.file_delete",
                principal_id = %context.user.principal_id,
                package_id = %context.package.id,
                branch_id = %context.branch.id,
                file_path = %file_path,
                "console branch file delete removing local file"
            );
            delete_local_file(&state, &context.package, &file_path).await?;
        }
    }
    record_branch_edit(&state, &context, None).await?;
    invalidate_branch(&state, &context.user, &context.package, &context.branch).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
struct EntityCreateBody {
    kind: Option<String>,
    id: Option<String>,
    #[serde(rename = "catalogId")]
    catalog_id: Option<String>,
    #[serde(rename = "variableType")]
    variable_type: Option<String>,
}

async fn branch_entity_create(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
    Json(body): Json<EntityCreateBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, true).await?;
    let kind = body.kind.as_deref().and_then(parse_kind);
    let id = parse_entity_id(body.id.as_deref());
    let catalog_id = parse_entity_id(body.catalog_id.as_deref());
    let (Some(kind), Some(id)) = (kind, id) else {
        return Err(invalid_entity_request());
    };
    if kind == EntityKind::CatalogEntries && catalog_id.is_none() {
        return Err(invalid_entity_request());
    }

    let variable_type = parse_variable_type(body.variable_type.as_deref());
    let files = entity_template_files(
        kind,
        &id,
        catalog_id.as_deref(),
        &context.package.path,
        &variable_type,
    );
    let backend = branch_backend(
        &state,
        &context.user,
        &context.package,
        "Creating the branch entity",
    )?;
    let existing: HashSet<String> = match &backend {
        BranchBackend::GitHub { token, .. } => {
            let github_repo = context_github_repo(&context)?;
            state
                .github
                .tree(
                    token,
                    &github_repo.owner,
                    &github_repo.name,
                    &context.branch.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Creating the branch entity"))?
                .into_iter()
                .filter(|entry| entry.entry_type == "blob")
                .map(|entry| entry.path)
                .collect()
        }
        BranchBackend::LocalWorkingTree => {
            let mut existing = HashSet::new();
            for file in &files {
                if local_file_exists(&state, &context.package, &file.path).await? {
                    existing.insert(file.path.clone());
                }
            }
            if kind == EntityKind::CatalogEntries {
                let catalog_id = catalog_id.as_deref().expect("validated above");
                let catalog_path = package_repo_path(
                    &context.package.path,
                    &format!("catalogs/{catalog_id}.schema.json"),
                );
                if local_file_exists(&state, &context.package, &catalog_path).await? {
                    existing.insert(catalog_path);
                }
            }
            existing
        }
    };
    if kind == EntityKind::CatalogEntries {
        let catalog_id = catalog_id.as_deref().expect("validated above");
        let catalog_path = package_repo_path(
            &context.package.path,
            &format!("catalogs/{catalog_id}.schema.json"),
        );
        if !existing.contains(&catalog_path) {
            return Err(ApiError::not_found(format!(
                "catalog does not exist: {catalog_id}"
            )));
        }
    }
    if let Some(conflict) = files.iter().find(|file| existing.contains(&file.path)) {
        return Err(ApiError {
            status: axum::http::StatusCode::CONFLICT,
            message: format!("file already exists: {}", conflict.path),
        });
    }

    match branch_backend(
        &state,
        &context.user,
        &context.package,
        "Creating the branch entity",
    )? {
        BranchBackend::GitHub { token, .. } => {
            let github_repo = context_github_repo(&context)?;
            for file in &files {
                state
                    .github
                    .create_file(
                        token,
                        &github_repo.owner,
                        &github_repo.name,
                        &file.path,
                        &context.branch.branch,
                        &file.content,
                        &format!("Create {}", file.path),
                    )
                    .await
                    .map_err(|err| ApiError::github(&err, "Creating the branch entity"))?;
            }
        }
        BranchBackend::LocalWorkingTree => {
            for file in &files {
                write_local_file(&state, &context.package, &file.path, &file.content).await?;
            }
        }
    }
    record_branch_edit(&state, &context, None).await?;
    invalidate_branch(&state, &context.user, &context.package, &context.branch).await;
    Ok(Json(json!({ "files": files })))
}

#[derive(serde::Deserialize)]
struct LspBody {
    op: Option<String>,
    path: Option<String>,
    text: Option<String>,
    position: Option<JsonValue>,
}

async fn branch_lsp(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
    Json(body): Json<LspBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, true).await?;
    let path = body
        .path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty());
    let (Some(path), Some(text)) = (path, body.text) else {
        return Err(ApiError::bad_request("path and text are required"));
    };
    if !belongs_to_package(&context.package.path, &path) {
        return Err(ApiError::bad_request(
            "file path does not belong to package",
        ));
    }

    let staged =
        inspect_branch_package(&state, &context.user, &context.package, &context.branch).await?;

    let op = body.op.as_deref().unwrap_or("unknown").to_owned();
    let lsp_started = std::time::Instant::now();
    let result: ApiResult<JsonValue> = match (body.op.as_deref(), body.position) {
        (Some("update"), _) => {
            let diagnostics = state
                .lsp
                .update(
                    &context.user.principal_id,
                    &context.branch.id,
                    staged,
                    &context.package,
                    &path,
                    &text,
                )
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            Ok(json!({ "diagnostics": diagnostics }))
        }
        (Some("completion"), Some(position)) => {
            let items = state
                .lsp
                .completion(
                    &context.user.principal_id,
                    &context.branch.id,
                    staged,
                    &context.package,
                    &path,
                    &text,
                    position,
                )
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            Ok(json!({ "items": items }))
        }
        (Some("hover"), Some(position)) => {
            let hover = state
                .lsp
                .hover(
                    &context.user.principal_id,
                    &context.branch.id,
                    staged,
                    &context.package,
                    &path,
                    &text,
                    position,
                )
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            Ok(json!({ "hover": hover }))
        }
        _ => Err(ApiError::bad_request("unknown lsp op")),
    };
    if let Some(observability) = &state.observability {
        observability
            .record_operation(
                &format!("lsp.{op}"),
                lsp_started.elapsed().as_millis(),
                result.is_ok(),
                json!({
                    "package_id": package_id,
                    "branch_id": branch_id,
                    "path": path,
                }),
            )
            .await;
    }
    let result = result?;
    Ok(Json(result))
}

async fn branch_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, false).await?;
    let BranchContext {
        user,
        package,
        github_repo,
        mut branch,
    } = context;

    let mut pr_sync_error: Option<String> = None;
    let pr_number = branch
        .pr_number
        .or_else(|| pull_request_number_from_url(branch.pr_url.as_deref()));
    let synced_recently = branch
        .pr_synced_at
        .as_deref()
        .is_some_and(|synced_at| synced_at > super::time::now_iso_minus(PR_SYNC_FRESH).as_str());
    if let Some(pr_number) = pr_number
        && !synced_recently
        && let Some(github_repo) = github_repo.as_ref()
    {
        match sync_pull_request(&state, &user, github_repo, &branch, pr_number).await {
            Ok(updated) => branch = updated,
            Err(error) => {
                pr_sync_error = Some(error);
            }
        }
    }

    let token = source_token(&user);
    let package_source =
        package_source_for_branch(&state, &user.principal_id, token, &package, &branch.branch)
            .await;
    let staged = match package_source {
        Ok(source) => state.stage.get_semantic_package(source, token).await,
        Err(err) => Err(crate::error::RototoError::new(err.message)),
    };
    let (entities, edit_load_error, lint, model) = match &staged {
        Ok(semantic) => {
            let lint = match semantic.package.lint().await {
                Ok(lint) => lint_json(&lint),
                Err(err) => lint_error_json(&branch.branch, &err.to_string()),
            };
            match inspect_package_inventory(&package, &semantic.model, semantic.package.root())
                .await
            {
                Ok(inventory) => {
                    match editable_entities(&package, semantic.package.root(), &inventory).await {
                        Ok(entities) => (
                            entities,
                            JsonValue::Null,
                            lint,
                            serde_json::to_value(semantic.model.as_ref())
                                .expect("model serializes"),
                        ),
                        Err(err) => (
                            Vec::new(),
                            json!(err.to_string()),
                            lint,
                            serde_json::to_value(semantic.model.as_ref())
                                .expect("model serializes"),
                        ),
                    }
                }
                Err(err) => (
                    Vec::new(),
                    json!(err.to_string()),
                    lint,
                    serde_json::to_value(semantic.model.as_ref()).expect("model serializes"),
                ),
            }
        }
        Err(err) => {
            let message = err.to_string();
            (
                Vec::new(),
                json!(message.clone()),
                lint_error_json(&branch.branch, &message),
                JsonValue::Null,
            )
        }
    };

    let edited_paths = branch_changed_paths(&state, &user, &package, &branch)
        .await
        .unwrap_or_default();
    let changes: Vec<BranchFileChange> = edited_paths
        .iter()
        .map(|path| BranchFileChange {
            id: path.clone(),
            file_path: path.clone(),
        })
        .collect();

    let capabilities = package_capabilities_json(&state, &user, &package);
    Ok(Json(json!({
        "package": package,
        "branch": branch,
        "prSyncError": pr_sync_error,
        "changes": changes,
        "lint": lint,
        "model": model,
        "entities": entities,
        "editLoadError": edit_load_error,
        "editedPaths": edited_paths,
        "sourceKind": capabilities["sourceKind"].clone(),
        "capabilities": capabilities["capabilities"].clone(),
    })))
}

async fn branch_entity(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((package_id, branch_id)): Path<(String, String)>,
    Query(query): Query<EntityQuery>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &package_id, &branch_id, false).await?;
    let base_text = base_entity_text(&state, &context, &query.path).await;

    let mut context_previews = JsonValue::Array(Vec::new());
    let token = source_token(&context.user);
    if let Ok(source) = package_source_for_branch(
        &state,
        &context.user.principal_id,
        token,
        &context.package,
        &context.branch.branch,
    )
    .await
        && let Ok(semantic) = state.stage.get_semantic_package(source, token).await
        && let Ok(inventory) =
            inspect_package_inventory(&context.package, &semantic.model, semantic.package.root())
                .await
        && inventory
            .variables
            .iter()
            .any(|variable| variable.path == query.path)
    {
        let runtime = match package_source_for_branch(
            &state,
            &context.user.principal_id,
            token,
            &context.package,
            &context.branch.branch,
        )
        .await
        {
            Ok(source) => state.stage.get_runtime_package(source, token).await.ok(),
            Err(_) => runtime_package_for_base(
                &state,
                &context.user.principal_id,
                token,
                &context.package,
            )
            .await
            .ok(),
        };
        if let Some(runtime) = runtime {
            let qualifier_ids: Vec<String> = inventory
                .qualifiers
                .iter()
                .map(|qualifier| qualifier.id.clone())
                .collect();
            let contexts = load_saved_contexts(
                &context.package,
                semantic.package.root(),
                &inventory,
                MAX_PREVIEW_CONTEXTS,
            )
            .await;
            if !qualifier_ids.is_empty() && !contexts.is_empty() {
                let previews = edit_context_previews(&runtime, &qualifier_ids, &contexts).await;
                context_previews = serde_json::to_value(previews).expect("previews serialize");
            }
        }
    }

    Ok(Json(json!({
        "baseText": base_text,
        "contextPreviews": context_previews,
    })))
}

async fn branch_changed_paths(
    state: &ConsoleState,
    user: &SessionUser,
    package: &PackageRecord,
    branch: &ActiveBranchRecord,
) -> ApiResult<Vec<String>> {
    if context_is_github_package(package) {
        let token = require_github_token(user, "Loading branch changes")?;
        let github_repo = github_repo_for_package(package)
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
        let comparison = state
            .github
            .compare_refs(
                token,
                &github_repo.owner,
                &github_repo.name,
                &branch.base_ref,
                &branch.branch,
            )
            .await
            .map_err(|err| ApiError::github(&err, "Loading branch changes"))?;
        return Ok(filter_package_paths(package, comparison.files));
    }
    if context_is_local_package(package) {
        let root = local_source_root(state, package).await?;
        let scope = local_package_scope(state, package);
        let paths = local_git::changed_paths(&root, &scope)
            .await
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
        return Ok(filter_package_paths(package, paths));
    }
    let selector = package_source_for_branch(
        state,
        &user.principal_id,
        source_token(user),
        package,
        &branch.branch,
    )
    .await?;
    let cached_tree = selector
        .cached_source_tree_origin()
        .map_err(|err| ApiError::internal(err.to_string()))?;
    let branch_name =
        BranchName::new(&branch.branch).map_err(|err| ApiError::bad_request(err.to_string()))?;
    let base_ref =
        GitRefName::new(&branch.base_ref).map_err(|err| ApiError::bad_request(err.to_string()))?;
    let changes = state
        .stage
        .get_branch_changes(cached_tree, branch_name, base_ref)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    Ok(filter_package_paths(
        package,
        changes
            .changed_files
            .into_iter()
            .map(|path| path.as_str().to_owned())
            .collect(),
    ))
}

fn filter_package_paths(package: &PackageRecord, paths: Vec<String>) -> Vec<String> {
    let prefix = if package.path == "." {
        String::new()
    } else {
        format!("{}/", package.path)
    };
    paths
        .into_iter()
        .filter(|path| prefix.is_empty() || path.starts_with(&prefix))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn local_source_root(state: &ConsoleState, package: &PackageRecord) -> ApiResult<PathBuf> {
    let source = state
        .fixed_package_source
        .as_deref()
        .unwrap_or(&package.source);
    let root =
        local_git::package_root(source).map_err(|err| ApiError::bad_request(err.to_string()))?;
    tokio::fs::canonicalize(&root).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to resolve local package source {}: {err}",
            root.display()
        ))
    })
}

fn local_package_scope(state: &ConsoleState, package: &PackageRecord) -> String {
    if state.fixed_package_source.is_some() {
        package.path.clone()
    } else {
        ".".to_owned()
    }
}

fn local_relative_path(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<String> {
    let relative = if state.fixed_package_source.is_some() || package.path == "." {
        file_path.trim()
    } else {
        file_path
            .strip_prefix(&format!("{}/", package.path))
            .ok_or_else(|| ApiError::bad_request("file path does not belong to package"))?
    };
    if relative.is_empty()
        || relative.starts_with('/')
        || relative
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ApiError::bad_request("file path is not valid"));
    }
    Ok(relative.to_owned())
}

async fn local_existing_file_path(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<PathBuf> {
    let root = local_source_root(state, package).await?;
    let relative = local_relative_path(state, package, file_path)?;
    let path = root.join(relative);
    let canonical = tokio::fs::canonicalize(&path).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ApiError::not_found(format!("file not found: {file_path}"))
        } else {
            ApiError::bad_request(format!("failed to resolve {file_path}: {err}"))
        }
    })?;
    if !canonical.starts_with(&root) {
        return Err(ApiError::bad_request(
            "file path escapes the local package source",
        ));
    }
    Ok(canonical)
}

async fn local_writable_file_path(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<PathBuf> {
    let root = local_source_root(state, package).await?;
    let relative = local_relative_path(state, package, file_path)?;
    ensure_local_parent_dir(&root, &relative).await?;
    let path = root.join(relative);
    if let Ok(metadata) = tokio::fs::symlink_metadata(&path).await
        && metadata.file_type().is_symlink()
    {
        return Err(ApiError::bad_request(
            "local package edits do not follow symlink files",
        ));
    }
    Ok(path)
}

async fn ensure_local_parent_dir(root: &FsPath, relative: &str) -> ApiResult<()> {
    let parent = FsPath::new(relative)
        .parent()
        .ok_or_else(|| ApiError::bad_request("file path must have a parent directory"))?;
    let mut current = root.to_path_buf();
    for component in parent.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(ApiError::bad_request("file path is not valid"));
        };
        current.push(segment);
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ApiError::bad_request(
                        "local package edits do not follow symlink directories",
                    ));
                }
                if !metadata.is_dir() {
                    return Err(ApiError::bad_request(format!(
                        "local package path is not a directory: {}",
                        current.display()
                    )));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&current).await.map_err(|err| {
                    ApiError::bad_request(format!(
                        "failed to create parent directory {}: {err}",
                        current.display()
                    ))
                })?;
            }
            Err(err) => {
                return Err(ApiError::bad_request(format!(
                    "failed to resolve parent directory {}: {err}",
                    current.display()
                )));
            }
        }
    }
    let canonical_parent = tokio::fs::canonicalize(&current).await.map_err(|err| {
        ApiError::bad_request(format!("failed to resolve parent directory: {err}"))
    })?;
    if !canonical_parent.starts_with(root) {
        return Err(ApiError::bad_request(
            "file path escapes the local package source",
        ));
    }
    Ok(())
}

async fn local_file_exists(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<bool> {
    match local_existing_file_path(state, package, file_path).await {
        Ok(path) => Ok(path.is_file()),
        Err(err) if err.status == axum::http::StatusCode::NOT_FOUND => Ok(false),
        Err(err) => Err(err),
    }
}

async fn read_local_file(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<String> {
    let path = local_existing_file_path(state, package, file_path).await?;
    tokio::fs::read_to_string(&path).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to read local file {}: {err}",
            path.display()
        ))
    })
}

async fn write_local_file(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
    content: &str,
) -> ApiResult<()> {
    let path = local_writable_file_path(state, package, file_path).await?;
    tokio::fs::write(&path, content).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to write local file {}: {err}",
            path.display()
        ))
    })
}

async fn delete_local_file(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<()> {
    let path = local_existing_file_path(state, package, file_path).await?;
    tokio::fs::remove_file(&path).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to delete local file {}: {err}",
            path.display()
        ))
    })
}

async fn branch_file_text_and_sha(
    state: &ConsoleState,
    context: &BranchContext,
    file_path: &str,
    backend: &BranchBackend<'_>,
) -> ApiResult<(String, Option<String>)> {
    match backend {
        BranchBackend::GitHub { token, .. } => {
            let github_repo = context_github_repo(context)?;
            let file = state
                .github
                .file(
                    token,
                    &github_repo.owner,
                    &github_repo.name,
                    file_path,
                    &context.branch.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Reading the branch file"))?;
            Ok((file.content, Some(file.sha)))
        }
        BranchBackend::LocalWorkingTree => {
            let text = read_local_file(state, &context.package, file_path).await?;
            Ok((text, None))
        }
    }
}

async fn write_branch_file(
    state: &ConsoleState,
    context: &BranchContext,
    file_path: &str,
    content: &str,
    sha: Option<&str>,
    message: &str,
) -> ApiResult<()> {
    match branch_backend(
        state,
        &context.user,
        &context.package,
        "Writing the branch file",
    )? {
        BranchBackend::GitHub { token, .. } => {
            let github_repo = context_github_repo(context)?;
            state
                .github
                .update_file(
                    token,
                    &github_repo.owner,
                    &github_repo.name,
                    file_path,
                    &context.branch.branch,
                    sha.expect("GitHub file reads include a sha"),
                    content,
                    message,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Writing the branch file"))?;
        }
        BranchBackend::LocalWorkingTree => {
            let _ = message;
            let _ = sha;
            write_local_file(state, &context.package, file_path, content).await?;
        }
    }
    Ok(())
}

async fn record_branch_edit(
    state: &ConsoleState,
    context: &BranchContext,
    commit: Option<String>,
) -> ApiResult<ActiveBranchRecord> {
    let last_seen_commit = match commit {
        Some(commit) => Some(commit),
        None if context_is_github_package(&context.package) => {
            match context.user.github_token.as_deref() {
                Some(token) => match context_github_repo(context) {
                    Ok(github_repo) => state
                        .github
                        .branch_head_sha(
                            token,
                            &github_repo.owner,
                            &github_repo.name,
                            &context.branch.branch,
                        )
                        .await
                        .ok(),
                    Err(_) => None,
                },
                None => None,
            }
        }
        None => None,
    };
    state
        .store
        .record_active_branch_edit(&context.branch.id, last_seen_commit)
        .await
        .map_err(ApiError::from)
}

async fn base_entity_text(
    state: &ConsoleState,
    context: &BranchContext,
    path: &str,
) -> Option<String> {
    if context_is_local_package(&context.package) {
        let root = local_source_root(state, &context.package).await.ok()?;
        let relative = local_relative_path(state, &context.package, path).ok()?;
        return local_git::file_at_head(&root, &relative)
            .await
            .ok()
            .flatten();
    }
    let token = context.user.github_token.as_deref()?;
    let github_repo = context_github_repo(context).ok()?;
    state
        .github
        .file(
            token,
            &github_repo.owner,
            &github_repo.name,
            path,
            &context.branch.base_ref,
        )
        .await
        .ok()
        .map(|file| file.content)
}

async fn editable_entities(
    package: &PackageRecord,
    staged_root: &std::path::Path,
    inventory: &PackageInventory,
) -> Result<Vec<JsonValue>> {
    struct Node {
        section: &'static str,
        id: String,
        kind: &'static str,
        path: String,
        description: Option<String>,
        badge: Option<String>,
        catalog_id: Option<String>,
        entry_key: Option<String>,
    }

    let mut nodes = Vec::new();
    for item in &inventory.variables {
        nodes.push(Node {
            section: "variables",
            id: item.id.clone(),
            kind: "variable",
            path: item.path.clone(),
            description: item.description.clone(),
            badge: Some(item.declaration.clone()),
            catalog_id: None,
            entry_key: None,
        });
    }
    for item in &inventory.qualifiers {
        nodes.push(Node {
            section: "qualifiers",
            id: item.id.clone(),
            kind: "qualifier",
            path: item.path.clone(),
            description: item.description.clone(),
            badge: Some("condition".to_owned()),
            catalog_id: None,
            entry_key: None,
        });
    }
    for item in &inventory.catalogs {
        nodes.push(Node {
            section: "catalogs",
            id: item.id.clone(),
            kind: "catalog",
            path: item.path.clone(),
            description: item.description.clone(),
            badge: Some(format!("{} values", item.entry_count)),
            catalog_id: None,
            entry_key: None,
        });
    }
    for item in &inventory.catalog_entries {
        nodes.push(Node {
            section: "catalogs",
            id: item.id.clone(),
            kind: "catalog value",
            path: item.path.clone(),
            description: Some(format!(
                "Value {} for catalog {}",
                item.key, item.catalog_id
            )),
            badge: Some(item.catalog_id.clone()),
            catalog_id: Some(item.catalog_id.clone()),
            entry_key: Some(item.key.clone()),
        });
    }
    for item in &inventory.linters {
        let Some(path) = item.path.clone() else {
            continue;
        };
        nodes.push(Node {
            section: "linters",
            id: item.id.clone(),
            kind: "linter",
            path,
            description: item.title.clone(),
            badge: Some(item.kind.to_owned()),
            catalog_id: None,
            entry_key: None,
        });
    }
    for item in &inventory.context.request_contexts {
        nodes.push(Node {
            section: "context",
            id: item.id.clone(),
            kind: "context schema",
            path: item.path.clone(),
            description: item
                .description
                .clone()
                .or_else(|| item.title.clone())
                .or_else(|| Some("Request context schema".to_owned())),
            badge: Some("schema".to_owned()),
            catalog_id: None,
            entry_key: None,
        });
    }
    for item in &inventory.context.entries {
        nodes.push(Node {
            section: "context",
            id: item.id.clone(),
            kind: "context example",
            path: item.path.clone(),
            description: Some(format!(
                "Sample {} for request context {}",
                item.key, item.request_context_id
            )),
            badge: Some(item.request_context_id.clone()),
            catalog_id: None,
            entry_key: None,
        });
    }

    let mut entities = Vec::with_capacity(nodes.len());
    for node in nodes {
        let definition = read_package_definition(package, staged_root, &node.path).await?;
        entities.push(json!({
            "section": node.section,
            "id": node.id,
            "kind": node.kind,
            "path": node.path,
            "description": node.description,
            "badge": node.badge,
            "text": definition.text,
            "language": language_for_path(&node.path),
            "catalogId": node.catalog_id,
            "entryKey": node.entry_key,
        }));
    }
    Ok(entities)
}

fn parse_kind(value: &str) -> Option<EntityKind> {
    match value {
        "variables" => Some(EntityKind::Variables),
        "qualifiers" => Some(EntityKind::Qualifiers),
        "catalogs" => Some(EntityKind::Catalogs),
        "catalog_entries" => Some(EntityKind::CatalogEntries),
        "context" => Some(EntityKind::Context),
        "linters" => Some(EntityKind::Linters),
        _ => None,
    }
}

fn invalid_entity_request() -> ApiError {
    ApiError::bad_request(
        "kind and id are required; catalog value creation also requires catalogId. ids may \
         contain letters, numbers, dot, dash, and underscore",
    )
}

fn pull_request_number_from_url(url: Option<&str>) -> Option<i64> {
    let url = url?;
    let (_, rest) = url.split_once("/pull/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let after = &rest[digits.len()..];
    if after.is_empty()
        || after.starts_with('/')
        || after.starts_with('?')
        || after.starts_with('#')
    {
        digits.parse().ok()
    } else {
        None
    }
}

fn pull_request_state(state: Option<&str>, merged_at: Option<&str>) -> String {
    if merged_at.is_some() {
        return "merged".to_owned();
    }
    state.unwrap_or("unknown").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn package(path: &str) -> PackageRecord {
        PackageRecord {
            id: "package-id".to_owned(),
            slug: "configs".to_owned(),
            source_tree_id: "repo-id".to_owned(),
            source_tree_label: "octo/configs".to_owned(),
            display_path: path.to_owned(),
            path: path.to_owned(),
            revision: "main".to_owned(),
            source: "git+https://github.com/octo/configs.git#main".to_owned(),
            discovered_at: "2026-06-13T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn branch_select_body_accepts_empty_request() {
        let body = parse_branch_select_body(b"")
            .unwrap_or_else(|err| panic!("empty request body should be allowed: {}", err.message));
        assert_eq!(body.branch, None);
    }

    #[test]
    fn branch_select_body_parses_requested_branch() {
        let body = parse_branch_select_body(br#"{"branch":"rototo/config"}"#)
            .unwrap_or_else(|err| panic!("valid request body should parse: {}", err.message));
        assert_eq!(body.branch.as_deref(), Some("rototo/config"));
    }

    #[test]
    fn package_path_filter_keeps_only_selected_package() {
        assert_eq!(
            filter_package_paths(
                &package("apps/payments"),
                vec![
                    "apps/payments/variables/a.toml".to_owned(),
                    "apps/web/variables/b.toml".to_owned(),
                ],
            ),
            vec!["apps/payments/variables/a.toml".to_owned()]
        );
        assert_eq!(
            filter_package_paths(
                &package("."),
                vec!["variables/a.toml".to_owned(), "README.md".to_owned()],
            ),
            vec!["README.md".to_owned(), "variables/a.toml".to_owned()]
        );
    }

    #[tokio::test]
    async fn local_parent_dir_creation_stays_under_root() {
        let root = TempDir::new().expect("temp dir");

        if let Err(error) =
            ensure_local_parent_dir(root.path(), "variables/nested/value.toml").await
        {
            panic!("parent directories should be created: {}", error.message);
        }

        assert!(root.path().join("variables/nested").is_dir());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_parent_dir_creation_rejects_symlink_ancestors() {
        let root = TempDir::new().expect("temp dir");
        let outside = TempDir::new().expect("outside temp dir");
        std::os::unix::fs::symlink(outside.path(), root.path().join("variables"))
            .expect("symlink should be created");

        let error = ensure_local_parent_dir(root.path(), "variables/nested/value.toml")
            .await
            .expect_err("symlink ancestors should be rejected");

        assert!(
            error.message.contains("symlink directories"),
            "{}",
            error.message
        );
        assert!(
            !outside.path().join("nested").exists(),
            "guard must not create directories through a symlink ancestor"
        );
    }

    #[test]
    fn pull_request_numbers_parse_from_urls() {
        assert_eq!(
            pull_request_number_from_url(Some("https://github.com/o/r/pull/42")),
            Some(42)
        );
        assert_eq!(
            pull_request_number_from_url(Some("https://github.com/o/r/pull/42/files")),
            Some(42)
        );
        assert_eq!(
            pull_request_number_from_url(Some("https://github.com/o/r/pull/42abc")),
            None
        );
        assert_eq!(pull_request_number_from_url(None), None);
    }
}
