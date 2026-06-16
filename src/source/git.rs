use std::path::Path;

use tempfile::TempDir;
use tokio::process::Command;

use crate::error::{Result, RototoError};

use super::path::select_subdir;
use super::types::{
    LoadedWorkspaceSource, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe,
    StagedSourceTree,
};
use super::uri::SourceUri;

pub(super) async fn stage_git_repo(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    let tree = stage_git_source_tree(uri, original, options).await?;
    let fingerprint = tree.fingerprint().cloned();
    let immutable = tree.immutable();
    Ok(LoadedWorkspaceSource {
        staged: tree.into_staged_workspace(),
        fingerprint: fingerprint.clone(),
        immutable,
        layers: vec![SourceLayer {
            source: original.to_owned(),
            fingerprint,
            immutable,
        }],
    })
}

pub(super) async fn stage_git_source_tree(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
) -> Result<StagedSourceTree> {
    let inner_scheme = uri
        .scheme
        .strip_prefix("git+")
        .ok_or_else(|| RototoError::new("invalid git workspace source"))?;
    if !matches!(inner_scheme, "file" | "https" | "ssh") {
        return Err(RototoError::new(format!(
            "git workspace source scheme is not supported: git+{inner_scheme}"
        )));
    }
    if let Some(ref_) = uri.ref_.as_deref() {
        validate_git_ref(ref_)?;
    }
    let clone_url = format!("{inner_scheme}://{}", uri.base);
    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let clone_dir = tempdir.path().join("clone");

    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command.arg("clone").arg("--quiet");
    let pinned_commit = uri.ref_.as_deref().is_some_and(is_full_git_commit);
    if !pinned_commit {
        command.arg("--depth=1");
    }
    if let Some(ref_) = &uri.ref_
        && !pinned_commit
    {
        command.arg("--branch").arg(ref_);
    }
    command.arg(&clone_url).arg(&clone_dir);
    scrub_git_process_variables(&mut command);

    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| {
            RototoError::new(format!(
                "git fetch timed out for workspace source: {original}"
            ))
        })?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git fetch failed for workspace source: {}",
            stderr.trim()
        )));
    }

    if let Some(ref_) = uri.ref_.as_deref()
        && pinned_commit
    {
        git_checkout(&clone_dir, ref_, options).await?;
    }
    let commit = git_rev_parse_head(&clone_dir, options).await?;
    let root = select_subdir(&clone_dir, uri.subdir.as_deref(), original).await?;
    Ok(StagedSourceTree::temporary(
        root,
        tempdir,
        Some(SourceFingerprint::GitCommit(commit)),
        pinned_commit,
    ))
}

pub(super) async fn probe_git_repo(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
    previous: Option<&SourceFingerprint>,
) -> Result<SourceProbe> {
    let Some(ref_) = uri.ref_.as_deref() else {
        return Ok(SourceProbe::Unknown);
    };
    if is_full_git_commit(ref_) {
        return Ok(SourceProbe::ImmutablePinned(SourceFingerprint::GitCommit(
            ref_.to_owned(),
        )));
    }
    let commit = git_ls_remote(uri, original, options).await?;
    let fingerprint = SourceFingerprint::GitCommit(commit);
    if previous == Some(&fingerprint) {
        Ok(SourceProbe::Unchanged)
    } else {
        Ok(SourceProbe::Changed(Some(fingerprint)))
    }
}

async fn git_rev_parse_head(repo: &Path, options: &SourceOptions) -> Result<String> {
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command.current_dir(repo).arg("rev-parse").arg("HEAD");
    scrub_git_process_variables(&mut command);
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new("git rev-parse timed out for workspace source"))?
        .map_err(|err| RototoError::new(format!("failed to run git: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git rev-parse failed for workspace source: {}",
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn git_checkout(repo: &Path, ref_: &str, options: &SourceOptions) -> Result<()> {
    validate_git_ref(ref_)?;
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command
        .current_dir(repo)
        .arg("checkout")
        .arg("--quiet")
        .arg(ref_);
    scrub_git_process_variables(&mut command);
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new("git checkout timed out for workspace source"))?
        .map_err(|err| RototoError::new(format!("failed to run git: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git checkout failed for workspace source: {}",
            stderr.trim()
        )));
    }
    Ok(())
}

async fn git_ls_remote(uri: &SourceUri, original: &str, options: &SourceOptions) -> Result<String> {
    let inner_scheme = uri
        .scheme
        .strip_prefix("git+")
        .ok_or_else(|| RototoError::new("invalid git workspace source"))?;
    let clone_url = format!("{inner_scheme}://{}", uri.base);
    let ref_ = uri
        .ref_
        .as_deref()
        .ok_or_else(|| RototoError::new("git workspace source has no ref"))?;
    validate_git_ref(ref_)?;
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command.arg("ls-remote").arg(&clone_url).arg("--").arg(ref_);
    scrub_git_process_variables(&mut command);
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| {
            RototoError::new(format!(
                "git check timed out for workspace source: {original}"
            ))
        })?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git check failed for workspace source: {}",
            stderr.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .ok_or_else(|| RototoError::new(format!("git ref `{ref_}` was not found in `{original}`")))
}

fn validate_git_ref(ref_: &str) -> Result<()> {
    if ref_.starts_with('-') {
        return Err(RototoError::new(format!(
            "git workspace ref must not begin with '-': {ref_}"
        )));
    }
    Ok(())
}

fn scrub_git_process_variables(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    for key in [
        "GIT_INDEX_FILE",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
}

fn is_full_git_commit(ref_: &str) -> bool {
    ref_.len() == 40 && ref_.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_git_commit_detection_requires_forty_hex_characters() {
        assert!(is_full_git_commit(
            "0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(!is_full_git_commit("main"));
        assert!(!is_full_git_commit(
            "0123456789abcdef0123456789abcdef0123456g"
        ));
    }
}
