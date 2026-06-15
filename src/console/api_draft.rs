use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, patch, post};
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
use super::store::{
    DraftChangeInput, DraftChangeRecord, DraftEventInput, DraftEventRecord, DraftSessionRecord,
    DraftStatus, NewDraftSession, PullRequestStateInput, SessionUser, WorkspaceRecord,
};
use super::variable_toml::update_primitive_variable_default;
use super::workspace_edit::{
    EntityKind, belongs_to_workspace, draft_branch_name, draft_pr_body, draft_pr_title,
    draft_source, entity_template_files, expected_variable_file_path, parse_entity_id,
    parse_variable_type, variable_value_target_path,
};
use super::workspace_source::workspace_source_for_branch;

const PR_SYNC_FRESH: Duration = Duration::from_secs(60);
const MAX_PREVIEW_CONTEXTS: usize = 4;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/workspaces/{workspace_id}/drafts", post(draft_create))
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}",
            patch(draft_rename),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/data",
            get(draft_data),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/entity",
            get(draft_entity),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/sync-pr",
            post(draft_sync_pr),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/publish",
            post(draft_publish),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/abandon",
            post(draft_abandon),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/variables",
            post(draft_variable_save),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/files",
            post(draft_file_save).delete(draft_file_delete),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/entities",
            post(draft_entity_create),
        )
        .route(
            "/workspaces/{workspace_id}/drafts/{draft_id}/lsp",
            post(draft_lsp),
        )
}

/// Authorized draft, workspace, and user bundle for one draft route.
///
/// `load_draft` creates this after checking the user owns both the workspace
/// and draft. It lives for one request and prevents later helper calls from
/// accidentally mixing ids from different route parameters.
struct DraftContext {
    user: SessionUser,
    workspace: WorkspaceRecord,
    draft: DraftSessionRecord,
}

/// Write mechanism selected for a draft operation.
///
/// The value is derived from the console write policy, workspace source kind,
/// and current user credential. It lives only for the current operation and
/// keeps GitHub writes, local-git writes, and direct-push mode explicit at each
/// mutation site.
enum DraftBackend<'a> {
    GitHub { token: &'a str, direct: bool },
    LocalGit,
}

