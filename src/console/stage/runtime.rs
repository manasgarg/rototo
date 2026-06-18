use std::sync::Arc;

use crate::error::Result;
use crate::sdk::{LoadOptions, Workspace};
use crate::source::SourceAuth;

pub(super) async fn get_runtime_workspace_from_inspected(
    inspected: Arc<Workspace>,
    source_token: &str,
) -> Result<Arc<Workspace>> {
    let inspected_root = inspected.root().to_string_lossy().into_owned();
    let runtime = Workspace::load_snapshot_with_options(
        inspected_root,
        load_options_for_source_token(source_token),
    )
    .await?;
    Ok(Arc::new(runtime))
}

fn load_options_for_source_token(source_token: &str) -> LoadOptions {
    if source_token.is_empty() {
        LoadOptions::default()
    } else {
        LoadOptions::default().with_source_auth(SourceAuth::Bearer(source_token.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::load;
    use crate::console::stage::{
        CachedWorkspaceLocator, GitRefName, SourceTreeOrigin, SourceTreeRevision, TokenIdentity,
        WorkspaceLocator, WorkspacePath,
    };
    use crate::sdk::ResolveContext;

    #[tokio::test]
    async fn runtime_workspace_resolves_from_local_workspace_source() {
        let tree = TempDir::new().expect("tree tempdir");
        write_workspace(&tree.path().join("workspaces/payments")).await;

        let selector = cached_workspace_source(
            SourceTreeOrigin::local_folder(tree.path()).await.unwrap(),
            SourceTreeRevision::LocalWorkingTree,
            "workspaces/payments",
        );

        let inspected = load::get_inspected_workspace(selector, "").await.unwrap();
        let runtime = get_runtime_workspace_from_inspected(inspected, "")
            .await
            .unwrap();
        let resolved = runtime
            .resolve_variable(
                "checkout",
                &ResolveContext::from_json(json!({})).expect("object context"),
            )
            .await
            .unwrap();

        assert_eq!(resolved.value, json!(true));
    }

    #[tokio::test]
    async fn runtime_workspace_resolves_from_git_revision_and_owns_staged_files() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_workspace(&repo.path().join("workspaces/payments")).await;
        commit_all(repo.path(), "add workspace");

        let selector = cached_workspace_source(
            SourceTreeOrigin::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            },
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
            "workspaces/payments",
        );

        let inspected = load::get_inspected_workspace(selector, "").await.unwrap();
        let runtime = get_runtime_workspace_from_inspected(inspected, "")
            .await
            .unwrap();

        assert!(
            runtime.root().join("rototo-workspace.toml").is_file(),
            "runtime workspace should own a snapshot of the inspected files"
        );
        let resolved = runtime
            .resolve_variable(
                "checkout",
                &ResolveContext::from_json(json!({})).expect("object context"),
            )
            .await
            .unwrap();
        assert_eq!(resolved.value, json!(true));
    }

    #[tokio::test]
    async fn runtime_workspace_is_lint_gated_but_inspection_still_succeeds() {
        let tree = TempDir::new().expect("tree tempdir");
        write_lint_broken_workspace(&tree.path().join("workspaces/payments")).await;
        let selector = cached_workspace_source(
            SourceTreeOrigin::local_folder(tree.path()).await.unwrap(),
            SourceTreeRevision::LocalWorkingTree,
            "workspaces/payments",
        );

        let inspected = load::get_inspected_workspace(selector.clone(), "")
            .await
            .unwrap();
        assert!(inspected.root().join("rototo-workspace.toml").is_file());

        let err = get_runtime_workspace_from_inspected(inspected, "")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("workspace lint failed"));
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

[resolve]
default = true
"#
            .trim_start(),
        )
        .await
        .unwrap();
    }

    async fn write_lint_broken_workspace(path: &Path) {
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

[resolve]
default = "yes"
"#
            .trim_start(),
        )
        .await
        .unwrap();
    }

    fn cached_workspace_source(
        tree: SourceTreeOrigin,
        revision: SourceTreeRevision,
        path: &str,
    ) -> CachedWorkspaceLocator {
        CachedWorkspaceLocator::new(
            "user_123",
            WorkspaceLocator::new(tree, revision, WorkspacePath::new(path).unwrap()),
            TokenIdentity::None,
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
