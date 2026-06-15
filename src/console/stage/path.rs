use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::error::{Result, RototoError};

use super::types::ArtifactHandle;

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";

pub(super) async fn artifact_workspace_root(
    artifact: &Arc<ArtifactHandle>,
    subdir: Option<&str>,
) -> Result<PathBuf> {
    match subdir {
        Some(subdir) => select_artifact_subdir(&artifact.root, subdir).await,
        None => infer_artifact_workspace_root(&artifact.root).await,
    }
}

pub(super) fn relative_path_is_safe(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

async fn infer_artifact_workspace_root(root: &Path) -> Result<PathBuf> {
    if async_is_file(&root.join(WORKSPACE_MANIFEST)).await {
        return canonicalize(root).await;
    }
    if let Some(wrapper) = single_directory(root).await?
        && async_is_file(&wrapper.join(WORKSPACE_MANIFEST)).await
    {
        return canonicalize(&wrapper).await;
    }
    canonicalize(root).await
}

async fn select_artifact_subdir(root: &Path, subdir: &str) -> Result<PathBuf> {
    if !relative_path_is_safe(Path::new(subdir)) {
        return Err(RototoError::new(format!(
            "workspace source subdir is unsafe: {subdir}"
        )));
    }
    match select_subdir(root, subdir).await {
        Ok(path) => Ok(path),
        Err(err) => {
            if let Some(wrapper) = single_directory(root).await?
                && let Ok(path) = select_subdir(&wrapper, subdir).await
            {
                return Ok(path);
            }
            Err(err)
        }
    }
}

async fn select_subdir(root: &Path, subdir: &str) -> Result<PathBuf> {
    let canonical_root = canonicalize(root).await?;
    let target = root.join(subdir);
    let metadata = tokio::fs::metadata(&target).await.map_err(|_| {
        RototoError::new(format!(
            "workspace source subdir `{subdir}` was not found in staged artifact"
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "workspace source subdir `{subdir}` is not a directory"
        )));
    }
    let canonical_target = canonicalize(&target).await?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(RototoError::new(format!(
            "workspace source subdir `{subdir}` escapes staged artifact"
        )));
    }
    Ok(canonical_target)
}

async fn canonicalize(path: &Path) -> Result<PathBuf> {
    tokio::fs::canonicalize(path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to canonicalize staged path {}: {err}",
            path.display()
        ))
    })
}

async fn single_directory(root: &Path) -> Result<Option<PathBuf>> {
    let mut dirs = Vec::new();
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect staged artifact: {err}")))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect staged artifact: {err}")))?
    {
        let path = entry.path();
        if entry
            .metadata()
            .await
            .map_err(|err| RototoError::new(format!("failed to inspect staged artifact: {err}")))?
            .is_dir()
        {
            dirs.push(path);
        }
    }
    Ok((dirs.len() == 1).then(|| dirs.remove(0)))
}

async fn async_is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
}
