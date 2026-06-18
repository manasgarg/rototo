use super::api::{ApiError, ApiResult, ConsoleState};
use super::github::GitHubRepoIdentity;
use super::stage::{
    CachedWorkspaceLocator, SemanticWorkspace, SourceTreeOrigin, SourceTreeRevision, TokenIdentity,
    WorkspaceLocator, WorkspacePath,
};
use super::store::WorkspaceRecord;
use crate::error::{Result, RototoError};
use crate::sdk::Workspace;
use std::sync::Arc;

/// Store/API input needed to build a stage workspace locator.
///
/// This keeps raw `WorkspaceRecord` fields at the console adapter boundary so
/// stage only receives normalized locators.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkspaceSourceInput<'a> {
    pub(crate) principal_id: &'a str,
    pub(crate) token: &'a str,
    pub(crate) path: &'a str,
    pub(crate) revision: &'a str,
    pub(crate) source: &'a str,
}

pub(crate) async fn cached_workspace_locator_for_base(
    input: WorkspaceSourceInput<'_>,
) -> Result<CachedWorkspaceLocator> {
    let parsed = ParsedWorkspaceSource::parse(input.source).await?;
    let revision = match parsed.kind {
        WorkspaceSourceBacking::Git => {
            SourceTreeRevision::git_ref_or_commit(selected_git_ref(input, &parsed))?
        }
        WorkspaceSourceBacking::LocalFolder => SourceTreeRevision::LocalWorkingTree,
        WorkspaceSourceBacking::Archive => SourceTreeRevision::ArchiveSnapshot,
    };
    cached_workspace_from_parts(input, parsed.tree, revision)
}

pub(crate) async fn cached_workspace_locator_for_branch(
    input: WorkspaceSourceInput<'_>,
    branch: impl AsRef<str>,
) -> Result<CachedWorkspaceLocator> {
    let parsed = ParsedWorkspaceSource::parse(input.source).await?;
    if !matches!(parsed.kind, WorkspaceSourceBacking::Git) {
        return Err(RototoError::new(
            "branch workspace sources require a git branch source tree",
        ));
    }
    cached_workspace_from_parts(input, parsed.tree, SourceTreeRevision::git_branch(branch)?)
}

pub(crate) fn github_repo_for_workspace(workspace: &WorkspaceRecord) -> Result<GitHubRepoIdentity> {
    super::github::repo_identity_from_source(&workspace.source)
}

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
    cached_workspace_locator_for_base(workspace_source_input(
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
    cached_workspace_locator_for_branch(input, branch)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))
}

