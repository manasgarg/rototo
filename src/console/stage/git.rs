use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio::process::Command;

use crate::error::{Result, RototoError};

use super::types::{ArtifactHandle, ArtifactKeepAlive, ArtifactRefresh, GitRepoStore};

pub(super) async fn stage_git_artifact(
    repo: Arc<GitRepoStore>,
    token: &str,
    remote: &str,
    ref_: Option<&str>,
    identity: String,
    previous: Option<Arc<ArtifactHandle>>,
) -> Result<ArtifactRefresh> {
    let _guard = repo.lock.lock().await;
    let (commit, immutable) = resolve_git_commit(remote, ref_, token).await?;
    if let Some(previous) = previous
        && previous.fingerprint == commit
    {
        return Ok(ArtifactRefresh::Unchanged(previous));
    }

    let refspec = ref_.unwrap_or("HEAD");
    let mut fetch = Command::new("git");
    fetch
        .arg("--git-dir")
        .arg(&repo.bare_dir)
        .arg("fetch")
        .arg("--depth=1")
        .arg(remote)
        .arg(refspec);
    run_authenticated_git(fetch, token).await?;

    let checkout = Arc::new(
        TempDir::new()
            .map_err(|err| RototoError::new(format!("failed to create git checkout: {err}")))?,
    );
    let root = checkout.path().join("checkout");
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|err| RototoError::new(format!("failed to create git checkout: {err}")))?;
    let mut checkout_cmd = Command::new("git");
    checkout_cmd
        .arg("--git-dir")
        .arg(&repo.bare_dir)
        .arg("--work-tree")
        .arg(&root)
        .arg("checkout")
        .arg("--quiet")
        .arg("--force")
        .arg(&commit)
        .arg("--")
        .arg(".");
    run_git(&mut checkout_cmd, Duration::from_secs(60)).await?;

    Ok(ArtifactRefresh::Changed(Arc::new(ArtifactHandle {
        identity,
        root,
        fingerprint: commit,
        immutable,
        _keep_alive: ArtifactKeepAlive::Git {
            _repo: repo.clone(),
            _checkout: checkout,
        },
    })))
}

async fn resolve_git_commit(
    remote: &str,
    ref_: Option<&str>,
    token: &str,
) -> Result<(String, bool)> {
    if let Some(ref_) = ref_
        && is_full_git_commit(ref_)
    {
        return Ok((ref_.to_owned(), true));
    }
    let ref_ = ref_.unwrap_or("HEAD");
    validate_git_ref(ref_)?;
    let mut command = Command::new("git");
    command.arg("ls-remote").arg(remote).arg("--").arg(ref_);
    let stdout = run_authenticated_git(command, token).await?;
    let commit = stdout
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .ok_or_else(|| RototoError::new(format!("git ref `{ref_}` was not found in `{remote}`")))?;
    Ok((commit.to_owned(), false))
}

async fn run_authenticated_git(mut command: Command, token: &str) -> Result<String> {
    scrub_git_process_variables(&mut command);
    if token.is_empty() {
        return run_git_prepared(&mut command, Duration::from_secs(60)).await;
    }
    let askpass = TempDir::new().map_err(|err| {
        RototoError::new(format!("failed to create git credential helper: {err}"))
    })?;
    let script = askpass.path().join("askpass.sh");
    tokio::fs::write(
        &script,
        "#!/bin/sh\ncase \"$1\" in\n*Username*) printf '%s\\n' x-access-token ;;\n*) printf '%s\\n' \"$ROTOTO_GIT_PASSWORD\" ;;\nesac\n",
    )
    .await
    .map_err(|err| RototoError::new(format!("failed to write git credential helper: {err}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = tokio::fs::metadata(&script)
            .await
            .map_err(|err| {
                RototoError::new(format!("failed to inspect git credential helper: {err}"))
            })?
            .permissions();
        permissions.set_mode(0o700);
        tokio::fs::set_permissions(&script, permissions)
            .await
            .map_err(|err| {
                RototoError::new(format!("failed to secure git credential helper: {err}"))
            })?;
    }
    command
        .env("GIT_ASKPASS", &script)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("ROTOTO_GIT_PASSWORD", token);
    run_git_prepared(&mut command, Duration::from_secs(60)).await
}

pub(super) async fn run_git(command: &mut Command, timeout: Duration) -> Result<String> {
    scrub_git_process_variables(command);
    run_git_prepared(command, timeout).await
}

async fn run_git_prepared(command: &mut Command, timeout: Duration) -> Result<String> {
    command.kill_on_drop(true);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| RototoError::new("git command timed out while staging workspace"))?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(RototoError::new(format!(
        "git command failed while staging workspace: {}",
        if stderr.is_empty() {
            output.status.to_string()
        } else {
            stderr
        }
    )))
}

fn scrub_git_process_variables(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
}

fn validate_git_ref(ref_: &str) -> Result<()> {
    if ref_.starts_with('-') {
        return Err(RototoError::new(format!(
            "git workspace ref must not begin with '-': {ref_}"
        )));
    }
    Ok(())
}

fn is_full_git_commit(ref_: &str) -> bool {
    ref_.len() == 40 && ref_.chars().all(|c| c.is_ascii_hexdigit())
}
