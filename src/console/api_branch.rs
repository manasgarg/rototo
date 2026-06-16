use std::collections::{BTreeSet, HashSet};
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
use super::api_workspace::{
    EntityQuery, lint_error_json, lint_json, load_saved_contexts, load_workspace,
    workspace_capabilities_json,
};
use super::capabilities::{WorkspaceSourceKind, WritePolicy, classify_workspace_source};
use super::github::workspace_repo_path;
use super::inventory::{
    WorkspaceInventory, inspect_workspace_inventory, language_for_path, read_workspace_definition,
    workspace_local_path,
};
use super::local_git;
use super::resolve_preview::edit_context_previews;
use super::stage::{BranchName, GitRefName};
use super::store::{
    ActiveBranchRecord, ActiveBranchStatus, BranchPullRequestInput, SelectBranchInput, SessionUser,
    WorkspaceRecord,
};
use super::variable_toml::update_primitive_variable_default;
use super::workspace_edit::{
    EntityKind, belongs_to_workspace, branch_pr_body, branch_pr_title, console_branch_name,
    entity_template_files, expected_variable_file_path, parse_entity_id, parse_variable_type,
    variable_value_target_path,
};
use super::workspace_source::{runtime_workspace_for_base, workspace_source_for_branch};

const PR_SYNC_FRESH: Duration = Duration::from_secs(60);
const MAX_PREVIEW_CONTEXTS: usize = 4;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/workspaces/{workspace_id}/branches", post(branch_select))
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}",
            patch(branch_rename),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/data",
            get(branch_data),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/entity",
            get(branch_entity),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/sync-pr",
            post(branch_sync_pr),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/publish",
            post(branch_publish),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/archive",
            post(branch_archive),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/variables",
            post(branch_variable_save),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/files",
            post(branch_file_save).delete(branch_file_delete),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/entities",
            post(branch_entity_create),
        )
        .route(
            "/workspaces/{workspace_id}/branches/{branch_id}/lsp",
            post(branch_lsp),
        )
}

struct BranchContext {
    user: SessionUser,
    workspace: WorkspaceRecord,
    branch: ActiveBranchRecord,
}

enum BranchBackend<'a> {
    GitHub { token: &'a str, direct: bool },
    LocalGit,
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
    workspace: &WorkspaceRecord,
    action: &str,
) -> ApiResult<BranchBackend<'a>> {
    let kind = classify_workspace_source(&workspace.source);
    match state.write_policy {
        WritePolicy::Disabled => Err(ApiError::bad_request(format!(
            "{action} is disabled for this console"
        ))),
        WritePolicy::PullRequest => match kind {
            WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit => {
                let token = require_github_token(user, action)?;
                Ok(BranchBackend::GitHub {
                    token,
                    direct: false,
                })
            }
            _ => Err(ApiError::bad_request(
                "pull-request writes are only implemented for GitHub workspaces",
            )),
        },
        WritePolicy::DirectPush => match kind {
            WorkspaceSourceKind::LocalPath | WorkspaceSourceKind::FileUrl => {
                Ok(BranchBackend::LocalGit)
            }
            WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit => {
                let token = require_github_token(user, action)?;
                Ok(BranchBackend::GitHub {
                    token,
                    direct: true,
                })
            }
            _ => Err(ApiError::bad_request(
                "direct-push writes are not implemented for this workspace source",
            )),
        },
    }
}

fn context_is_github_workspace(workspace: &WorkspaceRecord) -> bool {
    matches!(
        classify_workspace_source(&workspace.source),
        WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit
    )
}

async fn load_branch(
    state: &ConsoleState,
    headers: &HeaderMap,
    workspace_id: &str,
    branch_id: &str,
    require_active: bool,
) -> ApiResult<BranchContext> {
    let user = require_user(state, headers).await?;
    let workspace = load_workspace(state, &user, workspace_id).await?;
    let branch = state
        .store
        .get_active_branch_for_user(branch_id, &workspace.id, &user.principal_id)
        .await?
        .ok_or_else(|| ApiError::not_found("branch not found"))?;
    if require_active && branch.status != ActiveBranchStatus::Active {
        return Err(ApiError::bad_request("branch is not active"));
    }
    Ok(BranchContext {
        user,
        workspace,
        branch,
    })
}