fn workspace_source_input<'a>(
    principal_id: &'a str,
    token: &'a str,
    source: &'a str,
    workspace: &'a WorkspaceRecord,
) -> WorkspaceSourceInput<'a> {
    WorkspaceSourceInput {
        principal_id,
        token,
        path: &workspace.path,
        revision: &workspace.revision,
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

#[derive(Debug)]
struct ParsedWorkspaceSource {
    tree: SourceTreeOrigin,
    ref_: Option<String>,
    kind: WorkspaceSourceBacking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceSourceBacking {
    Git,
    LocalFolder,
    Archive,
}

impl ParsedWorkspaceSource {
    async fn parse(source: &str) -> Result<Self> {
        let source = source.trim();
        let Some(uri) = ParsedSourceUri::parse(source)? else {
            return Ok(Self {
                tree: SourceTreeOrigin::local_folder(source).await?,
                ref_: None,
                kind: WorkspaceSourceBacking::LocalFolder,
            });
        };

        if uri.scheme == "file" {
            if uri.ref_.is_some() || uri.subdir.is_some() {
                return Err(RototoError::new(
                    "file:// workspace sources do not support fragments",
                ));
            }
            return Ok(Self {
                tree: SourceTreeOrigin::local_folder(&uri.base).await?,
                ref_: None,
                kind: WorkspaceSourceBacking::LocalFolder,
            });
        }

        if uri.scheme.starts_with("git+") {
            return Ok(Self {
                tree: SourceTreeOrigin::git_remote(uri.source_without_fragment())?,
                ref_: uri.ref_,
                kind: WorkspaceSourceBacking::Git,
            });
        }

        if uri.scheme == "https" {
            if let Some((owner, name, _)) = github_archive_source(&uri.base) {
                return Ok(Self {
                    tree: SourceTreeOrigin::github(owner, name)?,
                    ref_: uri.ref_,
                    kind: WorkspaceSourceBacking::Git,
                });
            }
            return Ok(Self {
                tree: SourceTreeOrigin::archive(source)?,
                ref_: None,
                kind: WorkspaceSourceBacking::Archive,
            });
        }

        Err(RototoError::new(format!(
            "workspace source scheme is not supported: {}",
            uri.scheme
        )))
    }
}

#[derive(Clone, Debug)]
struct ParsedSourceUri {
    scheme: String,
    base: String,
    ref_: Option<String>,
    subdir: Option<String>,
}

impl ParsedSourceUri {
    fn parse(source: &str) -> Result<Option<Self>> {
        let Some((scheme, rest)) = source.split_once("://") else {
            return Ok(None);
        };
        if scheme.is_empty() || rest.is_empty() {
            return Err(RototoError::new(format!(
                "workspace source URI is invalid: {source}"
            )));
        }
        let (base, fragment) = match rest.split_once('#') {
            Some((base, fragment)) => (base, Some(fragment)),
            None => (rest, None),
        };
        if base.is_empty() {
            return Err(RototoError::new(format!(
                "workspace source URI is invalid: {source}"
            )));
        }
        let (ref_, subdir) = match fragment {
            Some(fragment) => match fragment.split_once(':') {
                Some((ref_, subdir)) => (
                    (!ref_.is_empty()).then(|| ref_.to_owned()),
                    (!subdir.is_empty()).then(|| subdir.to_owned()),
                ),
                None => ((!fragment.is_empty()).then(|| fragment.to_owned()), None),
            },
            None => (None, None),
        };
        Ok(Some(Self {
            scheme: scheme.to_ascii_lowercase(),
            base: base.to_owned(),
            ref_,
            subdir,
        }))
    }

    fn source_without_fragment(&self) -> String {
        format!("{}://{}", self.scheme, self.base)
    }
}

fn cached_workspace_from_parts(
    input: WorkspaceSourceInput<'_>,
    tree: SourceTreeOrigin,
    revision: SourceTreeRevision,
) -> Result<CachedWorkspaceLocator> {
    CachedWorkspaceLocator::new(
        input.principal_id,
        WorkspaceLocator::new(tree, revision, WorkspacePath::new(input.path)?),
        TokenIdentity::from_console_token(input.token),
    )
}

fn selected_git_ref<'a>(
    input: WorkspaceSourceInput<'a>,
    parsed: &'a ParsedWorkspaceSource,
) -> &'a str {
    let revision = input.revision.trim();
    if revision.is_empty() {
        parsed.ref_.as_deref().unwrap_or("main")
    } else {
        revision
    }
}

