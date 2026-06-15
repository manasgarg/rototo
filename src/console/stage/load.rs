#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use super::{SourceTree, SourceTreeSelection, WorkspaceSelector};
use crate::error::{Result, RototoError};
use crate::sdk::Workspace;
use crate::source::{SourceAuth, SourceOptions};

pub async fn get_inspected_workspace(
    selector: WorkspaceSelector,
    source_token: &str,
) -> Result<Arc<Workspace>> {
    let source = source_for_selector(&selector)?;
    let options = source_options(source_token);
    let workspace = Workspace::inspect_with_source_options(source, &options).await?;
    Ok(Arc::new(workspace))
}

fn source_options(source_token: &str) -> SourceOptions {
    if source_token.is_empty() {
        SourceOptions::default()
    } else {
        SourceOptions::default().with_auth(SourceAuth::Bearer(source_token.to_owned()))
    }
}

fn source_for_selector(selector: &WorkspaceSelector) -> Result<String> {
    match &selector.source_tree.source {
        SourceTree::LocalFolder { root }
            if matches!(selector.selection, SourceTreeSelection::CurrentTree) =>
        {
            Ok(local_workspace_source(root, selector.path.as_str()))
        }
        SourceTree::GitHub { owner, name } => {
            let Some(git_ref) = git_ref_for_selection(&selector.selection) else {
                return Err(invalid_selection_error());
            };
            Ok(git_workspace_source(
                &format!("git+https://github.com/{owner}/{name}.git"),
                git_ref,
                selector.path.as_str(),
            ))
        }
        SourceTree::GitRemote { remote_url } => {
            let Some(git_ref) = git_ref_for_selection(&selector.selection) else {
                return Err(invalid_selection_error());
            };
            Ok(git_workspace_source(
                remote_url,
                git_ref,
                selector.path.as_str(),
            ))
        }
        SourceTree::Archive { .. }
            if matches!(
                selector.selection,
                SourceTreeSelection::ArchiveFingerprint(_)
            ) =>
        {
            Err(RototoError::new(
                "archive workspace inspection requires archive staging support",
            ))
        }
        _ => Err(invalid_selection_error()),
    }
}

fn local_workspace_source(root: &Path, workspace_path: &str) -> String {
    if workspace_path == "." {
        root.display().to_string()
    } else {
        root.join(workspace_path).display().to_string()
    }
}

fn git_workspace_source(remote_url: &str, git_ref: &str, workspace_path: &str) -> String {
    if workspace_path == "." {
        format!("{remote_url}#{git_ref}")
    } else {
        format!("{remote_url}#{git_ref}:{workspace_path}")
    }
}

fn git_ref_for_selection(selection: &SourceTreeSelection) -> Option<&str> {
    match selection {
        SourceTreeSelection::BaseRef(ref_) => Some(ref_.as_ref()),
        SourceTreeSelection::Branch(branch) => Some(branch.as_ref()),
        SourceTreeSelection::Commit(commit) => Some(commit.as_ref()),
        SourceTreeSelection::CurrentTree | SourceTreeSelection::ArchiveFingerprint(_) => None,
    }
}

fn invalid_selection_error() -> RototoError {
    RototoError::new("source tree selection is not valid for workspace inspection")
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::{GitRefName, SourceTreeCacheKey, TokenIdentity, WorkspacePath};

    #[tokio::test]
    async fn inspects_local_workspace_path_from_source_tree_root() {
        let tree = TempDir::new().expect("tree tempdir");
        write_manifest(&tree.path().join("workspaces/payments")).await;

        let selector = WorkspaceSelector {
            source_tree: source_key(SourceTree::local_folder(tree.path()).await.unwrap()),
            path: WorkspacePath::new("workspaces/payments").unwrap(),
            selection: SourceTreeSelection::CurrentTree,
        };

        let workspace = get_inspected_workspace(selector, "").await.unwrap();

        assert_eq!(
            workspace.root(),
            tokio::fs::canonicalize(tree.path().join("workspaces/payments"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn inspects_git_workspace_path_from_selected_ref() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_manifest(&repo.path().join("workspaces/payments")).await;
        commit_all(repo.path(), "add workspace");

        let selector = WorkspaceSelector {
            source_tree: source_key(SourceTree::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            }),
            path: WorkspacePath::new("workspaces/payments").unwrap(),
            selection: SourceTreeSelection::BaseRef(GitRefName::new("main").unwrap()),
        };

        let workspace = get_inspected_workspace(selector, "").await.unwrap();

        assert!(
            workspace.root().join("rototo-workspace.toml").is_file(),
            "staged workspace should contain the manifest"
        );
    }

    #[tokio::test]
    async fn rejects_selection_that_does_not_match_inspection_source_tree() {
        let tree = TempDir::new().expect("tree tempdir");
        write_manifest(tree.path()).await;

        let selector = WorkspaceSelector {
            source_tree: source_key(SourceTree::local_folder(tree.path()).await.unwrap()),
            path: WorkspacePath::root(),
            selection: SourceTreeSelection::BaseRef(GitRefName::new("main").unwrap()),
        };

        let err = get_inspected_workspace(selector, "").await.unwrap_err();

        assert!(
            err.to_string()
                .contains("source tree selection is not valid")
        );
    }

    #[test]
    fn selector_source_strings_keep_tree_and_workspace_path_separate() {
        let selector = WorkspaceSelector {
            source_tree: source_key(SourceTree::GitRemote {
                remote_url: "git+file:///tmp/configs".to_owned(),
            }),
            path: WorkspacePath::new("apps/payments").unwrap(),
            selection: SourceTreeSelection::BaseRef(GitRefName::new("main").unwrap()),
        };

        assert_eq!(
            source_for_selector(&selector).unwrap(),
            "git+file:///tmp/configs#main:apps/payments"
        );
    }

    async fn write_manifest(path: &Path) {
        tokio::fs::create_dir_all(path).await.unwrap();
        tokio::fs::write(path.join("rototo-workspace.toml"), "schema_version = 1\n")
            .await
            .unwrap();
    }

    fn source_key(source: SourceTree) -> SourceTreeCacheKey {
        SourceTreeCacheKey::new("user_123", source, TokenIdentity::none()).unwrap()
    }

    fn init_repo(path: &Path) {
        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "console@example.com"]);
        run_git(path, &["config", "user.name", "Console Test"]);
    }

    fn commit_all(path: &Path, message: &str) {
        run_git(path, &["add", "."]);
        run_git(path, &["commit", "-m", message]);
    }

    fn run_git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .expect("run git");
        assert!(status.success(), "git {:?} failed", args);
    }
}
