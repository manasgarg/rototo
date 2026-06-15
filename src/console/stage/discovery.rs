#![allow(dead_code)]

use std::path::{Path, PathBuf};

use super::{CachedTreeSource, TreeRevision, TreeSource, WorkspaceDiscovery, WorkspacePath};
use crate::error::{Result, RototoError};
use crate::source::{SourceOptions, load_workspace_source};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";

pub async fn discover_workspaces(
    cached_tree: CachedTreeSource,
    revision: TreeRevision,
) -> Result<WorkspaceDiscovery> {
    let root = stage_root_for_revision(&cached_tree.tree, &revision).await?;
    let workspaces = discover_workspace_paths(root.path()).await?;
    Ok(WorkspaceDiscovery {
        cached_tree,
        revision,
        workspaces,
    })
}

enum StagedRoot {
    Borrowed(PathBuf),
    Loaded(crate::source::LoadedWorkspaceSource),
}

impl StagedRoot {
    fn path(&self) -> &Path {
        match self {
            Self::Borrowed(path) => path,
            Self::Loaded(loaded) => loaded.staged().path(),
        }
    }
}

async fn stage_root_for_revision(tree: &TreeSource, revision: &TreeRevision) -> Result<StagedRoot> {
    match tree {
        TreeSource::LocalFolder { root } if matches!(revision, TreeRevision::LocalWorkingTree) => {
            Ok(StagedRoot::Borrowed(root.clone()))
        }
        TreeSource::GitHub { owner, name } => {
            let Some(git_ref) = git_ref_for_revision(revision) else {
                return Err(invalid_selection_error());
            };
            stage_git_root(
                &format!("git+https://github.com/{owner}/{name}.git"),
                git_ref,
            )
            .await
        }
        TreeSource::GitRemote { remote_url } => {
            let Some(git_ref) = git_ref_for_revision(revision) else {
                return Err(invalid_selection_error());
            };
            stage_git_root(remote_url, git_ref).await
        }
        TreeSource::Archive { .. } if matches!(revision, TreeRevision::ArchiveSnapshot(_)) => Err(
            RototoError::new("archive workspace discovery requires archive staging support"),
        ),
        _ => Err(invalid_selection_error()),
    }
}

fn git_ref_for_revision(revision: &TreeRevision) -> Option<&str> {
    match revision {
        TreeRevision::GitRef(ref_) => Some(ref_.as_ref()),
        TreeRevision::GitBranch(branch) => Some(branch.as_ref()),
        TreeRevision::GitCommit(commit) => Some(commit.as_ref()),
        TreeRevision::LocalWorkingTree | TreeRevision::ArchiveSnapshot(_) => None,
    }
}

fn invalid_selection_error() -> RototoError {
    RototoError::new("tree revision is not valid for workspace discovery")
}

async fn stage_git_root(remote_url: &str, git_ref: &str) -> Result<StagedRoot> {
    let source = format!("{remote_url}#{git_ref}");
    let loaded = load_workspace_source(&source, &SourceOptions::default()).await?;
    Ok(StagedRoot::Loaded(loaded))
}

async fn discover_workspace_paths(root: &Path) -> Result<Vec<WorkspacePath>> {
    let mut workspaces = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut entries = read_sorted_dir(&dir).await?;
        for entry in entries.drain(..) {
            if entry.file_name == WORKSPACE_MANIFEST && entry.kind == DiscoveredEntryKind::File {
                workspaces.push(workspace_path_for_manifest(root, &entry.path)?);
            } else if entry.kind == DiscoveredEntryKind::Directory {
                stack.push(entry.path);
            }
        }
    }

    workspaces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok(workspaces)
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

fn workspace_path_for_manifest(root: &Path, manifest: &Path) -> Result<WorkspacePath> {
    let workspace_root = manifest
        .parent()
        .ok_or_else(|| RototoError::new("workspace manifest has no parent directory"))?;
    let relative = workspace_root.strip_prefix(root).map_err(|err| {
        RototoError::new(format!(
            "workspace manifest `{}` is outside staged root `{}`: {err}",
            manifest.display(),
            root.display()
        ))
    })?;
    if relative.as_os_str().is_empty() {
        return Ok(WorkspacePath::root());
    }
    WorkspacePath::new(relative.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::{CachedTreeSource, GitRefName, TokenIdentity};

    #[tokio::test]
    async fn discovers_workspaces_in_local_tree() {
        let tree = TempDir::new().expect("tree tempdir");
        write_manifest(tree.path()).await;
        write_manifest(&tree.path().join("workspaces/payments")).await;
        write_manifest(&tree.path().join("workspaces/search")).await;
        tokio::fs::create_dir_all(tree.path().join("not-a-workspace"))
            .await
            .unwrap();

        let discovery = discover_workspaces(
            source_key(TreeSource::local_folder(tree.path()).await.unwrap()),
            TreeRevision::LocalWorkingTree,
        )
        .await
        .unwrap();

        assert_eq!(
            workspace_strings(&discovery.workspaces),
            vec![".", "workspaces/payments", "workspaces/search"]
        );
    }

    #[tokio::test]
    async fn discovers_workspaces_from_git_base_ref() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_manifest(repo.path()).await;
        write_manifest(&repo.path().join("workspaces/payments")).await;
        commit_all(repo.path(), "add workspaces");

        let discovery = discover_workspaces(
            source_key(TreeSource::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            }),
            TreeRevision::GitRef(GitRefName::new("main").unwrap()),
        )
        .await
        .unwrap();

        assert_eq!(
            workspace_strings(&discovery.workspaces),
            vec![".", "workspaces/payments"]
        );
    }

    #[tokio::test]
    async fn discovers_workspaces_from_git_branch_revision() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_manifest(repo.path()).await;
        commit_all(repo.path(), "add root workspace");
        run_git(repo.path(), &["checkout", "-b", "feature/payments"]);
        write_manifest(&repo.path().join("workspaces/payments")).await;
        commit_all(repo.path(), "add payments workspace");

        let discovery = discover_workspaces(
            source_key(TreeSource::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            }),
            TreeRevision::git_branch("feature/payments").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(
            workspace_strings(&discovery.workspaces),
            vec![".", "workspaces/payments"]
        );
    }

    #[tokio::test]
    async fn rejects_revision_that_does_not_match_tree_source_kind() {
        let tree = TempDir::new().expect("tree tempdir");
        let err = discover_workspaces(
            source_key(TreeSource::local_folder(tree.path()).await.unwrap()),
            TreeRevision::GitRef(GitRefName::new("main").unwrap()),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("revision is not valid"));
    }

    async fn write_manifest(root: &Path) {
        tokio::fs::create_dir_all(root).await.unwrap();
        tokio::fs::write(root.join(WORKSPACE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();
    }

    fn source_key(source: TreeSource) -> CachedTreeSource {
        CachedTreeSource::new("user_123", source, TokenIdentity::none()).unwrap()
    }

    fn workspace_strings(workspaces: &[WorkspacePath]) -> Vec<&str> {
        workspaces.iter().map(WorkspacePath::as_str).collect()
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
