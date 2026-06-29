use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::{Result, RototoError};

pub fn package_root(source: &str) -> Result<PathBuf> {
    if let Some(path) = source.strip_prefix("file://") {
        return Ok(PathBuf::from(path));
    }
    if source.contains("://") {
        return Err(RototoError::new(
            "local git writes require a local path or file:// package source",
        ));
    }
    Ok(PathBuf::from(source))
}

pub async fn current_branch(source: &str) -> Result<String> {
    let root = package_root(source)?;
    current_branch_at(&root).await
}

pub async fn current_branch_at(root: &Path) -> Result<String> {
    let output = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    let branch = output.trim();
    if branch.is_empty() || branch == "HEAD" {
        return Err(RototoError::new(
            "local package is not on a named git branch",
        ));
    }
    Ok(branch.to_owned())
}

pub async fn head_commit(root: &Path) -> Result<String> {
    let output = git(root, &["rev-parse", "HEAD"]).await?;
    let commit = output.trim();
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RototoError::new(
            "local package HEAD is not a full git commit",
        ));
    }
    Ok(commit.to_owned())
}

pub async fn changed_paths(root: &Path, scope: &str) -> Result<Vec<String>> {
    let scope = if scope.trim().is_empty() {
        "."
    } else {
        scope.trim()
    };
    let mut paths = BTreeSet::new();
    let diff = git(root, &["diff", "--name-only", "HEAD", "--", scope]).await?;
    paths.extend(diff.lines().filter_map(repo_relative_path));
    let untracked = git(
        root,
        &["ls-files", "--others", "--exclude-standard", "--", scope],
    )
    .await?;
    paths.extend(untracked.lines().filter_map(repo_relative_path));
    Ok(paths.into_iter().collect())
}

pub async fn file_at_head(root: &Path, relative_to_root: &str) -> Result<Option<String>> {
    let path = git_path_for_root(root, relative_to_root).await?;
    let spec = format!("HEAD:{path}");
    match git(root, &["show", &spec]).await {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if git_show_missing_path(&err.to_string()) => Ok(None),
        Err(err) => Err(err),
    }
}

async fn git_path_for_root(root: &Path, relative_to_root: &str) -> Result<String> {
    let prefix = git(root, &["rev-parse", "--show-prefix"]).await?;
    let prefix = prefix.trim();
    let relative = relative_to_root
        .trim()
        .trim_start_matches("./")
        .replace('\\', "/");
    let path = if prefix.is_empty() {
        relative
    } else if relative.is_empty() || relative == "." {
        prefix.trim_end_matches('/').to_owned()
    } else {
        format!("{prefix}{relative}")
    };
    if path.is_empty() || path == "." {
        return Err(RototoError::new("git object path cannot be empty"));
    }
    Ok(path)
}

fn git_show_missing_path(message: &str) -> bool {
    message.contains("exists on disk, but not in 'HEAD'")
        || message.contains("does not exist in 'HEAD'")
        || message.contains("fatal: path")
}

fn repo_relative_path(path: &str) -> Option<String> {
    let path = path.trim().trim_matches('"').replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }
    Some(path)
}

async fn git(root: &Path, args: &[&str]) -> Result<String> {
    let started = std::time::Instant::now();
    let command_label = format!("git {}", args.join(" "));
    tracing::debug!(
        operation = "process.command",
        command = %command_label,
        cwd = %root.display(),
        "console outbound process call started"
    );
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .map_err(|err| {
            tracing::warn!(
                operation = "process.command",
                command = %command_label,
                cwd = %root.display(),
                error = %err,
                latency_ms = started.elapsed().as_millis(),
                "console outbound process call failed to start"
            );
            RototoError::new(format!("failed to run git {}: {err}", args.join(" ")))
        })?;
    if output.status.success() {
        tracing::info!(
            operation = "process.command",
            command = %command_label,
            cwd = %root.display(),
            status = output.status.code(),
            latency_ms = started.elapsed().as_millis(),
            "console outbound process call completed"
        );
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    tracing::warn!(
        operation = "process.command",
        command = %command_label,
        cwd = %root.display(),
        status = output.status.code(),
        latency_ms = started.elapsed().as_millis(),
        "console outbound process call returned non-zero status"
    );
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

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn changed_paths_include_tracked_and_untracked_package_files() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_file(
            &repo.path().join("packages/payments/rototo-package.toml"),
            "schema_version = 1\n",
        )
        .await;
        write_file(
            &repo
                .path()
                .join("packages/payments/variables/checkout.toml"),
            "schema_version = 1\n",
        )
        .await;
        write_file(
            &repo.path().join("packages/search/variables/query.toml"),
            "schema_version = 1\n",
        )
        .await;
        commit_all(repo.path(), "add packages");

        write_file(
            &repo
                .path()
                .join("packages/payments/variables/checkout.toml"),
            "schema_version = 1\ntype = \"bool\"\n",
        )
        .await;
        write_file(
            &repo.path().join("packages/payments/variables/new.toml"),
            "schema_version = 1\n",
        )
        .await;
        write_file(
            &repo.path().join("packages/search/variables/query.toml"),
            "schema_version = 1\ntype = \"string\"\n",
        )
        .await;

        let paths = changed_paths(repo.path(), "packages/payments")
            .await
            .unwrap();

        assert_eq!(
            paths,
            vec![
                "packages/payments/variables/checkout.toml".to_owned(),
                "packages/payments/variables/new.toml".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn file_at_head_reads_from_nested_package_root() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        let package = repo.path().join("packages/payments");
        write_file(
            &package.join("variables/checkout.toml"),
            "schema_version = 1\n",
        )
        .await;
        commit_all(repo.path(), "add checkout");
        write_file(
            &package.join("variables/checkout.toml"),
            "schema_version = 1\ntype = \"bool\"\n",
        )
        .await;

        let contents = file_at_head(&package, "variables/checkout.toml")
            .await
            .unwrap();
        let missing = file_at_head(&package, "variables/missing.toml")
            .await
            .unwrap();

        assert_eq!(contents.as_deref(), Some("schema_version = 1\n"));
        assert_eq!(missing, None);
    }

    async fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(path, contents).await.unwrap();
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
