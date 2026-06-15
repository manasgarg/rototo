use std::path::{Component, Path, PathBuf};

use crate::error::{Result, RototoError};

pub(super) async fn select_subdir(
    root: &Path,
    subdir: Option<&str>,
    original: &str,
) -> Result<PathBuf> {
    let canonical_root = tokio::fs::canonicalize(root).await.map_err(|err| {
        RototoError::new(format!(
            "failed to canonicalize staged workspace root {}: {err}",
            root.display()
        ))
    })?;
    let Some(subdir) = subdir else {
        return Ok(canonical_root);
    };
    if !relative_path_is_safe(Path::new(subdir)) {
        return Err(RototoError::new(format!(
            "workspace source subdir is unsafe: {subdir}"
        )));
    }
    let target = root.join(subdir);
    let metadata = tokio::fs::metadata(&target).await.map_err(|_| {
        RototoError::new(format!(
            "workspace source subdir `{subdir}` was not found in `{original}`"
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "workspace source subdir `{subdir}` is not a directory"
        )));
    }
    let canonical_target = tokio::fs::canonicalize(&target).await.map_err(|err| {
        RototoError::new(format!(
            "failed to canonicalize workspace source subdir `{subdir}`: {err}"
        ))
    })?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(RototoError::new(format!(
            "workspace source subdir `{subdir}` escapes staged workspace"
        )));
    }
    Ok(canonical_target)
}

pub(super) async fn async_is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
}

pub(super) fn relative_path_is_safe(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}
