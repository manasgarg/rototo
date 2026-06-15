use super::api::{ApiError, ApiResult, ConsoleState};
use super::stage::{CachedWorkspaceSource, SemanticWorkspace, WorkspaceSourceInput};
use super::store::WorkspaceRecord;
use crate::sdk::Workspace;
use std::sync::Arc;

pub(crate) async fn workspace_source_for_base(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    workspace: &WorkspaceRecord,
) -> ApiResult<CachedWorkspaceSource> {
    CachedWorkspaceSource::for_base_workspace(WorkspaceSourceInput {
        principal_id,
        token,
        owner: &workspace.owner,
        name: &workspace.name,
        path: &workspace.path,
        git_ref: &workspace.git_ref,
        source: source_tree_source(state.fixed_workspace_source.as_deref(), workspace),
    })
    .await
    .map_err(|err| ApiError::internal(err.to_string()))
}

pub(crate) async fn semantic_workspace_for_base(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    workspace: &WorkspaceRecord,
) -> ApiResult<SemanticWorkspace> {
    let workspace_source = workspace_source_for_base(state, principal_id, token, workspace).await?;
    state
        .stage
        .get_semantic_workspace(workspace_source, token)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))
}

pub(crate) async fn runtime_workspace_for_base(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    workspace: &WorkspaceRecord,
) -> ApiResult<Arc<Workspace>> {
    let workspace_source = workspace_source_for_base(state, principal_id, token, workspace).await?;
    state
        .stage
        .get_runtime_workspace(workspace_source, token)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))
}

fn source_tree_source<'a>(
    fixed_workspace_source: Option<&'a str>,
    workspace: &'a WorkspaceRecord,
) -> &'a str {
    fixed_workspace_source.unwrap_or(&workspace.source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> WorkspaceRecord {
        WorkspaceRecord {
            id: "workspace_1".to_owned(),
            repo_id: "repo_1".to_owned(),
            slug: "octo-configs-root".to_owned(),
            owner: "octo".to_owned(),
            name: "configs".to_owned(),
            path: ".".to_owned(),
            git_ref: "main".to_owned(),
            source: "https://api.github.com/repos/octo/configs/tarball/main".to_owned(),
            discovered_at: "2026-06-13T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn source_tree_source_prefers_fixed_source_tree_root() {
        let mut workspace = workspace();
        workspace.path = "apps/payments".to_owned();
        workspace.source = "/tmp/configs/apps/payments".to_owned();

        assert_eq!(
            source_tree_source(Some("/tmp/configs"), &workspace),
            "/tmp/configs"
        );
        assert_eq!(source_tree_source(None, &workspace), workspace.source);
    }
}
