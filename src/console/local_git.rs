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