fn draft_backend<'a>(
    state: &ConsoleState,
    user: &'a SessionUser,
    workspace: &WorkspaceRecord,
    action: &str,
) -> ApiResult<DraftBackend<'a>> {
    let kind = classify_workspace_source(&workspace.source);
    match state.write_policy {
        WritePolicy::Disabled => Err(ApiError::bad_request(format!(
            "{action} is disabled for this console"
        ))),
        WritePolicy::PullRequest => match kind {
            WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit => {
                let token = require_github_token(user, action)?;
                Ok(DraftBackend::GitHub {
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
                Ok(DraftBackend::LocalGit)
            }
            WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit => {
                let token = require_github_token(user, action)?;
                Ok(DraftBackend::GitHub {
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

async fn load_draft(
    state: &ConsoleState,
    headers: &HeaderMap,
    workspace_id: &str,
    draft_id: &str,
    require_open: bool,
) -> ApiResult<DraftContext> {
    let user = require_user(state, headers).await?;
    let workspace = load_workspace(state, &user, workspace_id).await?;
    let draft = state
        .store
        .get_draft_session_for_user(draft_id, &workspace.id, &user.principal_id)
        .await?
        .ok_or_else(|| ApiError::not_found("draft not found"))?;
    if require_open && draft.status != DraftStatus::Open {
        return Err(ApiError::bad_request("draft is not open"));
    }
    Ok(DraftContext {
        user,
        workspace,
        draft,
    })
}

async fn invalidate_draft(
    state: &ConsoleState,
    workspace: &WorkspaceRecord,
    draft: &DraftSessionRecord,
) {
    // Staged checkouts of the draft branch go stale after a commit.
    state.lsp.drop_sessions_for_draft(&draft.id).await;
    state
        .stage
        .invalidate_source(&draft_source(workspace, draft))
        .await;
}

async fn inspect_draft_workspace(
    state: &ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
    draft: &DraftSessionRecord,
) -> ApiResult<Arc<crate::sdk::Workspace>> {
    let token = source_token(user);
    let workspace_source =
        workspace_source_for_branch(state, &user.principal_id, token, workspace, &draft.branch)
            .await?;
    state
        .stage
        .get_inspected_workspace(workspace_source, token)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))
}

/// Optional body for starting a draft.
///
/// Empty bodies are accepted for the normal "create a branch for me" flow.
/// When present, `branch` is validated against the selected backend before a
/// durable `draft_sessions` row is inserted.
#[derive(serde::Deserialize, Default)]
struct DraftCreateBody {
    branch: Option<String>,
}

async fn draft_create(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let workspace = load_workspace(&state, &user, &workspace_id).await?;
    let requested_branch = parse_draft_create_body(&body)?
        .branch
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty());

    let base_ref = workspace.git_ref.clone();
    let backend = draft_backend(&state, &user, &workspace, "Starting a draft")?;
    let target_branch = match backend {
        DraftBackend::LocalGit => {
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
            branch
        }
        DraftBackend::GitHub { direct: true, .. } => {
            if let Some(requested) = requested_branch.as_deref()
                && requested != base_ref
            {
                return Err(ApiError::bad_request(format!(
                    "direct-push drafts write to configured ref {base_ref}, not {requested}"
                )));
            }
            base_ref.clone()
        }
        DraftBackend::GitHub { direct: false, .. } => {
            if requested_branch.as_deref() == Some(base_ref.as_str()) {
                return Err(ApiError::bad_request(format!(
                    "Editing {base_ref} directly would skip review. Pick another branch, or start a new draft."
                )));
            }
            requested_branch.clone().unwrap_or_default()
        }
    };
    if !target_branch.is_empty() {
        let existing = state
            .store
            .find_open_draft_for_repo_branch(&workspace.id, &user.principal_id, &target_branch)
            .await?;
        if let Some(existing) = existing {
            let existing = state
                .store
                .ensure_draft_workspace(&existing.id, &workspace.id)
                .await?;
            return Ok(Json(json!({ "draft": existing })));
        }
    }

    let draft = match backend {
        DraftBackend::LocalGit => {
            state
                .store
                .create_draft_session(NewDraftSession {
                    workspace_id: workspace.id.clone(),
                    principal_id: user.principal_id.clone(),
                    branch: target_branch,
                    base_ref,
                })
                .await?
        }
        DraftBackend::GitHub {
            token,
            direct: true,
        } => {
            state
                .github
                .assert_repo_write_access(token, &workspace.owner, &workspace.name)
                .await
                .map_err(|err| ApiError::github(&err, "Starting a draft"))?;
            state
                .github
                .branch_head_sha(token, &workspace.owner, &workspace.name, &base_ref)
                .await
                .map_err(|err| ApiError::github(&err, "Starting a draft"))?;
            state
                .store
                .create_draft_session(NewDraftSession {
                    workspace_id: workspace.id.clone(),
                    principal_id: user.principal_id.clone(),
                    branch: target_branch,
                    base_ref,
                })
                .await?
        }
        DraftBackend::GitHub {
            token,
            direct: false,
        } => {
            state
                .github
                .assert_repo_write_access(token, &workspace.owner, &workspace.name)
                .await
                .map_err(|err| ApiError::github(&err, "Starting a draft"))?;
            if let Some(branch) = requested_branch {
                state
                    .github
                    .branch_head_sha(token, &workspace.owner, &workspace.name, &branch)
                    .await
                    .map_err(|err| ApiError::github(&err, "Starting a draft"))?;
                state
                    .store
                    .create_draft_session(NewDraftSession {
                        workspace_id: workspace.id.clone(),
                        principal_id: user.principal_id.clone(),
                        branch,
                        base_ref,
                    })
                    .await?
            } else {
                let base_sha = state
                    .github
                    .branch_head_sha(token, &workspace.owner, &workspace.name, &base_ref)
                    .await
                    .map_err(|err| ApiError::github(&err, "Starting a draft"))?;
                let branch = draft_branch_name(&user.identity.display_login(), &workspace);
                state
                    .github
                    .create_branch(token, &workspace.owner, &workspace.name, &branch, &base_sha)
                    .await
                    .map_err(|err| ApiError::github(&err, "Starting a draft"))?;
                state
                    .store
                    .create_draft_session(NewDraftSession {
                        workspace_id: workspace.id.clone(),
                        principal_id: user.principal_id.clone(),
                        branch,
                        base_ref,
                    })
                    .await?
            }
        }
    };
    Ok(Json(json!({ "draft": draft })))
}

fn parse_draft_create_body(body: &[u8]) -> ApiResult<DraftCreateBody> {
    if body.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(DraftCreateBody::default());
    }
    serde_json::from_slice(body)
        .map_err(|err| ApiError::bad_request(format!("invalid JSON body: {err}")))
}

/// Request body for retargeting an open pull-request draft to another branch.
///
/// The branch name is validated against GitHub before the existing draft row is
/// updated. The struct is discarded after that one PATCH request.
#[derive(serde::Deserialize)]
struct DraftRenameBody {
    branch: Option<String>,
}

async fn draft_rename(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
    Json(body): Json<DraftRenameBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
    let Some(branch) = body
        .branch
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty())
    else {
        return Err(ApiError::bad_request("branch is required"));
    };
    if branch == context.draft.branch {
        return Ok(Json(json!({ "draft": context.draft })));
    }
    let DraftBackend::GitHub {
        token,
        direct: false,
    } = draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Renaming the draft branch",
    )?
    else {
        return Err(ApiError::bad_request(
            "branch rename only applies to GitHub pull-request drafts",
        ));
    };
    let renamed = state
        .github
        .rename_branch(
            token,
            &context.workspace.owner,
            &context.workspace.name,
            &context.draft.branch,
            &branch,
        )
        .await
        .map_err(|err| ApiError::github(&err, "Renaming the draft branch"))?;
    let updated = state
        .store
        .update_draft_branch(&context.draft.id, &renamed, &context.draft.branch)
        .await?;
    Ok(Json(json!({ "draft": updated })))
}

async fn draft_sync_pr(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, false).await?;
    let DraftBackend::GitHub {
        token: _,
        direct: false,
    } = draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Syncing the pull request",
    )?
    else {
        return Err(ApiError::bad_request(
            "pull request sync only applies to GitHub pull-request drafts",
        ));
    };
    let Some(pr_number) = context
        .draft
        .pr_number
        .or_else(|| pull_request_number_from_url(context.draft.pr_url.as_deref()))
    else {
        return Err(ApiError::bad_request("draft does not have a pull request"));
    };
    let draft = sync_pull_request(
        &state,
        &context.user,
        &context.workspace,
        &context.draft,
        pr_number,
    )
    .await
    .map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "draft": draft })))
}

