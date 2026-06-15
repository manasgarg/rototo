use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Result, RototoError};

use super::inventory::workspace_local_path;
use super::store::WorkspaceRecord;

/// Result of publishing a direct-push branch through local git.
///
/// The route creates this after staging tracked branch paths, committing if
/// needed, and attempting to push the current branch. It is serialized once to
/// the browser; the struct itself is not persisted.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPublishResult {
    pub commit: Option<String>,
    pub pushed: bool,
    pub upstream: Option<String>,
    pub push_error: Option<String>,
}

pub fn workspace_root(source: &str) -> Result<PathBuf> {
    if let Some(path) = source.strip_prefix("file://") {
        return Ok(PathBuf::from(path));
    }
    if source.contains("://") {
        return Err(RototoError::new(
            "local git writes require a local path or file:// workspace source",
        ));
    }
    Ok(PathBuf::from(source))
}

pub async fn current_branch(source: &str) -> Result<String> {
    let root = workspace_root(source)?;
    let output = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    let branch = output.trim();
    if branch.is_empty() || branch == "HEAD" {
        return Err(RototoError::new(
            "local workspace is not on a named git branch",
        ));
    }
    Ok(branch.to_owned())
}

pub async fn head_sha(source: &str) -> Result<String> {
    let root = workspace_root(source)?;
    let output = git(&root, &["rev-parse", "HEAD"]).await?;
    let sha = output.trim();
    if sha.is_empty() {
        return Err(RototoError::new("local workspace has no HEAD commit"));
    }
    Ok(sha.to_owned())
}

pub async fn changed_paths(workspace: &WorkspaceRecord, base_ref: &str) -> Result<Vec<String>> {
    let root = workspace_root(&workspace.source)?;
    let pathspec = if workspace.path == "." {
        ".".to_owned()
    } else {
        workspace.path.clone()
    };
    let mut paths = std::collections::BTreeSet::new();
    if let Ok(diff) = git(
        &root,
        &[
            "diff",
            "--name-only",
            &format!("{base_ref}...HEAD"),
            "--",
            &pathspec,
        ],
    )
    .await
    {
        paths.extend(
            diff.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
        );
    }
    let status = git(&root, &["status", "--porcelain", "--", &pathspec]).await?;
    paths.extend(status.lines().filter_map(status_path));
    Ok(paths.into_iter().collect())
}

pub async fn read_file(workspace: &WorkspaceRecord, file_path: &str) -> Result<String> {
    let root = workspace_root(&workspace.source)?;
    let relative = workspace_local_path(workspace, file_path)?;
    tokio::fs::read_to_string(root.join(relative))
        .await
        .map_err(|err| RototoError::new(format!("failed to read {file_path}: {err}")))
}

pub async fn write_file(workspace: &WorkspaceRecord, file_path: &str, content: &str) -> Result<()> {
    let root = workspace_root(&workspace.source)?;
    let relative = workspace_local_path(workspace, file_path)?;
    let path = root.join(relative);
    ensure_inside(&root, &path)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| {
            RototoError::new(format!("failed to create {}: {err}", parent.display()))
        })?;
    }
    tokio::fs::write(&path, content)
        .await
        .map_err(|err| RototoError::new(format!("failed to write {file_path}: {err}")))
}

pub async fn delete_file(workspace: &WorkspaceRecord, file_path: &str) -> Result<()> {
    let root = workspace_root(&workspace.source)?;
    let relative = workspace_local_path(workspace, file_path)?;
    let path = root.join(relative);
    ensure_inside(&root, &path)?;
    tokio::fs::remove_file(&path)
        .await
        .map_err(|err| RototoError::new(format!("failed to delete {file_path}: {err}")))
}

pub async fn file_exists(workspace: &WorkspaceRecord, file_path: &str) -> Result<bool> {
    let root = workspace_root(&workspace.source)?;
    let relative = workspace_local_path(workspace, file_path)?;
    Ok(tokio::fs::metadata(root.join(relative)).await.is_ok())
}

