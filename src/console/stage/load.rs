use std::path::Path;
use std::sync::Arc;

use super::{CachedPackageLocator, SourceTreeOrigin, SourceTreeRevision};
use crate::error::{Result, RototoError};
use crate::sdk::Package;
use crate::source::{SourceAuth, SourceOptions};

pub async fn get_inspected_package(
    selector: CachedPackageLocator,
    source_token: &str,
) -> Result<Arc<Package>> {
    let source = source_for_selector(&selector)?;
    let options = source_options(source_token);
    let package = Package::inspect_with_source_options(source, &options).await?;
    Ok(Arc::new(package))
}

fn source_options(source_token: &str) -> SourceOptions {
    if source_token.is_empty() {
        SourceOptions::default()
    } else {
        SourceOptions::default().with_auth(SourceAuth::Bearer(source_token.to_owned()))
    }
}

fn source_for_selector(selector: &CachedPackageLocator) -> Result<String> {
    match &selector.package.source_tree.origin {
        SourceTreeOrigin::LocalFolder { root }
            if matches!(
                selector.package.source_tree.revision,
                SourceTreeRevision::LocalWorkingTree
            ) =>
        {
            Ok(local_package_source(root, selector.package.path.as_str()))
        }
        SourceTreeOrigin::GitHub { owner, name } => {
            let Some(git_ref) = git_ref_for_revision(&selector.package.source_tree.revision) else {
                return Err(invalid_selection_error());
            };
            Ok(git_package_source(
                &format!("git+https://github.com/{owner}/{name}.git"),
                git_ref,
                selector.package.path.as_str(),
            ))
        }
        SourceTreeOrigin::GitRemote { remote_url } => {
            let Some(git_ref) = git_ref_for_revision(&selector.package.source_tree.revision) else {
                return Err(invalid_selection_error());
            };
            Ok(git_package_source(
                remote_url,
                git_ref,
                selector.package.path.as_str(),
            ))
        }
        SourceTreeOrigin::Archive { url }
            if matches!(
                selector.package.source_tree.revision,
                SourceTreeRevision::ArchiveSnapshot
            ) =>
        {
            Ok(archive_package_source(url, selector.package.path.as_str()))
        }
        _ => Err(invalid_selection_error()),
    }
}

fn local_package_source(root: &Path, package_path: &str) -> String {
    if package_path == "." {
        root.display().to_string()
    } else {
        root.join(package_path).display().to_string()
    }
}

fn git_package_source(remote_url: &str, git_ref: &str, package_path: &str) -> String {
    if package_path == "." {
        format!("{remote_url}#{git_ref}")
    } else {
        format!("{remote_url}#{git_ref}:{package_path}")
    }
}

fn archive_package_source(url: &str, package_path: &str) -> String {
    if package_path == "." {
        url.to_owned()
    } else {
        format!("{url}#:{package_path}")
    }
}

fn git_ref_for_revision(revision: &SourceTreeRevision) -> Option<&str> {
    match revision {
        SourceTreeRevision::GitRef(ref_) => Some(ref_.as_ref()),
        SourceTreeRevision::GitBranch(branch) => Some(branch.as_ref()),
        SourceTreeRevision::GitCommit(commit) => Some(commit.as_ref()),
        SourceTreeRevision::LocalWorkingTree | SourceTreeRevision::ArchiveSnapshot => None,
    }
}

fn invalid_selection_error() -> RototoError {
    RototoError::new("tree revision is not valid for package inspection")
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::{GitRefName, PackageLocator, PackagePath, TokenIdentity};

    #[tokio::test]
    async fn inspects_local_package_path_from_source_tree_root() {
        let tree = TempDir::new().expect("tree tempdir");
        write_manifest(&tree.path().join("packages/payments")).await;

        let selector = cached_package_source(
            SourceTreeOrigin::local_folder(tree.path()).await.unwrap(),
            SourceTreeRevision::LocalWorkingTree,
            "packages/payments",
        );

        let package = get_inspected_package(selector, "").await.unwrap();

        assert_eq!(
            package.root(),
            tokio::fs::canonicalize(tree.path().join("packages/payments"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn inspects_git_package_path_from_selected_ref() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_manifest(&repo.path().join("packages/payments")).await;
        commit_all(repo.path(), "add package");

        let selector = cached_package_source(
            SourceTreeOrigin::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            },
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
            "packages/payments",
        );

        let package = get_inspected_package(selector, "").await.unwrap();

        assert!(
            package.root().join("rototo-package.toml").is_file(),
            "staged package should contain the manifest"
        );
    }

    #[tokio::test]
    async fn selected_package_inspection_resolves_extends_layers() {
        let tree = TempDir::new().expect("tree tempdir");
        write_manifest(&tree.path().join("base")).await;
        tokio::fs::create_dir_all(tree.path().join("child"))
            .await
            .unwrap();
        tokio::fs::write(
            tree.path().join("child/rototo-package.toml"),
            r#"schema_version = 1
extends = ["../base"]
"#,
        )
        .await
        .unwrap();

        let selector = cached_package_source(
            SourceTreeOrigin::local_folder(tree.path()).await.unwrap(),
            SourceTreeRevision::LocalWorkingTree,
            "child",
        );

        let package = get_inspected_package(selector, "").await.unwrap();

        assert_eq!(package.source_layers().len(), 2);
    }

    #[tokio::test]
    async fn rejects_revision_that_does_not_match_inspection_source_tree_origin() {
        let tree = TempDir::new().expect("tree tempdir");
        write_manifest(tree.path()).await;

        let selector = cached_package_source(
            SourceTreeOrigin::local_folder(tree.path()).await.unwrap(),
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
            ".",
        );

        let err = get_inspected_package(selector, "").await.unwrap_err();

        assert!(err.to_string().contains("tree revision is not valid"));
    }

    #[test]
    fn package_locator_strings_keep_source_tree_and_package_path_separate() {
        let selector = cached_package_source(
            SourceTreeOrigin::GitRemote {
                remote_url: "git+file:///tmp/configs".to_owned(),
            },
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
            "apps/payments",
        );

        assert_eq!(
            source_for_selector(&selector).unwrap(),
            "git+file:///tmp/configs#main:apps/payments"
        );
    }

    #[test]
    fn archive_locator_strings_keep_snapshot_and_package_path_separate() {
        let selector = cached_package_source(
            SourceTreeOrigin::Archive {
                url: "https://example.com/config.tar.gz".to_owned(),
            },
            SourceTreeRevision::ArchiveSnapshot,
            "apps/payments",
        );

        assert_eq!(
            source_for_selector(&selector).unwrap(),
            "https://example.com/config.tar.gz#:apps/payments"
        );
    }

    async fn write_manifest(path: &Path) {
        tokio::fs::create_dir_all(path).await.unwrap();
        tokio::fs::write(path.join("rototo-package.toml"), "schema_version = 1\n")
            .await
            .unwrap();
    }

    fn cached_package_source(
        tree: SourceTreeOrigin,
        revision: SourceTreeRevision,
        path: &str,
    ) -> CachedPackageLocator {
        CachedPackageLocator::new(
            "user_123",
            PackageLocator::new(tree, revision, PackagePath::new(path).unwrap()),
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