async fn sync_pull_request(
    state: &ConsoleState,
    user: &SessionUser,
    workspace: &WorkspaceRecord,
    draft: &DraftSessionRecord,
    pr_number: i64,
) -> std::result::Result<DraftSessionRecord, String> {
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
        .update_draft_pull_request_state(PullRequestStateInput {
            draft_id: draft.id.clone(),
            pr_number: pr.number,
            pr_state: pull_request_state(pr.state.as_deref(), pr.merged_at.as_deref()),
            pr_url: pr.html_url.clone(),
            pr_merged_at: pr.merged_at.clone(),
        })
        .await
        .map_err(|err| err.to_string())
}

fn tracked_draft_paths(
    changes: &[DraftChangeRecord],
    events: &[DraftEventRecord],
) -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = changes
        .iter()
        .map(|change| change.file_path.clone())
        .collect();
    for event in events {
        if let Some(detail) = event
            .detail_json
            .as_deref()
            .and_then(|detail| serde_json::from_str::<JsonValue>(detail).ok())
        {
            if let Some(file_path) = detail.get("filePath").and_then(JsonValue::as_str) {
                paths.insert(file_path.to_owned());
            }
            if let Some(files) = detail.get("files").and_then(JsonValue::as_array) {
                for file in files {
                    if let Some(file) = file.as_str() {
                        paths.insert(file.to_owned());
                    }
                }
            }
        }
    }
    paths
}

