#![allow(dead_code)]

use super::identity::strip_prefix_ignore_ascii_case;
use super::{SourceTree, SourceTreeCacheKey, SourceTreeSelection, TokenIdentity, WorkspacePath};
use crate::error::{Result, RototoError};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WorkspaceSelector {
    pub source_tree: SourceTreeCacheKey,
    pub path: WorkspacePath,
    pub selection: SourceTreeSelection,
}

/// Compatibility input for current console routes and store rows.
///
/// This keeps the raw `WorkspaceRecord` shape out of the stage cache while the
/// old store still carries source strings, owner/name display fields, and
/// workspace paths separately.
#[derive(Clone, Copy, Debug)]
pub struct WorkspaceSelectorInput<'a> {
    pub principal_id: &'a str,
    pub token: &'a str,
    pub owner: &'a str,
    pub name: &'a str,
    pub path: &'a str,
    pub git_ref: &'a str,
    pub source: &'a str,
}

impl WorkspaceSelector {
    pub async fn for_base_workspace(input: WorkspaceSelectorInput<'_>) -> Result<Self> {
        let parsed = LegacyWorkspaceSource::parse(input.source, input.owner, input.name).await?;
        let selection = match parsed.kind {
            LegacyWorkspaceSourceKind::Git => {
                SourceTreeSelection::git_ref_or_commit(selected_git_ref(input, &parsed))?
            }
            LegacyWorkspaceSourceKind::LocalFolder => SourceTreeSelection::CurrentTree,
            LegacyWorkspaceSourceKind::Archive => {
                return Err(RototoError::new(
                    "archive workspace selectors require a resolved archive fingerprint",
                ));
            }
        };
        selector_from_parts(input, parsed.source_tree, selection)
    }

    pub async fn for_branch_workspace(
        input: WorkspaceSelectorInput<'_>,
        branch: impl AsRef<str>,
    ) -> Result<Self> {
        let parsed = LegacyWorkspaceSource::parse(input.source, input.owner, input.name).await?;
        if !matches!(parsed.kind, LegacyWorkspaceSourceKind::Git) {
            return Err(RototoError::new(
                "branch workspace selectors require a git-backed source tree",
            ));
        }
        selector_from_parts(
            input,
            parsed.source_tree,
            SourceTreeSelection::branch(branch)?,
        )
    }
}

