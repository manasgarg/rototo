use std::path::Path;

use tempfile::TempDir;

use crate::error::{Result, RototoError};

#[cfg(feature = "console")]
use super::types::StagedSourceTree;
use super::types::{LoadedWorkspaceSource, SourceLayer, StagedWorkspace};
use super::uri::SourceUri;

pub(super) async fn stage_local_path(path: &Path) -> Result<StagedWorkspace> {
    Ok(StagedWorkspace::local(path.to_path_buf()))
}

#[cfg(feature = "console")]
pub(super) async fn stage_local_tree(path: &Path) -> Result<StagedSourceTree> {
    Ok(StagedSourceTree::local(path.to_path_buf()))
}

pub(super) async fn snapshot_local_path(path: &Path) -> Result<LoadedWorkspaceSource> {
    let source_label = path.to_string_lossy().into_owned();
    let source = path.to_path_buf();
    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let target = tempdir.path().join("workspace");
    let target_for_task = target.clone();
    tokio::task::spawn_blocking(move || copy_dir_recursive(&source, &target_for_task))
        .await
        .map_err(|err| RototoError::new(format!("workspace snapshot task failed: {err}")))??;
    Ok(LoadedWorkspaceSource {
        staged: StagedWorkspace::temporary(target, tempdir),
        fingerprint: None,
        immutable: false,
        layers: vec![SourceLayer {
            source: source_label,
            fingerprint: None,
            immutable: false,
        }],
    })
}

pub(super) async fn stage_file_uri(uri: &SourceUri) -> Result<StagedWorkspace> {
    if uri.ref_.is_some() || uri.subdir.is_some() {
        return Err(RototoError::new(
            "file:// workspace sources do not support fragments",
        ));
    }
    stage_local_path(Path::new(&uri.base)).await
}

#[cfg(feature = "console")]
pub(super) async fn stage_file_uri_tree(uri: &SourceUri) -> Result<StagedSourceTree> {
    if uri.ref_.is_some() || uri.subdir.is_some() {
        return Err(RototoError::new(
            "file:// source trees do not support fragments",
        ));
    }
    stage_local_tree(Path::new(&uri.base)).await
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    let metadata = std::fs::metadata(source).map_err(|err| {
        RototoError::new(format!(
            "failed to inspect workspace {}: {err}",
            source.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "workspace source is not a directory: {}",
            source.display()
        )));
    }
    std::fs::create_dir_all(target).map_err(|err| {
        RototoError::new(format!(
            "failed to create workspace snapshot {}: {err}",
            target.display()
        ))
    })?;
    for entry in std::fs::read_dir(source).map_err(|err| {
        RototoError::new(format!(
            "failed to read workspace directory {}: {err}",
            source.display()
        ))
    })? {
        let entry = entry
            .map_err(|err| RototoError::new(format!("failed to read workspace entry: {err}")))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata().map_err(|err| {
            RototoError::new(format!(
                "failed to inspect workspace entry {}: {err}",
                source_path.display()
            ))
        })?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if metadata.is_file() {
            std::fs::copy(&source_path, &target_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to copy workspace entry {}: {err}",
                    source_path.display()
                ))
            })?;
        } else {
            return Err(RototoError::new(format!(
                "workspace snapshot contains unsupported entry type: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}