async fn draft_publish(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
    let changes = state.store.list_draft_changes(&context.draft.id).await?;
    let events = state.store.list_draft_events(&context.draft.id).await?;
    let tracked_paths = tracked_draft_paths(&changes, &events);
    if tracked_paths.is_empty() {
        return Err(ApiError::bad_request("draft has no tracked changes"));
    }

    let mut lint_workspaces = state
        .store
        .list_workspaces_for_draft(&context.draft.id)
        .await?;
    if lint_workspaces.is_empty() {
        lint_workspaces.push(context.workspace.clone());
    }
    let mut errors = 0;
    let mut warnings = 0;
    for workspace in &lint_workspaces {
        let inspected =
            inspect_draft_workspace(&state, &context.user, workspace, &context.draft).await?;
        let lint = inspected
            .lint()
            .await
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
        errors += lint
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::diagnostics::Severity::Error)
            .count();
        warnings += lint.diagnostics.len()
            - lint
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == crate::diagnostics::Severity::Error)
                .count();
    }
    if errors > 0 {
        return Err(ApiError::bad_request(format!(
            "draft has {errors} lint error(s) across included workspace(s); fix lint before publishing"
        )));
    }

    match draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Publishing the draft",
    )? {
        DraftBackend::GitHub {
            token,
            direct: false,
        } => {
            let pr = state
                .github
                .create_pull_request(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &draft_pr_title(&context.workspace),
                    &draft_pr_body(
                        &context.workspace,
                        &context.draft,
                        &changes,
                        errors,
                        warnings,
                    ),
                    &context.draft.branch,
                    &context.draft.base_ref,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Publishing the draft"))?;
            let pr_state = if pr.merged_at.is_some() {
                "merged".to_owned()
            } else {
                pr.state.clone().unwrap_or_else(|| "open".to_owned())
            };
            state
                .store
                .mark_draft_published(&context.draft.id, pr.number, &pr_state, &pr.html_url)
                .await?;
            Ok(Json(json!({
                "pullRequest": {
                    "html_url": pr.html_url,
                    "number": pr.number,
                    "state": pr.state,
                    "merged_at": pr.merged_at,
                }
            })))
        }
        DraftBackend::GitHub {
            token: _,
            direct: true,
        } => {
            let draft = state
                .store
                .mark_draft_direct_published(
                    &context.draft.id,
                    format!("Direct-pushed {}", context.draft.branch),
                    json!({
                        "backend": "githubApi",
                        "branch": context.draft.branch,
                    }),
                )
                .await?;
            Ok(Json(json!({
                "draft": draft,
                "directPush": { "backend": "githubApi" },
            })))
        }
        DraftBackend::LocalGit => {
            let paths: Vec<String> = tracked_paths.into_iter().collect();
            let result = local_git::commit_and_push(
                &context.workspace,
                &paths,
                &draft_pr_title(&context.workspace),
            )
            .await
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
            let draft = state
                .store
                .mark_draft_direct_published(
                    &context.draft.id,
                    if result.pushed {
                        format!("Committed and pushed {}", context.draft.branch)
                    } else {
                        format!("Committed {}", context.draft.branch)
                    },
                    json!({
                        "backend": "localGit",
                        "branch": context.draft.branch,
                        "commit": result.commit,
                        "pushed": result.pushed,
                        "upstream": result.upstream,
                        "pushError": result.push_error,
                    }),
                )
                .await?;
            Ok(Json(json!({ "draft": draft, "directPush": result })))
        }
    }
}

async fn draft_abandon(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
    let draft = state
        .store
        .mark_draft_abandoned(&context.draft.id)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    state.lsp.drop_sessions_for_draft(&context.draft.id).await;
    Ok(Json(json!({ "draft": draft })))
}