pub async fn commit_and_push(
    workspace: &WorkspaceRecord,
    paths: &[String],
    message: &str,
) -> Result<LocalPublishResult> {
    let root = workspace_root(&workspace.source)?;
    if paths.is_empty() {
        return Err(RototoError::new("branch has no files to commit"));
    }
    let mut relative_paths = Vec::with_capacity(paths.len());
    for path in paths {
        let relative = workspace_local_path(workspace, path)?;
        git(&root, &["add", "--", &relative]).await?;
        relative_paths.push(relative);
    }
    let mut status_args = vec!["status", "--porcelain", "--"];
    status_args.extend(relative_paths.iter().map(String::as_str));
    let status = git(&root, &status_args).await?;
    if status.trim().is_empty() {
        return Ok(LocalPublishResult {
            commit: None,
            pushed: false,
            upstream: None,
            push_error: None,
        });
    }
    let mut commit_args = vec!["commit", "-m", message, "--"];
    commit_args.extend(relative_paths.iter().map(String::as_str));
    git(&root, &commit_args).await?;
    let commit = git(&root, &["rev-parse", "HEAD"])
        .await
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let upstream = git(
        &root,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .await
    .ok()
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty());
    let Some(upstream) = upstream else {
        return Ok(LocalPublishResult {
            commit,
            pushed: false,
            upstream: None,
            push_error: None,
        });
    };
    match git(&root, &["push"]).await {
        Ok(_) => Ok(LocalPublishResult {
            commit,
            pushed: true,
            upstream: Some(upstream),
            push_error: None,
        }),
        Err(err) => Ok(LocalPublishResult {
            commit,
            pushed: false,
            upstream: Some(upstream),
            push_error: Some(err.to_string()),
        }),
    }
}

async fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .map_err(|err| RototoError::new(format!("failed to run git {}: {err}", args.join(" "))))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(RototoError::new(format!(
        "git {} failed: {}",
        args.join(" "),
        if stderr.is_empty() {
            output.status.to_string()
        } else {
            stderr
        }
    )))
}

fn status_path(line: &str) -> Option<String> {
    let line = line.trim_end();
    if line.len() < 4 {
        return None;
    }
    let path = if line.starts_with('R') || line.starts_with('C') {
        line.rsplit(" -> ").next().unwrap_or(&line[3..])
    } else {
        &line[3..]
    };
    let path = path.trim().trim_matches('"');
    (!path.is_empty()).then(|| path.to_owned())
}

fn ensure_inside(root: &Path, path: &Path) -> Result<()> {
    let root = root
        .canonicalize()
        .map_err(|err| RototoError::new(format!("failed to resolve {}: {err}", root.display())))?;
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if !candidate.starts_with(&root) {
        return Err(RototoError::new("file path escapes the workspace root"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
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
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn workspace(root: &Path) -> WorkspaceRecord {
        WorkspaceRecord {
            id: "workspace".to_owned(),
            slug: "workspace".to_owned(),
            repo_id: "repo".to_owned(),
            owner: "local".to_owned(),
            name: "workspace".to_owned(),
            path: ".".to_owned(),
            git_ref: "main".to_owned(),
            source: root.display().to_string(),
            discovered_at: "2026-06-14T00:00:00Z".to_owned(),
        }
    }

    #[tokio::test]
    async fn commit_and_push_commits_only_requested_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        run_git(root, &["init"]);
        run_git(root, &["config", "user.name", "Test User"]);
        run_git(root, &["config", "user.email", "test@example.com"]);
        std::fs::write(root.join("tracked.txt"), "base\n").unwrap();
        std::fs::write(root.join("other.txt"), "base\n").unwrap();
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-m", "initial"]);

        std::fs::write(root.join("tracked.txt"), "branch\n").unwrap();
        std::fs::write(root.join("other.txt"), "unrelated\n").unwrap();
        run_git(root, &["add", "other.txt"]);

        let result = commit_and_push(&workspace(root), &["tracked.txt".to_owned()], "branch")
            .await
            .unwrap();

        assert!(result.commit.is_some());
        assert!(!result.pushed);
        assert_eq!(run_git(root, &["show", "HEAD:tracked.txt"]), "branch\n");
        assert_eq!(run_git(root, &["show", "HEAD:other.txt"]), "base\n");
        assert_eq!(
            run_git(root, &["status", "--porcelain", "--", "other.txt"]),
            "M  other.txt\n"
        );
    }

    #[test]
    fn status_path_reads_modified_untracked_and_renamed_paths() {
        assert_eq!(
            status_path(" M variables/a.toml"),
            Some("variables/a.toml".to_owned())
        );
        assert_eq!(
            status_path("?? variables/new.toml"),
            Some("variables/new.toml".to_owned())
        );
        assert_eq!(
            status_path("R  variables/old.toml -> variables/new.toml"),
            Some("variables/new.toml".to_owned())
        );
    }
}