fn github_archive_source(base: &str) -> Option<(&str, &str, &str)> {
    let rest = strip_prefix_ignore_ascii_case(base, "api.github.com/repos/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    let archive_kind = parts.next()?;
    if !matches!(archive_kind, "tarball" | "zipball") {
        return None;
    }
    let git_ref = parts.next()?;
    Some((owner, name, git_ref))
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        value.get(prefix.len()..)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::stage::{BranchName, GitRefName, SourceTreeOrigin, SourceTreeRevision};
    use crate::console::stage::{TokenIdentity, WorkspaceLocator, WorkspacePath};
    use tempfile::TempDir;

    fn github_workspace_input() -> WorkspaceSourceInput<'static> {
        WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            path: "workspaces/payments",
            revision: "main",
            source: "git+https://github.com/Rototo/Config.git#main:workspaces/payments",
        }
    }

    fn workspace() -> WorkspaceRecord {
        WorkspaceRecord {
            id: "workspace_1".to_owned(),
            source_tree_id: "repo_1".to_owned(),
            slug: "octo-configs-root".to_owned(),
            source_tree_label: "octo/configs".to_owned(),
            path: ".".to_owned(),
            revision: "main".to_owned(),
            source: "https://api.github.com/repos/octo/configs/tarball/main".to_owned(),
            discovered_at: "2026-06-13T00:00:00Z".to_owned(),
        }
    }

    #[tokio::test]
    async fn base_workspace_locator_adapts_current_github_store_shape() {
        let source = cached_workspace_locator_for_base(github_workspace_input())
            .await
            .unwrap();

        assert_eq!(
            source,
            CachedWorkspaceLocator::new(
                "user_123",
                WorkspaceLocator::new(
                    SourceTreeOrigin::GitHub {
                        owner: "rototo".to_owned(),
                        name: "config".to_owned(),
                    },
                    SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
                    WorkspacePath::new("workspaces/payments").unwrap(),
                ),
                TokenIdentity::None,
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn base_workspace_locator_uses_commit_revision_for_pinned_git_ref() {
        let input = WorkspaceSourceInput {
            revision: "8D3C4B5A6F7081920A1B2C3D4E5F60718293A4B5",
            ..github_workspace_input()
        };

        let source = cached_workspace_locator_for_base(input).await.unwrap();

        match source.workspace.source_tree.revision {
            SourceTreeRevision::GitCommit(commit) => {
                assert_eq!(commit.as_ref(), "8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5");
            }
            revision => panic!("expected git commit revision, got {revision:?}"),
        }
    }

    #[tokio::test]
    async fn branch_workspace_locator_adapts_token_and_branch_identity() {
        let input = WorkspaceSourceInput {
            token: "ghp_secret",
            ..github_workspace_input()
        };

        let source =
            cached_workspace_locator_for_branch(input, "rototo-console/alice/change-checkout")
                .await
                .unwrap();

        assert_eq!(
            source.token,
            TokenIdentity::Sha256Hex(
                "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434".to_owned()
            )
        );
        assert_eq!(
            source.workspace.source_tree.revision,
            SourceTreeRevision::GitBranch(
                BranchName::new("rototo-console/alice/change-checkout").unwrap()
            )
        );
    }

    #[tokio::test]
    async fn workspace_locator_adapter_preserves_generic_git_remote_identity() {
        let input = WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            path: "services/api",
            revision: "main",
            source: "git+https://Git.Example.com/Team/Config.git#main:services/api",
        };

        let source = cached_workspace_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.workspace.source_tree.origin,
            SourceTreeOrigin::GitRemote {
                remote_url: "git+https://git.example.com/Team/Config.git".to_owned()
            }
        );
        assert_eq!(
            source.workspace.path,
            WorkspacePath::new("services/api").unwrap()
        );
        assert_eq!(
            source.workspace.source_tree.revision,
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn workspace_locator_adapter_maps_local_workspace_to_working_tree() {
        let tempdir = TempDir::new().expect("tempdir");
        let input = WorkspaceSourceInput {
            principal_id: "local-user",
            token: "",
            path: ".",
            revision: "main",
            source: tempdir.path().to_str().expect("utf8 temp path"),
        };

        let source = cached_workspace_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.workspace.source_tree.origin,
            SourceTreeOrigin::LocalFolder {
                root: tempdir.path().canonicalize().unwrap()
            }
        );
        assert_eq!(source.workspace.path, WorkspacePath::root());
        assert_eq!(
            source.workspace.source_tree.revision,
            SourceTreeRevision::LocalWorkingTree
        );
    }

    #[tokio::test]
    async fn workspace_locator_adapter_maps_github_archive_store_shape_to_git_tree() {
        let input = WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            path: "workspaces/payments",
            revision: "main",
            source: "https://API.GITHUB.com/repos/Rototo/Config/tarball/main#:workspaces/payments",
        };

        let source = cached_workspace_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.workspace.source_tree.origin,
            SourceTreeOrigin::GitHub {
                owner: "rototo".to_owned(),
                name: "config".to_owned(),
            }
        );
        assert_eq!(
            source.workspace.source_tree.revision,
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn workspace_locator_adapter_maps_arbitrary_archive_to_snapshot() {
        let input = WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            path: "workspaces/payments",
            revision: "main",
            source: "https://example.com/releases/config.tar.gz#:workspaces/payments",
        };

        let source = cached_workspace_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.workspace.source_tree.origin,
            SourceTreeOrigin::Archive {
                url: "https://example.com/releases/config.tar.gz".to_owned()
            }
        );
        assert_eq!(
            source.workspace.source_tree.revision,
            SourceTreeRevision::ArchiveSnapshot
        );
        assert!(
            cached_workspace_locator_for_branch(input, "rototo-console/alice/change")
                .await
                .is_err()
        );
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

    fn expect_ok<T>(result: ApiResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{}", err.message),
        }
    }
}