/// Request body for editing a primitive variable's default value.
///
/// This is the friendly-editor path, so it carries the semantic variable id and
/// the file path the browser believes owns that value. The route rechecks the
/// path, writes the file through the chosen backend, and records the net draft
/// change for later PR text.
#[derive(serde::Deserialize)]
struct VariableSaveBody {
    #[serde(rename = "variableId")]
    variable_id: Option<String>,
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    value: Option<String>,
}

async fn draft_variable_save(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
    Json(body): Json<VariableSaveBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
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

    let backend = draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Saving the draft change",
    )?;
    let (current_text, sha) = match backend {
        DraftBackend::GitHub { token, .. } => {
            let file = state
                .github
                .file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &file_path,
                    &context.draft.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Saving the draft change"))?;
            (file.content, Some(file.sha))
        }
        DraftBackend::LocalGit => (
            local_git::read_file(&context.workspace, &file_path)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?,
            None,
        ),
    };
    let update = update_primitive_variable_default(&current_text, &value)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    if update.before_literal != update.after_literal {
        match draft_backend(
            &state,
            &context.user,
            &context.workspace,
            "Saving the draft change",
        )? {
            DraftBackend::GitHub { token, .. } => {
                state
                    .github
                    .update_file(
                        token,
                        &context.workspace.owner,
                        &context.workspace.name,
                        &file_path,
                        &context.draft.branch,
                        sha.as_deref().expect("GitHub file reads include a sha"),
                        &update.text,
                        &format!("Update {variable_id} default value"),
                    )
                    .await
                    .map_err(|err| ApiError::github(&err, "Saving the draft change"))?;
            }
            DraftBackend::LocalGit => {
                local_git::write_file(&context.workspace, &file_path, &update.text)
                    .await
                    .map_err(|err| ApiError::bad_request(err.to_string()))?;
            }
        }
        invalidate_draft(&state, &context.workspace, &context.draft).await;
    }

    let change = state
        .store
        .record_draft_change(DraftChangeInput {
            draft_id: context.draft.id.clone(),
            file_path,
            target_path: Some(variable_value_target_path(&update.value_key)),
            before: update.before,
            after: update.after,
        })
        .await?;
    Ok(Json(json!({ "change": change })))
}

/// Raw file save request from the source editor.
///
/// The body contains draft-branch text only; the route reloads the current
/// branch file, commits the replacement through the chosen backend when it
/// changed, and stores a draft change row for the path.
#[derive(serde::Deserialize)]
struct FileSaveBody {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    content: Option<String>,
}

async fn draft_file_save(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
    Json(body): Json<FileSaveBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
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

    let backend = draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Saving the draft file",
    )?;
    let (current_text, sha) = match backend {
        DraftBackend::GitHub { token, .. } => {
            let file = state
                .github
                .file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &file_path,
                    &context.draft.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Saving the draft file"))?;
            (file.content, Some(file.sha))
        }
        DraftBackend::LocalGit => (
            local_git::read_file(&context.workspace, &file_path)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?,
            None,
        ),
    };
    if current_text != content {
        match draft_backend(
            &state,
            &context.user,
            &context.workspace,
            "Saving the draft file",
        )? {
            DraftBackend::GitHub { token, .. } => {
                state
                    .github
                    .update_file(
                        token,
                        &context.workspace.owner,
                        &context.workspace.name,
                        &file_path,
                        &context.draft.branch,
                        sha.as_deref().expect("GitHub file reads include a sha"),
                        &content,
                        &format!("Update {file_path}"),
                    )
                    .await
                    .map_err(|err| ApiError::github(&err, "Saving the draft file"))?;
            }
            DraftBackend::LocalGit => {
                local_git::write_file(&context.workspace, &file_path, &content)
                    .await
                    .map_err(|err| ApiError::bad_request(err.to_string()))?;
            }
        }
        state
            .store
            .record_draft_change(DraftChangeInput {
                draft_id: context.draft.id.clone(),
                file_path,
                target_path: None,
                before: json!(current_text),
                after: json!(content),
            })
            .await?;
        invalidate_draft(&state, &context.workspace, &context.draft).await;
    }
    Ok(Json(json!({ "ok": true })))
}

