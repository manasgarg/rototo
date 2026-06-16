use super::api::{ApiError, ApiResult, ConsoleState};
use super::capabilities::{WorkspaceSourceKind, classify_workspace_source};
use super::stage::{CachedWorkspaceLocator, SemanticWorkspace, WorkspaceLocatorInput};
use super::store::WorkspaceRecord;
use crate::sdk::Workspace;
use std::sync::Arc;

pub(crate) async fn workspace_source_for_base(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    workspace: &WorkspaceRecord,
) -> ApiResult<CachedWorkspaceLocator> {
    workspace_source_for_base_source(
        state.fixed_workspace_source.as_deref(),
        principal_id,
        token,
        workspace,
    )
    .await
}

pub(crate) async fn workspace_source_for_branch(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    workspace: &WorkspaceRecord,
    branch: &str,
) -> ApiResult<CachedWorkspaceLocator> {
    let source = source_tree_source(state.fixed_workspace_source.as_deref(), workspace);
    workspace_source_for_branch_source(source, principal_id, token, workspace, branch).await
}

async fn workspace_source_for_base_source(
    fixed_workspace_source: Option<&str>,
    principal_id: &str,
    token: &str,
    workspace: &WorkspaceRecord,
) -> ApiResult<CachedWorkspaceLocator> {
    CachedWorkspaceLocator::for_base_workspace(workspace_source_input(
        principal_id,
        token,
        source_tree_source(fixed_workspace_source, workspace),
        workspace,
    ))
    .await
    .map_err(|err| ApiError::internal(err.to_string()))
}

async fn workspace_source_for_branch_source(
    source: &str,
    principal_id: &str,
    token: &str,
    workspace: &WorkspaceRecord,
    branch: &str,
) -> ApiResult<CachedWorkspaceLocator> {
    let input = workspace_source_input(principal_id, token, source, workspace);
    if branch_source_uses_working_tree(source) {
        CachedWorkspaceLocator::for_base_workspace(input).await
    } else {
        CachedWorkspaceLocator::for_branch_workspace(input, branch).await
    }
    .map_err(|err| ApiError::internal(err.to_string()))
}

fn workspace_source_input<'a>(
    principal_id: &'a str,
    token: &'a str,
    source: &'a str,
    workspace: &'a WorkspaceRecord,
) -> WorkspaceLocatorInput<'a> {
    WorkspaceLocatorInput {
        principal_id,
        token,
        owner: &workspace.owner,
        name: &workspace.name,
        path: &workspace.path,
        git_ref: &workspace.git_ref,
        source,
    }
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

fn branch_source_uses_working_tree(source: &str) -> bool {
    matches!(
        classify_workspace_source(source),
        WorkspaceSourceKind::LocalPath | WorkspaceSourceKind::FileUrl
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::stage::{BranchName, SourceTreeOrigin, SourceTreeRevision};
    use tempfile::TempDir;

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

    #[tokio::test]
    async fn branch_workspace_source_selects_branch_for_git_workspace() {
        let mut workspace = workspace();
        workspace.path = "apps/payments".to_owned();
        workspace.source = "git+https://github.com/octo/configs.git#main:apps/payments".to_owned();

        let source = expect_ok(
            workspace_source_for_branch_source(
                &workspace.source,
                "user_123",
                "",
                &workspace,
                "feature/payments",
            )
            .await,
        );

        assert_eq!(
            source.workspace.source_tree.origin,
            SourceTreeOrigin::GitHub {
                owner: "octo".to_owned(),
                name: "configs".to_owned(),
            }
        );
        assert_eq!(
            source.workspace.source_tree.revision,
            SourceTreeRevision::GitBranch(BranchName::new("feature/payments").unwrap())
        );
    }

    #[tokio::test]
    async fn branch_workspace_source_keeps_local_workspace_on_working_tree() {
        let tempdir = TempDir::new().expect("tempdir");
        let mut workspace = workspace();
        workspace.path = ".".to_owned();
        workspace.source = tempdir.path().to_string_lossy().into_owned();

        let source = expect_ok(
            workspace_source_for_branch_source(
                &workspace.source,
                "local-user",
                "",
                &workspace,
                "feature/payments",
            )
            .await,
        );

        assert_eq!(
            source.workspace.source_tree.origin,
            SourceTreeOrigin::LocalFolder {
                root: tempdir.path().canonicalize().unwrap()
            }
        );
        assert_eq!(
            source.workspace.source_tree.revision,
            SourceTreeRevision::LocalWorkingTree
        );
    }

    fn expect_ok<T>(result: ApiResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{}", err.message),
        }
    }
}