async fn invalidate_branch(
    state: &ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
    branch: &ActiveBranchRecord,
) {
    state.lsp.drop_sessions_for_branch(&branch.id).await;
    if let Ok(source) = workspace_source_for_branch(
        state,
        &user.principal_id,
        source_token(user),
        workspace,
        &branch.branch,
    )
    .await
    {
        if let Ok(cached_tree) = source.cached_source_tree_origin() {
            state
                .stage
                .invalidate_branch(&cached_tree, &branch.branch)
                .await;
        } else {
            state.stage.invalidate_workspace(&source).await;
        }
    }
}

async fn inspect_branch_workspace(
    state: &ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
    branch: &ActiveBranchRecord,
) -> ApiResult<Arc<crate::sdk::Workspace>> {
    let token = source_token(user);
    let workspace_source =
        workspace_source_for_branch(state, &user.principal_id, token, workspace, &branch.branch)
            .await?;
    state
        .stage
        .get_inspected_workspace(workspace_source, token)
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
    Path(workspace_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    let requested_branch = parse_branch_select_body(&body)?
        .branch
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty());

    let base_ref = workspace.git_ref.clone();
    let backend = branch_backend(&state, &user, &workspace, "Opening a branch")?;
    let target = branch_selection_target(
        &state,
        &user,
        &workspace,
        &backend,
        requested_branch,
        &base_ref,
    )
    .await?;

    if let Some(existing) = state
        .store
        .find_active_branch_for_repo_branch(&workspace.id, &user.principal_id, &target.branch)
        .await?
    {
        let existing = state
            .store
            .ensure_active_branch_workspace(&existing.id, &workspace.id, &user.principal_id)
            .await?;
        return Ok(Json(json!({ "branch": existing })));
    }

    let branch = state
        .store
        .select_branch(SelectBranchInput {
            workspace_id: workspace.id.clone(),
            principal_id: user.principal_id.clone(),
            branch: target.branch,
            base_ref,
            base_commit: target.base_commit,
            last_seen_commit: target.last_seen_commit,
        })
        .await?;
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
    workspace: &WorkspaceRecord,
    backend: &BranchBackend<'a>,
    requested_branch: Option<String>,
    base_ref: &str,
) -> ApiResult<BranchSelectionTarget> {
    match backend {
        BranchBackend::LocalGit => {
            let branch = local_git::current_branch(&workspace.source)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            if let Some(requested) = requested_branch.as_deref()
                && requested != branch
            {
                return Err(ApiError::bad_request(format!(
                    "local workspace is on branch {branch}, not {requested}"
                )));
            }
            let last_seen_commit = local_git::head_sha(&workspace.source).await.ok();
            Ok(BranchSelectionTarget {
                branch,
                base_commit: None,
                last_seen_commit,
            })
        }
        BranchBackend::GitHub {
            token,
            direct: true,
        } => {
            if let Some(requested) = requested_branch.as_deref()
                && requested != base_ref
            {
                return Err(ApiError::bad_request(format!(
                    "direct-push branches write to configured ref {base_ref}, not {requested}"
                )));
            }
            state
                .github
                .assert_repo_write_access(token, &workspace.owner, &workspace.name)
                .await
                .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
            let sha = state
                .github
                .branch_head_sha(token, &workspace.owner, &workspace.name, base_ref)
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
            state
                .github
                .assert_repo_write_access(token, &workspace.owner, &workspace.name)
                .await
                .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
            let base_sha = state
                .github
                .branch_head_sha(token, &workspace.owner, &workspace.name, base_ref)
                .await
                .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
            let branch = match requested_branch {
                Some(branch) => {
                    if branch == base_ref {
                        return Err(ApiError::bad_request(format!(
                            "Editing {base_ref} directly would skip review. Pick another branch."
                        )));
                    }
                    state
                        .github
                        .branch_head_sha(token, &workspace.owner, &workspace.name, &branch)
                        .await
                        .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
                    branch
                }
                None => {
                    let branch = console_branch_name(&user.identity.display_login(), workspace);
                    state
                        .github
                        .create_branch(token, &workspace.owner, &workspace.name, &branch, &base_sha)
                        .await
                        .map_err(|err| ApiError::github(&err, "Opening a branch"))?;
                    branch
                }
            };
            let last_seen_commit = state
                .github
                .branch_head_sha(token, &workspace.owner, &workspace.name, &branch)
                .await
                .ok();
            Ok(BranchSelectionTarget {
                branch,
                base_commit: Some(base_sha),
                last_seen_commit,
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
    Json(body): Json<BranchRenameBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, true).await?;
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
        &context.workspace,
        "Renaming the branch",
    )?
    else {
        return Err(ApiError::bad_request(
            "branch rename only applies to GitHub pull-request branches",
        ));
    };
    let renamed = state
        .github
        .rename_branch(
            token,
            &context.workspace.owner,
            &context.workspace.name,
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, false).await?;
    let BranchBackend::GitHub {
        token: _,
        direct: false,
    } = branch_backend(
        &state,
        &context.user,
        &context.workspace,
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
        &context.workspace,
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
    workspace: &WorkspaceRecord,
    branch: &ActiveBranchRecord,
    pr_number: i64,
) -> std::result::Result<ActiveBranchRecord, String> {
    let token = user
        .github_token
        .as_deref()
        .ok_or_else(|| "Syncing the pull request requires a GitHub credential".to_owned())?;
    let pr = state
        .github
        .pull_request(token, &workspace.owner, &workspace.name, pr_number)
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, true).await?;
    let changed_paths =
        branch_changed_paths(&state, &context.user, &context.workspace, &context.branch).await?;
    if changed_paths.is_empty() {
        return Err(ApiError::bad_request("branch has no changed files"));
    }

    let mut lint_workspaces = state
        .store
        .list_workspaces_for_active_branch(&context.branch.id)
        .await?;
    if lint_workspaces.is_empty() {
        lint_workspaces.push(context.workspace.clone());
    }
    let mut errors = 0;
    let mut warnings = 0;
    for workspace in &lint_workspaces {
        let inspected =
            inspect_branch_workspace(&state, &context.user, workspace, &context.branch).await?;
        let lint = inspected
            .lint()
            .await
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
        let workspace_errors = lint
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::diagnostics::Severity::Error)
            .count();
        errors += workspace_errors;
        warnings += lint.diagnostics.len() - workspace_errors;
    }
    if errors > 0 {
        return Err(ApiError::bad_request(format!(
            "branch has {errors} lint error(s) across included workspace(s); fix lint before publishing"
        )));
    }

    match branch_backend(
        &state,
        &context.user,
        &context.workspace,
        "Publishing the branch",
    )? {
        BranchBackend::GitHub {
            token,
            direct: false,
        } => {
            let pr = state
                .github
                .create_pull_request(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &branch_pr_title(&context.workspace),
                    &branch_pr_body(
                        &context.workspace,
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
            let branch = state
                .store
                .record_active_branch_edit(&context.branch.id, None)
                .await?;
            Ok(Json(json!({
                "branch": branch,
                "directPush": { "backend": "githubApi" },
            })))
        }
        BranchBackend::LocalGit => {
            let result = local_git::commit_and_push(
                &context.workspace,
                &changed_paths,
                &branch_pr_title(&context.workspace),
            )
            .await
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
            let branch = state
                .store
                .record_active_branch_edit(&context.branch.id, result.commit.clone())
                .await?;
            Ok(Json(json!({ "branch": branch, "directPush": result })))
        }
    }
}

async fn branch_archive(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, false).await?;
    let branch = state
        .store
        .archive_active_branch(&context.branch.id)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    state.lsp.drop_sessions_for_branch(&context.branch.id).await;
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
    Json(body): Json<VariableSaveBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, true).await?;
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
    let expected = expected_variable_file_path(&context.workspace, &variable_id);
    if file_path != expected {
        return Err(ApiError::bad_request(
            "variable file path does not match workspace",
        ));
    }

    let backend = branch_backend(
        &state,
        &context.user,
        &context.workspace,
        "Saving the branch change",
    )?;
    let (current_text, sha) =
        branch_file_text_and_sha(&state, &context, &file_path, &backend).await?;
    let update = update_primitive_variable_default(&current_text, &value)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    if update.before_literal != update.after_literal {
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
        invalidate_branch(&state, &context.user, &context.workspace, &context.branch).await;
    }

    Ok(Json(json!({
        "ok": true,
        "targetPath": variable_value_target_path(&update.value_key),
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
    Json(body): Json<FileSaveBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, true).await?;
    let file_path = body
        .file_path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty());
    let (Some(file_path), Some(content)) = (file_path, body.content) else {
        return Err(ApiError::bad_request("filePath and content are required"));
    };
    if !belongs_to_workspace(&context.workspace.path, &file_path) {
        return Err(ApiError::bad_request(
            "file path does not belong to workspace",
        ));
    }

    let backend = branch_backend(
        &state,
        &context.user,
        &context.workspace,
        "Saving the branch file",
    )?;
    let (current_text, sha) =
        branch_file_text_and_sha(&state, &context, &file_path, &backend).await?;
    if current_text != content {
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
        invalidate_branch(&state, &context.user, &context.workspace, &context.branch).await;
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
    Json(body): Json<FileDeleteBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, true).await?;
    let Some(file_path) = body
        .file_path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
    else {
        return Err(ApiError::bad_request("filePath is required"));
    };
    if !belongs_to_workspace(&context.workspace.path, &file_path) {
        return Err(ApiError::bad_request(
            "file path does not belong to workspace",
        ));
    }

    match branch_backend(
        &state,
        &context.user,
        &context.workspace,
        "Deleting the branch file",
    )? {
        BranchBackend::GitHub { token, .. } => {
            let file = state
                .github
                .file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &file_path,
                    &context.branch.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Deleting the branch file"))?;
            state
                .github
                .delete_file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &file_path,
                    &context.branch.branch,
                    &file.sha,
                    &format!("Delete {file_path}"),
                )
                .await
                .map_err(|err| ApiError::github(&err, "Deleting the branch file"))?;
        }
        BranchBackend::LocalGit => {
            local_git::delete_file(&context.workspace, &file_path)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
        }
    }
    record_branch_edit(&state, &context, None).await?;
    invalidate_branch(&state, &context.user, &context.workspace, &context.branch).await;
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
    Json(body): Json<EntityCreateBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, true).await?;
    let kind = body.kind.as_deref().and_then(parse_kind);
    let id = parse_entity_id(body.id.as_deref());
    let catalog_id = parse_entity_id(body.catalog_id.as_deref());
    let (Some(kind), Some(id)) = (kind, id) else {
        return Err(invalid_entity_request());
    };
    if kind == EntityKind::CatalogEntries && catalog_id.is_none() {
        return Err(invalid_entity_request());
    }

    let files = entity_template_files(
        kind,
        &id,
        catalog_id.as_deref(),
        &context.workspace.path,
        parse_variable_type(body.variable_type.as_deref()),
    );
    let backend = branch_backend(
        &state,
        &context.user,
        &context.workspace,
        "Creating the branch entity",
    )?;
    let existing: HashSet<String> = match backend {
        BranchBackend::GitHub { token, .. } => state
            .github
            .tree(
                token,
                &context.workspace.owner,
                &context.workspace.name,
                &context.branch.branch,
            )
            .await
            .map_err(|err| ApiError::github(&err, "Creating the branch entity"))?
            .into_iter()
            .filter(|entry| entry.entry_type == "blob")
            .map(|entry| entry.path)
            .collect(),
        BranchBackend::LocalGit => {
            let mut existing = HashSet::new();
            for file in &files {
                if local_git::file_exists(&context.workspace, &file.path)
                    .await
                    .map_err(|err| ApiError::bad_request(err.to_string()))?
                {
                    existing.insert(file.path.clone());
                }
            }
            if let Some(catalog_id) = catalog_id.as_deref() {
                let catalog_path = workspace_repo_path(
                    &context.workspace.path,
                    &format!("catalogs/{catalog_id}.toml"),
                );
                if local_git::file_exists(&context.workspace, &catalog_path)
                    .await
                    .map_err(|err| ApiError::bad_request(err.to_string()))?
                {
                    existing.insert(catalog_path);
                }
            }
            existing
        }
    };
    if kind == EntityKind::CatalogEntries {
        let catalog_id = catalog_id.as_deref().expect("validated above");
        let catalog_path = workspace_repo_path(
            &context.workspace.path,
            &format!("catalogs/{catalog_id}.toml"),
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
        &context.workspace,
        "Creating the branch entity",
    )? {
        BranchBackend::GitHub { token, .. } => {
            for file in &files {
                state
                    .github
                    .create_file(
                        token,
                        &context.workspace.owner,
                        &context.workspace.name,
                        &file.path,
                        &context.branch.branch,
                        &file.content,
                        &format!("Create {}", file.path),
                    )
                    .await
                    .map_err(|err| ApiError::github(&err, "Creating the branch entity"))?;
            }
        }
        BranchBackend::LocalGit => {
            for file in &files {
                local_git::write_file(&context.workspace, &file.path, &file.content)
                    .await
                    .map_err(|err| ApiError::bad_request(err.to_string()))?;
            }
        }
    }
    record_branch_edit(&state, &context, None).await?;
    invalidate_branch(&state, &context.user, &context.workspace, &context.branch).await;
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
    Json(body): Json<LspBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, true).await?;
    let path = body
        .path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty());
    let (Some(path), Some(text)) = (path, body.text) else {
        return Err(ApiError::bad_request("path and text are required"));
    };
    if !belongs_to_workspace(&context.workspace.path, &path) {
        return Err(ApiError::bad_request(
            "file path does not belong to workspace",
        ));
    }

    let staged =
        inspect_branch_workspace(&state, &context.user, &context.workspace, &context.branch)
            .await?;

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
                    &context.workspace,
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
                    &context.workspace,
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
                    &context.workspace,
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
                    "workspace_id": workspace_id,
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, false).await?;
    let BranchContext {
        user,
        workspace,
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
    {
        match sync_pull_request(&state, &user, &workspace, &branch, pr_number).await {
            Ok(updated) => branch = updated,
            Err(error) => pr_sync_error = Some(error),
        }
    }

    let token = source_token(&user);
    let workspace_source = workspace_source_for_branch(
        &state,
        &user.principal_id,
        token,
        &workspace,
        &branch.branch,
    )
    .await;
    let staged = match workspace_source {
        Ok(source) => state.stage.get_semantic_workspace(source, token).await,
        Err(err) => Err(crate::error::RototoError::new(err.message)),
    };
    let (entities, edit_load_error, lint, model) = match &staged {
        Ok(semantic) => {
            let lint = match semantic.workspace.lint().await {
                Ok(lint) => lint_json(&lint),
                Err(err) => lint_error_json(&branch.branch, &err.to_string()),
            };
            match inspect_workspace_inventory(
                &workspace,
                &semantic.model,
                semantic.workspace.root(),
            )
            .await
            {
                Ok(inventory) => {
                    match editable_entities(&workspace, semantic.workspace.root(), &inventory).await
                    {
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

    let edited_paths = branch_changed_paths(&state, &user, &workspace, &branch)
        .await
        .unwrap_or_default();
    let changes: Vec<BranchFileChange> = edited_paths
        .iter()
        .map(|path| BranchFileChange {
            id: path.clone(),
            file_path: path.clone(),
        })
        .collect();

    let capabilities = workspace_capabilities_json(&state, &user, &workspace);
    Ok(Json(json!({
        "workspace": workspace,
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
    Path((workspace_id, branch_id)): Path<(String, String)>,
    Query(query): Query<EntityQuery>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_branch(&state, &headers, &workspace_id, &branch_id, false).await?;
    let base_text = base_entity_text(&state, &context, &query.path).await;

    let mut context_previews = JsonValue::Array(Vec::new());
    let token = source_token(&context.user);
    if let Ok(source) = workspace_source_for_branch(
        &state,
        &context.user.principal_id,
        token,
        &context.workspace,
        &context.branch.branch,
    )
    .await
        && let Ok(semantic) = state.stage.get_semantic_workspace(source, token).await
        && let Ok(inventory) = inspect_workspace_inventory(
            &context.workspace,
            &semantic.model,
            semantic.workspace.root(),
        )
        .await
        && inventory
            .variables
            .iter()
            .any(|variable| variable.path == query.path)
    {
        let runtime = match workspace_source_for_branch(
            &state,
            &context.user.principal_id,
            token,
            &context.workspace,
            &context.branch.branch,
        )
        .await
        {
            Ok(source) => state.stage.get_runtime_workspace(source, token).await.ok(),
            Err(_) => runtime_workspace_for_base(
                &state,
                &context.user.principal_id,
                token,
                &context.workspace,
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
                &context.workspace,
                semantic.workspace.root(),
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
    workspace: &WorkspaceRecord,
    branch: &ActiveBranchRecord,
) -> ApiResult<Vec<String>> {
    if context_is_github_workspace(workspace) {
        let token = require_github_token(user, "Loading branch changes")?;
        let comparison = state
            .github
            .compare_refs(
                token,
                &workspace.owner,
                &workspace.name,
                &branch.base_ref,
                &branch.branch,
            )
            .await
            .map_err(|err| ApiError::github(&err, "Loading branch changes"))?;
        return Ok(filter_workspace_paths(workspace, comparison.files));
    }
    let selector = workspace_source_for_branch(
        state,
        &user.principal_id,
        source_token(user),
        workspace,
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
    Ok(filter_workspace_paths(
        workspace,
        changes
            .changed_files
            .into_iter()
            .map(|path| path.as_str().to_owned())
            .collect(),
    ))
}

fn filter_workspace_paths(workspace: &WorkspaceRecord, paths: Vec<String>) -> Vec<String> {
    let prefix = if workspace.path == "." {
        String::new()
    } else {
        format!("{}/", workspace.path)
    };
    paths
        .into_iter()
        .filter(|path| prefix.is_empty() || path.starts_with(&prefix))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn branch_file_text_and_sha(
    state: &ConsoleState,
    context: &BranchContext,
    file_path: &str,
    backend: &BranchBackend<'_>,
) -> ApiResult<(String, Option<String>)> {
    match backend {
        BranchBackend::GitHub { token, .. } => {
            let file = state
                .github
                .file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    file_path,
                    &context.branch.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Reading the branch file"))?;
            Ok((file.content, Some(file.sha)))
        }
        BranchBackend::LocalGit => Ok((
            local_git::read_file(&context.workspace, file_path)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?,
            None,
        )),
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
        &context.workspace,
        "Writing the branch file",
    )? {
        BranchBackend::GitHub { token, .. } => {
            state
                .github
                .update_file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    file_path,
                    &context.branch.branch,
                    sha.expect("GitHub file reads include a sha"),
                    content,
                    message,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Writing the branch file"))?;
        }
        BranchBackend::LocalGit => {
            local_git::write_file(&context.workspace, file_path, content)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
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
        None if context_is_github_workspace(&context.workspace) => {
            match context.user.github_token.as_deref() {
                Some(token) => state
                    .github
                    .branch_head_sha(
                        token,
                        &context.workspace.owner,
                        &context.workspace.name,
                        &context.branch.branch,
                    )
                    .await
                    .ok(),
                None => None,
            }
        }
        None => local_git::head_sha(&context.workspace.source).await.ok(),
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
    if context_is_github_workspace(&context.workspace) {
        let token = context.user.github_token.as_deref()?;
        return state
            .github
            .file(
                token,
                &context.workspace.owner,
                &context.workspace.name,
                path,
                &context.branch.base_ref,
            )
            .await
            .ok()
            .map(|file| file.content);
    }
    let relative = workspace_local_path(&context.workspace, path).ok()?;
    let output = tokio::process::Command::new("git")
        .args([
            "-C",
            &context.workspace.source,
            "show",
            &format!("{}:./{relative}", context.branch.base_ref),
        ])
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn editable_entities(
    workspace: &WorkspaceRecord,
    staged_root: &std::path::Path,
    inventory: &WorkspaceInventory,
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
            badge: Some(format!("{} predicates", item.predicate_count)),
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
            badge: Some(format!("{} entries", item.entry_count)),
            catalog_id: None,
            entry_key: None,
        });
    }
    for item in &inventory.catalog_entries {
        nodes.push(Node {
            section: "catalogs",
            id: item.id.clone(),
            kind: "catalog entry",
            path: item.path.clone(),
            description: Some(format!(
                "Entry {} for catalog {}",
                item.key, item.catalog_id
            )),
            badge: Some(item.catalog_id.clone()),
            catalog_id: Some(item.catalog_id.clone()),
            entry_key: Some(item.key.clone()),
        });
    }
    for item in &inventory.schemas {
        nodes.push(Node {
            section: "schemas",
            id: item.id.clone(),
            kind: "schema",
            path: item.path.clone(),
            description: item.title.clone(),
            badge: Some("json".to_owned()),
            catalog_id: None,
            entry_key: None,
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
    if let Some(schema_path) = &inventory.context.schema_path {
        nodes.push(Node {
            section: "context",
            id: "context.schema.json".to_owned(),
            kind: "context schema",
            path: schema_path.clone(),
            description: Some("Workspace context schema".to_owned()),
            badge: Some("schema".to_owned()),
            catalog_id: None,
            entry_key: None,
        });
    }
    for path in &inventory.context.examples {
        nodes.push(Node {
            section: "context",
            id: path.rsplit('/').next().unwrap_or(path).to_owned(),
            kind: "context example",
            path: path.clone(),
            description: Some("Example resolution context".to_owned()),
            badge: Some("example".to_owned()),
            catalog_id: None,
            entry_key: None,
        });
    }

    let mut entities = Vec::with_capacity(nodes.len());
    for node in nodes {
        let definition = read_workspace_definition(workspace, staged_root, &node.path).await?;
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
        "schemas" => Some(EntityKind::Schemas),
        "context" => Some(EntityKind::Context),
        "linters" => Some(EntityKind::Linters),
        _ => None,
    }
}

fn invalid_entity_request() -> ApiError {
    ApiError::bad_request(
        "kind and id are required; catalog entry creation also requires catalogId. ids may \
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

    fn workspace(path: &str) -> WorkspaceRecord {
        WorkspaceRecord {
            id: "workspace-id".to_owned(),
            slug: "configs".to_owned(),
            repo_id: "repo-id".to_owned(),
            owner: "octo".to_owned(),
            name: "configs".to_owned(),
            path: path.to_owned(),
            git_ref: "main".to_owned(),
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
    fn workspace_path_filter_keeps_only_selected_workspace() {
        assert_eq!(
            filter_workspace_paths(
                &workspace("apps/payments"),
                vec![
                    "apps/payments/variables/a.toml".to_owned(),
                    "apps/web/variables/b.toml".to_owned(),
                ],
            ),
            vec!["apps/payments/variables/a.toml".to_owned()]
        );
        assert_eq!(
            filter_workspace_paths(
                &workspace("."),
                vec!["variables/a.toml".to_owned(), "README.md".to_owned()],
            ),
            vec!["README.md".to_owned(), "variables/a.toml".to_owned()]
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