/// Raw file delete request from the source editor.
///
/// The route validates that the path stays inside the workspace, performs the
/// delete through the selected backend, records an append-only draft event, and
/// invalidates any staged view of the draft branch.
#[derive(serde::Deserialize)]
struct FileDeleteBody {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
}

async fn draft_file_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
    Json(body): Json<FileDeleteBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
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

    match draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Deleting the draft file",
    )? {
        DraftBackend::GitHub { token, .. } => {
            let file = state
                .github
                .file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &file_path,
                    &context.draft.branch,
                )
                .await
                .map_err(|err| ApiError::github(&err, "Deleting the draft file"))?;
            state
                .github
                .delete_file(
                    token,
                    &context.workspace.owner,
                    &context.workspace.name,
                    &file_path,
                    &context.draft.branch,
                    &file.sha,
                    &format!("Delete {file_path}"),
                )
                .await
                .map_err(|err| ApiError::github(&err, "Deleting the draft file"))?;
        }
        DraftBackend::LocalGit => {
            local_git::delete_file(&context.workspace, &file_path)
                .await
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
        }
    }
    state
        .store
        .record_draft_event(DraftEventInput {
            draft_id: context.draft.id.clone(),
            kind: "file.deleted".to_owned(),
            summary: format!("Deleted {file_path}"),
            detail: Some(json!({ "filePath": file_path })),
        })
        .await?;
    invalidate_draft(&state, &context.workspace, &context.draft).await;
    Ok(Json(json!({ "ok": true })))
}

/// Entity creation request from the structured draft editor.
///
/// It describes the workspace noun to create and enough context for templated
/// files, such as catalog membership or primitive variable type. The body is
/// validated before any file is written, then the generated `PlannedFile`s
/// become the draft's branch changes.
#[derive(serde::Deserialize)]
struct EntityCreateBody {
    kind: Option<String>,
    id: Option<String>,
    #[serde(rename = "catalogId")]
    catalog_id: Option<String>,
    #[serde(rename = "variableType")]
    variable_type: Option<String>,
}

async fn draft_entity_create(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
    Json(body): Json<EntityCreateBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
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
    let backend = draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Creating the draft entity",
    )?;
    let existing: std::collections::HashSet<String> = match backend {
        DraftBackend::GitHub { token, .. } => state
            .github
            .tree(
                token,
                &context.workspace.owner,
                &context.workspace.name,
                &context.draft.branch,
            )
            .await
            .map_err(|err| ApiError::github(&err, "Creating the draft entity"))?
            .into_iter()
            .filter(|entry| entry.entry_type == "blob")
            .map(|entry| entry.path)
            .collect(),
        DraftBackend::LocalGit => {
            let mut existing = std::collections::HashSet::new();
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

    match draft_backend(
        &state,
        &context.user,
        &context.workspace,
        "Creating the draft entity",
    )? {
        DraftBackend::GitHub { token, .. } => {
            for file in &files {
                state
                    .github
                    .create_file(
                        token,
                        &context.workspace.owner,
                        &context.workspace.name,
                        &file.path,
                        &context.draft.branch,
                        &file.content,
                        &format!("Create {}", file.path),
                    )
                    .await
                    .map_err(|err| ApiError::github(&err, "Creating the draft entity"))?;
            }
        }
        DraftBackend::LocalGit => {
            for file in &files {
                local_git::write_file(&context.workspace, &file.path, &file.content)
                    .await
                    .map_err(|err| ApiError::bad_request(err.to_string()))?;
            }
        }
    }
    state
        .store
        .record_draft_event(DraftEventInput {
            draft_id: context.draft.id.clone(),
            kind: "entity.created".to_owned(),
            summary: format!("Created {} {id}", kind.label()),
            detail: Some(json!({
                "kind": kind_wire_name(kind),
                "id": id,
                "files": files.iter().map(|file| file.path.clone()).collect::<Vec<_>>(),
            })),
        })
        .await?;
    invalidate_draft(&state, &context.workspace, &context.draft).await;
    Ok(Json(json!({ "files": files })))
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

fn kind_wire_name(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Variables => "variables",
        EntityKind::Qualifiers => "qualifiers",
        EntityKind::Catalogs => "catalogs",
        EntityKind::CatalogEntries => "catalog_entries",
        EntityKind::Schemas => "schemas",
        EntityKind::Context => "context",
        EntityKind::Linters => "linters",
    }
}

fn invalid_entity_request() -> ApiError {
    ApiError::bad_request(
        "kind and id are required; catalog entry creation also requires catalogId. ids may \
         contain letters, numbers, dot, dash, and underscore",
    )
}

/// Language-server request from the in-browser editor.
///
/// The request overlays unsaved text for one draft file and asks for an update,
/// completion, or hover operation. It is not stored; the `LspSessions` cache
/// owns any longer-lived server process state.
#[derive(serde::Deserialize)]
struct LspBody {
    op: Option<String>,
    path: Option<String>,
    text: Option<String>,
    position: Option<JsonValue>,
}

async fn draft_lsp(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
    Json(body): Json<LspBody>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, true).await?;
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
        inspect_draft_workspace(&state, &context.user, &context.workspace, &context.draft).await?;

    let op = body.op.as_deref().unwrap_or("unknown").to_owned();
    let lsp_started = std::time::Instant::now();
    let result: ApiResult<JsonValue> = match (body.op.as_deref(), body.position) {
        (Some("update"), _) => {
            let diagnostics = state
                .lsp
                .update(
                    &context.user.principal_id,
                    &context.draft.id,
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
                    &context.draft.id,
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
                    &context.draft.id,
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
                    "draft_id": draft_id,
                    "path": path,
                }),
            )
            .await;
    }
    let result = result?;
    Ok(Json(result))
}

