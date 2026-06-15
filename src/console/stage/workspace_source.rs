#![allow(dead_code)]

use super::identity::strip_prefix_ignore_ascii_case;
use super::{
    CachedWorkspaceSource, TokenIdentity, TreeRevision, TreeSource, WorkspacePath, WorkspaceSource,
};
use crate::error::{Result, RototoError};

/// Compatibility input for current console routes and store rows.
///
/// This keeps the raw `WorkspaceRecord` shape out of the stage cache while the
/// old store still carries source strings, owner/name display fields, and
/// workspace paths separately.
#[derive(Clone, Copy, Debug)]
pub struct WorkspaceSourceInput<'a> {
    pub principal_id: &'a str,
    pub token: &'a str,
    pub owner: &'a str,
    pub name: &'a str,
    pub path: &'a str,
    pub git_ref: &'a str,
    pub source: &'a str,
}

impl CachedWorkspaceSource {
    pub async fn for_base_workspace(input: WorkspaceSourceInput<'_>) -> Result<Self> {
        let parsed = LegacyWorkspaceSource::parse(input.source, input.owner, input.name).await?;
        let revision = match parsed.kind {
            LegacyWorkspaceSourceKind::Git => {
                TreeRevision::git_ref_or_commit(selected_git_ref(input, &parsed))?
            }
            LegacyWorkspaceSourceKind::LocalFolder => TreeRevision::LocalWorkingTree,
            LegacyWorkspaceSourceKind::Archive => {
                return Err(RototoError::new(
                    "archive workspace sources require a resolved archive snapshot",
                ));
            }
        };
        cached_workspace_from_parts(input, parsed.tree, revision)
    }

    pub async fn for_branch_workspace(
        input: WorkspaceSourceInput<'_>,
        branch: impl AsRef<str>,
    ) -> Result<Self> {
        let parsed = LegacyWorkspaceSource::parse(input.source, input.owner, input.name).await?;
        if !matches!(parsed.kind, LegacyWorkspaceSourceKind::Git) {
            return Err(RototoError::new(
                "branch workspace sources require a git-backed source tree",
            ));
        }
        cached_workspace_from_parts(input, parsed.tree, TreeRevision::git_branch(branch)?)
    }
}