#[derive(Debug)]
struct LegacyWorkspaceSource {
    source_tree: SourceTree,
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
                source_tree: SourceTree::local_folder(source).await?,
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
                source_tree: SourceTree::local_folder(&uri.base).await?,
                ref_: None,
                kind: LegacyWorkspaceSourceKind::LocalFolder,
            });
        }

        if uri.scheme.starts_with("git+") {
            return Ok(Self {
                source_tree: SourceTree::git_remote(uri.source_without_fragment())?,
                ref_: uri.ref_,
                kind: LegacyWorkspaceSourceKind::Git,
            });
        }

        if uri.scheme == "https" {
            if github_archive_source(&uri.base).is_some() {
                return Ok(Self {
                    source_tree: SourceTree::github(owner, name)?,
                    ref_: uri.ref_,
                    kind: LegacyWorkspaceSourceKind::Git,
                });
            }
            return Ok(Self {
                source_tree: SourceTree::archive(source)?,
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

fn selector_from_parts(
    input: WorkspaceSelectorInput<'_>,
    source_tree: SourceTree,
    selection: SourceTreeSelection,
) -> Result<WorkspaceSelector> {
    Ok(WorkspaceSelector {
        source_tree: SourceTreeCacheKey::new(
            input.principal_id,
            source_tree,
            TokenIdentity::from_console_token(input.token),
        )?,
        path: WorkspacePath::new(input.path)?,
        selection,
    })
}

fn selected_git_ref<'a>(
    input: WorkspaceSelectorInput<'a>,
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

    fn github_workspace_input() -> WorkspaceSelectorInput<'static> {
        WorkspaceSelectorInput {
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
    async fn base_workspace_selector_adapts_current_github_store_shape() {
        let selector = WorkspaceSelector::for_base_workspace(github_workspace_input())
            .await
            .unwrap();

        assert_eq!(
            selector,
            WorkspaceSelector {
                source_tree: SourceTreeCacheKey::new(
                    "user_123",
                    SourceTree::GitHub {
                        owner: "rototo".to_owned(),
                        name: "config".to_owned(),
                    },
                    TokenIdentity::None,
                )
                .unwrap(),
                path: WorkspacePath::new("workspaces/payments").unwrap(),
                selection: SourceTreeSelection::BaseRef(GitRefName::new("main").unwrap()),
            }
        );
    }

    #[tokio::test]
    async fn base_workspace_selector_uses_commit_selection_for_pinned_git_ref() {
        let input = WorkspaceSelectorInput {
            git_ref: "8D3C4B5A6F7081920A1B2C3D4E5F60718293A4B5",
            ..github_workspace_input()
        };

        let selector = WorkspaceSelector::for_base_workspace(input).await.unwrap();

        assert_eq!(
            selector.selection,
            SourceTreeSelection::Commit(
                GitCommit::new("8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5").unwrap()
            )
        );
    }

    #[tokio::test]
    async fn branch_workspace_selector_adapts_token_and_branch_identity() {
        let input = WorkspaceSelectorInput {
            token: "ghp_secret",
            ..github_workspace_input()
        };

        let selector =
            WorkspaceSelector::for_branch_workspace(input, "rototo-console/alice/change-checkout")
                .await
                .unwrap();

        assert_eq!(
            selector.source_tree.token,
            TokenIdentity::Sha256Hex(
                "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434".to_owned()
            )
        );
        assert_eq!(
            selector.selection,
            SourceTreeSelection::Branch(
                BranchName::new("rototo-console/alice/change-checkout").unwrap()
            )
        );
    }

    #[tokio::test]
    async fn selector_adapter_preserves_generic_git_remote_identity() {
        let input = WorkspaceSelectorInput {
            principal_id: "user_123",
            token: "",
            owner: "Team",
            name: "Config",
            path: "services/api",
            git_ref: "main",
            source: "git+https://Git.Example.com/Team/Config.git#main:services/api",
        };

        let selector = WorkspaceSelector::for_base_workspace(input).await.unwrap();

        assert_eq!(
            selector.source_tree.source,
            SourceTree::GitRemote {
                remote_url: "git+https://git.example.com/Team/Config.git".to_owned()
            }
        );
        assert_eq!(selector.path, WorkspacePath::new("services/api").unwrap());
        assert_eq!(
            selector.selection,
            SourceTreeSelection::BaseRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn selector_adapter_maps_local_workspace_to_current_tree() {
        let tempdir = TempDir::new().expect("tempdir");
        let input = WorkspaceSelectorInput {
            principal_id: "local-user",
            token: "",
            owner: "demo",
            name: "config",
            path: ".",
            git_ref: "main",
            source: tempdir.path().to_str().expect("utf8 temp path"),
        };

        let selector = WorkspaceSelector::for_base_workspace(input).await.unwrap();

        assert_eq!(
            selector.source_tree.source,
            SourceTree::LocalFolder {
                root: tempdir.path().canonicalize().unwrap()
            }
        );
        assert_eq!(selector.path, WorkspacePath::root());
        assert_eq!(selector.selection, SourceTreeSelection::CurrentTree);
    }

    #[tokio::test]
    async fn selector_adapter_maps_github_archive_store_shape_to_git_tree() {
        let input = WorkspaceSelectorInput {
            principal_id: "user_123",
            token: "",
            owner: "Rototo",
            name: "Config",
            path: "workspaces/payments",
            git_ref: "main",
            source: "https://API.GITHUB.com/repos/Rototo/Config/tarball/main#:workspaces/payments",
        };

        let selector = WorkspaceSelector::for_base_workspace(input).await.unwrap();

        assert_eq!(
            selector.source_tree.source,
            SourceTree::GitHub {
                owner: "rototo".to_owned(),
                name: "config".to_owned(),
            }
        );
        assert_eq!(
            selector.selection,
            SourceTreeSelection::BaseRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn selector_adapter_rejects_unresolved_arbitrary_archive() {
        let input = WorkspaceSelectorInput {
            principal_id: "user_123",
            token: "",
            owner: "demo",
            name: "config",
            path: "workspaces/payments",
            git_ref: "main",
            source: "https://example.com/releases/config.tar.gz#:workspaces/payments",
        };

        assert!(WorkspaceSelector::for_base_workspace(input).await.is_err());
        assert!(
            WorkspaceSelector::for_branch_workspace(input, "rototo-console/alice/change")
                .await
                .is_err()
        );
    }
}