async fn draft_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, false).await?;
    let DraftContext {
        user,
        workspace,
        mut draft,
    } = context;

    // Refresh the pull-request state at most once a minute.
    let mut pr_sync_error: Option<String> = None;
    let pr_number = draft
        .pr_number
        .or_else(|| pull_request_number_from_url(draft.pr_url.as_deref()));
    let synced_recently = draft
        .pr_synced_at
        .as_deref()
        .is_some_and(|synced_at| synced_at > super::time::now_iso_minus(PR_SYNC_FRESH).as_str());
    if let Some(pr_number) = pr_number
        && !synced_recently
    {
        match sync_pull_request(&state, &user, &workspace, &draft, pr_number).await {
            Ok(updated) => draft = updated,
            Err(error) => pr_sync_error = Some(error),
        }
    }

    let changes = state.store.list_draft_changes(&draft.id).await?;
    let events = state.store.list_draft_events(&draft.id).await?;

    let source = draft_source(&workspace, &draft);
    let staged = state
        .stage
        .semantic_model(source_token(&user), &source)
        .await;
    let (entities, edit_load_error, lint, model, staged_root) = match &staged {
        Ok((inspected, model)) => {
            let lint = match inspected.lint().await {
                Ok(lint) => lint_json(&lint),
                Err(err) => lint_error_json(&draft.branch, &err.to_string()),
            };
            match inspect_workspace_inventory(&workspace, model, inspected.root()).await {
                Ok(inventory) => {
                    match editable_entities(&workspace, inspected.root(), &inventory).await {
                        Ok(entities) => (
                            entities,
                            JsonValue::Null,
                            lint,
                            serde_json::to_value(model.as_ref()).expect("model serializes"),
                            Some(inspected.root().to_path_buf()),
                        ),
                        Err(err) => (
                            Vec::new(),
                            json!(err.to_string()),
                            lint,
                            serde_json::to_value(model.as_ref()).expect("model serializes"),
                            Some(inspected.root().to_path_buf()),
                        ),
                    }
                }
                Err(err) => (
                    Vec::new(),
                    json!(err.to_string()),
                    lint,
                    serde_json::to_value(model.as_ref()).expect("model serializes"),
                    Some(inspected.root().to_path_buf()),
                ),
            }
        }
        Err(err) => {
            let message = err.to_string();
            (
                Vec::new(),
                json!(message.clone()),
                lint_error_json(&draft.branch, &message),
                JsonValue::Null,
                None,
            )
        }
    };
    let _ = staged_root;

    // Paths touched on this branch: session changes and events, plus the ref
    // comparison when the source is remote — the branch may carry commits
    // made outside this session.
    let mut edited_paths = tracked_draft_paths(&changes, &events);
    if context_is_github_workspace(&workspace)
        && let Some(token) = user.github_token.as_deref()
        && let Ok(comparison) = state
            .github
            .compare_refs(
                token,
                &workspace.owner,
                &workspace.name,
                &draft.base_ref,
                &draft.branch,
            )
            .await
    {
        edited_paths.extend(comparison.files);
    }

    let capabilities = workspace_capabilities_json(&state, &user, &workspace);
    Ok(Json(json!({
        "workspace": workspace,
        "draft": draft,
        "prSyncError": pr_sync_error,
        "changes": changes,
        "events": events,
        "lint": lint,
        "model": model,
        "entities": entities,
        "editLoadError": edit_load_error,
        "editedPaths": edited_paths,
        "sourceKind": capabilities["sourceKind"].clone(),
        "capabilities": capabilities["capabilities"].clone(),
    })))
}