#[derive(Debug)]
struct LegacyWorkspaceSource {
    tree: TreeSource,
    ref_: Option<String>,
    kind: LegacyWorkspaceSourceKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyWorkspaceSourceKind {
    Git,
    LocalFolder,
    Archive,
}

impl LegacyWorkspaceSource {
    async fn parse(source: &str, owner: &str, name: &str) -> Result<Self> {
        let source = source.trim();
        let Some(uri) = ParsedSourceUri::parse(source)? else {
            return Ok(Self {
                tree: TreeSource::local_folder(source).await?,
                ref_: None,
                kind: LegacyWorkspaceSourceKind::LocalFolder,
            });
        };

        if uri.scheme == "file" {
            if uri.ref_.is_some() || uri.subdir.is_some() {
                return Err(RototoError::new(
                    "file:// workspace sources do not support fragments",
                ));
            }
            return Ok(Self {
                tree: TreeSource::local_folder(&uri.base).await?,
                ref_: None,
                kind: LegacyWorkspaceSourceKind::LocalFolder,
            });
        }

        if uri.scheme.starts_with("git+") {
            return Ok(Self {
                tree: TreeSource::git_remote(uri.source_without_fragment())?,
                ref_: uri.ref_,
                kind: LegacyWorkspaceSourceKind::Git,
            });
        }

        if uri.scheme == "https" {
            if github_archive_source(&uri.base).is_some() {
                return Ok(Self {
                    tree: TreeSource::github(owner, name)?,
                    ref_: uri.ref_,
                    kind: LegacyWorkspaceSourceKind::Git,
                });
            }
            return Ok(Self {
                tree: TreeSource::archive(source)?,
                ref_: None,
                kind: LegacyWorkspaceSourceKind::Archive,
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
    tree: TreeSource,
    revision: TreeRevision,
) -> Result<CachedWorkspaceSource> {
    CachedWorkspaceSource::new(
        input.principal_id,
        WorkspaceSource::new(tree, revision, WorkspacePath::new(input.path)?),
        TokenIdentity::from_console_token(input.token),
    )
}

fn selected_git_ref<'a>(
    input: WorkspaceSourceInput<'a>,
    parsed: &'a LegacyWorkspaceSource,
) -> &'a str {
    let git_ref = input.git_ref.trim();
    if git_ref.is_empty() {
        parsed.ref_.as_deref().unwrap_or("main")
    } else {
        git_ref
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::{BranchName, GitCommit, GitRefName};

    fn github_workspace_input() -> WorkspaceSourceInput<'static> {
        WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            owner: "Rototo",
            name: "Config",
            path: "workspaces/payments",
            git_ref: "main",
            source: "git+https://github.com/Rototo/Config.git#main:workspaces/payments",
        }
    }

    #[tokio::test]
    async fn base_workspace_source_adapts_current_github_store_shape() {
        let source = CachedWorkspaceSource::for_base_workspace(github_workspace_input())
            .await
            .unwrap();

        assert_eq!(
            source,
            CachedWorkspaceSource::new(
                "user_123",
                WorkspaceSource::new(
                    TreeSource::GitHub {
                        owner: "rototo".to_owned(),
                        name: "config".to_owned(),
                    },
                    TreeRevision::GitRef(GitRefName::new("main").unwrap()),
                    WorkspacePath::new("workspaces/payments").unwrap(),
                ),
                TokenIdentity::None,
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn base_workspace_source_uses_commit_revision_for_pinned_git_ref() {
        let input = WorkspaceSourceInput {
            git_ref: "8D3C4B5A6F7081920A1B2C3D4E5F60718293A4B5",
            ..github_workspace_input()
        };

        let source = CachedWorkspaceSource::for_base_workspace(input)
            .await
            .unwrap();

        assert_eq!(
            source.workspace.revision,
            TreeRevision::GitCommit(
                GitCommit::new("8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5").unwrap()
            )
        );
    }

    #[tokio::test]
    async fn branch_workspace_source_adapts_token_and_branch_identity() {
        let input = WorkspaceSourceInput {
            token: "ghp_secret",
            ..github_workspace_input()
        };

        let source = CachedWorkspaceSource::for_branch_workspace(
            input,
            "rototo-console/alice/change-checkout",
        )
        .await
        .unwrap();

        assert_eq!(
            source.token,
            TokenIdentity::Sha256Hex(
                "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434".to_owned()
            )
        );
        assert_eq!(
            source.workspace.revision,
            TreeRevision::GitBranch(
                BranchName::new("rototo-console/alice/change-checkout").unwrap()
            )
        );
    }

    #[tokio::test]
    async fn workspace_source_adapter_preserves_generic_git_remote_identity() {
        let input = WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            owner: "Team",
            name: "Config",
            path: "services/api",
            git_ref: "main",
            source: "git+https://Git.Example.com/Team/Config.git#main:services/api",
        };

        let source = CachedWorkspaceSource::for_base_workspace(input)
            .await
            .unwrap();

        assert_eq!(
            source.workspace.tree,
            TreeSource::GitRemote {
                remote_url: "git+https://git.example.com/Team/Config.git".to_owned()
            }
        );
        assert_eq!(
            source.workspace.path,
            WorkspacePath::new("services/api").unwrap()
        );
        assert_eq!(
            source.workspace.revision,
            TreeRevision::GitRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn workspace_source_adapter_maps_local_workspace_to_working_tree() {
        let tempdir = TempDir::new().expect("tempdir");
        let input = WorkspaceSourceInput {
            principal_id: "local-user",
            token: "",
            owner: "demo",
            name: "config",
            path: ".",
            git_ref: "main",
            source: tempdir.path().to_str().expect("utf8 temp path"),
        };

        let source = CachedWorkspaceSource::for_base_workspace(input)
            .await
            .unwrap();

        assert_eq!(
            source.workspace.tree,
            TreeSource::LocalFolder {
                root: tempdir.path().canonicalize().unwrap()
            }
        );
        assert_eq!(source.workspace.path, WorkspacePath::root());
        assert_eq!(source.workspace.revision, TreeRevision::LocalWorkingTree);
    }

    #[tokio::test]
    async fn workspace_source_adapter_maps_github_archive_store_shape_to_git_tree() {
        let input = WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            owner: "Rototo",
            name: "Config",
            path: "workspaces/payments",
            git_ref: "main",
            source: "https://API.GITHUB.com/repos/Rototo/Config/tarball/main#:workspaces/payments",
        };

        let source = CachedWorkspaceSource::for_base_workspace(input)
            .await
            .unwrap();

        assert_eq!(
            source.workspace.tree,
            TreeSource::GitHub {
                owner: "rototo".to_owned(),
                name: "config".to_owned(),
            }
        );
        assert_eq!(
            source.workspace.revision,
            TreeRevision::GitRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn workspace_source_adapter_rejects_unresolved_arbitrary_archive() {
        let input = WorkspaceSourceInput {
            principal_id: "user_123",
            token: "",
            owner: "demo",
            name: "config",
            path: "workspaces/payments",
            git_ref: "main",
            source: "https://example.com/releases/config.tar.gz#:workspaces/payments",
        };

        assert!(
            CachedWorkspaceSource::for_base_workspace(input)
                .await
                .is_err()
        );
        assert!(
            CachedWorkspaceSource::for_branch_workspace(input, "rototo-console/alice/change")
                .await
                .is_err()
        );
    }
}
