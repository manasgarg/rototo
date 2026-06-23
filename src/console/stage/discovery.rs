use std::path::{Path, PathBuf};

use super::DiscoveredPackages;
use super::PackagePath;
use crate::error::{Result, RototoError};

const PACKAGE_MANIFEST: &str = "rototo-package.toml";

pub async fn discover_packages(root: &Path) -> Result<DiscoveredPackages> {
    let paths = discover_package_paths(root).await?;
    Ok(DiscoveredPackages { paths })
}

async fn discover_package_paths(root: &Path) -> Result<Vec<PackagePath>> {
    let mut packages = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut entries = read_sorted_dir(&dir).await?;
        for entry in entries.drain(..) {
            if entry.file_name == PACKAGE_MANIFEST && entry.kind == DiscoveredEntryKind::File {
                packages.push(package_path_for_manifest(root, &entry.path)?);
            } else if entry.kind == DiscoveredEntryKind::Directory {
                stack.push(entry.path);
            }
        }
    }

    packages.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok(packages)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiscoveredEntryKind {
    Directory,
    File,
    Other,
}

struct DiscoveredEntry {
    file_name: String,
    path: PathBuf,
    kind: DiscoveredEntryKind,
}

async fn read_sorted_dir(dir: &Path) -> Result<Vec<DiscoveredEntry>> {
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .map_err(|err| RototoError::new(format!("failed to read `{}`: {err}", dir.display())))?;
    let mut entries = Vec::new();

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to read `{}`: {err}", dir.display())))?
    {
        let file_type = entry.file_type().await.map_err(|err| {
            RototoError::new(format!(
                "failed to read file type for `{}`: {err}",
                entry.path().display()
            ))
        })?;
        let kind = if file_type.is_dir() {
            DiscoveredEntryKind::Directory
        } else if file_type.is_file() {
            DiscoveredEntryKind::File
        } else {
            DiscoveredEntryKind::Other
        };
        entries.push(DiscoveredEntry {
            file_name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
            kind,
        });
    }

    entries.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(entries)
}

fn package_path_for_manifest(root: &Path, manifest: &Path) -> Result<PackagePath> {
    let package_root = manifest
        .parent()
        .ok_or_else(|| RototoError::new("package manifest has no parent directory"))?;
    let relative = package_root.strip_prefix(root).map_err(|err| {
        RototoError::new(format!(
            "package manifest `{}` is outside staged root `{}`: {err}",
            manifest.display(),
            root.display()
        ))
    })?;
    if relative.as_os_str().is_empty() {
        return Ok(PackagePath::root());
    }
    PackagePath::new(relative.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::source_tree;
    use crate::console::stage::{
        CachedSourceTreeOrigin, GitRefName, SourceTreeOrigin, SourceTreeRevision, TokenIdentity,
    };

    #[tokio::test]
    async fn discovers_packages_in_local_tree() {
        let tree = TempDir::new().expect("tree tempdir");
        write_manifest(tree.path()).await;
        write_manifest(&tree.path().join("packages/payments")).await;
        write_manifest(&tree.path().join("packages/search")).await;
        tokio::fs::create_dir_all(tree.path().join("not-a-package"))
            .await
            .unwrap();

        let discovery = discover_for_source(
            source_key(SourceTreeOrigin::local_folder(tree.path()).await.unwrap()),
            SourceTreeRevision::LocalWorkingTree,
        )
        .await
        .unwrap();

        assert_eq!(
            package_strings(&discovery.paths),
            vec![".", "packages/payments", "packages/search"]
        );
    }

    #[tokio::test]
    async fn discovers_packages_from_git_base_ref() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_manifest(repo.path()).await;
        write_manifest(&repo.path().join("packages/payments")).await;
        commit_all(repo.path(), "add packages");

        let discovery = discover_for_source(
            source_key(SourceTreeOrigin::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            }),
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
        )
        .await
        .unwrap();

        assert_eq!(
            package_strings(&discovery.paths),
            vec![".", "packages/payments"]
        );
    }

    #[tokio::test]
    async fn discovers_packages_from_git_branch_revision() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_manifest(repo.path()).await;
        commit_all(repo.path(), "add root package");
        run_git(repo.path(), &["checkout", "-b", "feature/payments"]);
        write_manifest(&repo.path().join("packages/payments")).await;
        commit_all(repo.path(), "add payments package");

        let discovery = discover_for_source(
            source_key(SourceTreeOrigin::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            }),
            SourceTreeRevision::git_branch("feature/payments").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(
            package_strings(&discovery.paths),
            vec![".", "packages/payments"]
        );
    }

    #[tokio::test]
    async fn git_discovery_does_not_resolve_package_extends() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        tokio::fs::write(
            repo.path().join(PACKAGE_MANIFEST),
            r#"schema_version = 1
extends = ["git+file:///missing/parent-package#main"]
"#,
        )
        .await
        .unwrap();
        commit_all(repo.path(), "add extending package");

        let discovery = discover_for_source(
            source_key(SourceTreeOrigin::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            }),
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
        )
        .await
        .unwrap();

        assert_eq!(package_strings(&discovery.paths), vec!["."]);
    }

    #[tokio::test]
    async fn rejects_revision_that_does_not_match_source_tree_origin_kind() {
        let tree = TempDir::new().expect("tree tempdir");
        let err = discover_for_source(
            source_key(SourceTreeOrigin::local_folder(tree.path()).await.unwrap()),
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("revision is not valid"));
    }

    async fn write_manifest(root: &Path) {
        tokio::fs::create_dir_all(root).await.unwrap();
        tokio::fs::write(root.join(PACKAGE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();
    }

    fn source_key(source: SourceTreeOrigin) -> CachedSourceTreeOrigin {
        CachedSourceTreeOrigin::new("user_123", source, TokenIdentity::None).unwrap()
    }

    async fn discover_for_source(
        cached_tree: CachedSourceTreeOrigin,
        revision: SourceTreeRevision,
    ) -> Result<DiscoveredPackages> {
        let staged =
            source_tree::stage_tree_for_revision(cached_tree.clone(), revision.clone()).await?;
        discover_packages(staged.root()).await
    }

    fn package_strings(packages: &[PackagePath]) -> Vec<&str> {
        packages.iter().map(PackagePath::as_str).collect()
    }

    fn init_repo(root: &Path) {
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "rototo@example.com"]);
        run_git(root, &["config", "user.name", "Rototo Test"]);
    }

    fn commit_all(root: &Path, message: &str) {
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-m", message]);
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap_or_else(|err| panic!("failed to run git {}: {err}", args.join(" ")));
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