async fn draft_entity(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((workspace_id, draft_id)): Path<(String, String)>,
    Query(query): Query<EntityQuery>,
) -> ApiResult<Json<JsonValue>> {
    let context = load_draft(&state, &headers, &workspace_id, &draft_id, false).await?;
    let base_text = base_entity_text(&state, &context, &query.path).await;

    // Editing a variable: pre-evaluate every qualifier against each saved
    // request context so the form can preview resolution pathways live. The
    // runtime prefers the draft branch (so qualifier edits in the draft
    // count) and falls back to the base workspace when the draft is not
    // lint-clean.
    let mut context_previews = JsonValue::Array(Vec::new());
    let source = draft_source(&context.workspace, &context.draft);
    if let Ok((inspected, model)) = state
        .stage
        .semantic_model(source_token(&context.user), &source)
        .await
        && let Ok(inventory) =
            inspect_workspace_inventory(&context.workspace, &model, inspected.root()).await
        && inventory
            .variables
            .iter()
            .any(|variable| variable.path == query.path)
    {
        let runtime = match state
            .stage
            .runtime(source_token(&context.user), &source)
            .await
        {
            Ok(runtime) => Some(runtime),
            Err(_) => state
                .stage
                .runtime(source_token(&context.user), &context.workspace.source)
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
                inspected.root(),
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

/// The entity's text at the draft's base ref: GitHub contents for remote
/// sources, `git show` for local dev workspaces. None when unavailable (new
/// files, missing refs).
async fn base_entity_text(
    state: &ConsoleState,
    context: &DraftContext,
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
                &context.draft.base_ref,
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
            &format!("{}:./{relative}", context.draft.base_ref),
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
    /// Intermediate editable-entity row derived from inventory.
    ///
    /// The helper fills these from server inventory items, reads each file's
    /// current draft-branch text, serializes them, and then drops the vector.
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

    #[test]
    fn draft_create_body_accepts_empty_request() {
        let body = parse_draft_create_body(b"")
            .unwrap_or_else(|err| panic!("empty request body should be allowed: {}", err.message));
        assert_eq!(body.branch, None);

        let body = parse_draft_create_body(b"  \n").unwrap_or_else(|err| {
            panic!("whitespace request body should be allowed: {}", err.message)
        });
        assert_eq!(body.branch, None);
    }

    #[test]
    fn draft_create_body_parses_requested_branch() {
        let body = parse_draft_create_body(br#"{"branch":"rototo/draft"}"#)
            .unwrap_or_else(|err| panic!("valid request body should parse: {}", err.message));
        assert_eq!(body.branch.as_deref(), Some("rototo/draft"));
    }

    #[test]
    fn draft_create_body_rejects_invalid_json() {
        let Err(err) = parse_draft_create_body(b"not-json") else {
            panic!("invalid body should fail");
        };
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(err.message.contains("invalid JSON body"));
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
