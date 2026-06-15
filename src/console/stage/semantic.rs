#![allow(dead_code)]

use std::sync::Arc;

use super::load;
use super::{CachedWorkspaceSource, SemanticWorkspace};
use crate::error::Result;

pub async fn get_semantic_workspace(
    selector: CachedWorkspaceSource,
    source_token: &str,
) -> Result<SemanticWorkspace> {
    let workspace = load::get_inspected_workspace(selector, source_token).await?;
    let model = workspace.semantic_model().await?;
    Ok(SemanticWorkspace {
        workspace,
        model: Arc::new(model),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::{
        GitRefName, TokenIdentity, TreeRevision, TreeSource, WorkspacePath, WorkspaceSource,
    };

    #[tokio::test]
    async fn semantic_workspace_uses_the_inspected_local_workspace_root() {
        let tree = TempDir::new().expect("tree tempdir");
        write_workspace(&tree.path().join("workspaces/payments")).await;

        let selector = cached_workspace_source(
            TreeSource::local_folder(tree.path()).await.unwrap(),
            TreeRevision::LocalWorkingTree,
            "workspaces/payments",
        );

        let semantic = get_semantic_workspace(selector, "").await.unwrap();

        assert_eq!(
            semantic.workspace.root(),
            tokio::fs::canonicalize(tree.path().join("workspaces/payments"))
                .await
                .unwrap()
        );
        assert_eq!(semantic.model.version, 3);
        assert_eq!(semantic.model.variables.len(), 1);
    }

    #[tokio::test]
    async fn semantic_workspace_loads_from_git_revision() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_workspace(&repo.path().join("workspaces/payments")).await;
        commit_all(repo.path(), "add workspace");

        let selector = cached_workspace_source(
            TreeSource::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            },
            TreeRevision::GitRef(GitRefName::new("main").unwrap()),
            "workspaces/payments",
        );

        let semantic = get_semantic_workspace(selector, "").await.unwrap();

        assert!(
            semantic
                .workspace
                .root()
                .join("rototo-workspace.toml")
                .is_file(),
            "staged workspace should contain the manifest"
        );
        assert_eq!(semantic.model.variables.len(), 1);
    }

    async fn write_workspace(path: &Path) {
        tokio::fs::create_dir_all(path.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(path.join("rototo-workspace.toml"), "schema_version = 1\n")
            .await
            .unwrap();
        tokio::fs::write(
            path.join("variables/checkout.toml"),
            r#"
schema_version = 1
type = "bool"

[values]
enabled = true

[resolve]
default = "enabled"
"#
            .trim_start(),
        )
        .await
        .unwrap();
    }

    fn cached_workspace_source(
        tree: TreeSource,
        revision: TreeRevision,
        path: &str,
    ) -> CachedWorkspaceSource {
        CachedWorkspaceSource::new(
            "user_123",
            WorkspaceSource::new(tree, revision, WorkspacePath::new(path).unwrap()),
            TokenIdentity::none(),
        )
        .unwrap()
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
