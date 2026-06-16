use std::path::{Path, PathBuf};

use crate::error::{Result, RototoError};

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
