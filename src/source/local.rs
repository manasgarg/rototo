use std::path::Path;

use tempfile::TempDir;

use crate::error::{Result, RototoError};

use super::types::{LoadedPackageSource, SourceLayer, StagedPackage};
use super::uri::SourceUri;

pub(super) async fn stage_local_path(path: &Path) -> Result<StagedPackage> {
    Ok(StagedPackage::local(path.to_path_buf()))
}

pub(super) async fn snapshot_local_path(path: &Path) -> Result<LoadedPackageSource> {
    let source_label = path.to_string_lossy().into_owned();
    let source = path.to_path_buf();
    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let target = tempdir.path().join("package");
    let target_for_task = target.clone();
    tokio::task::spawn_blocking(move || copy_dir_recursive(&source, &target_for_task))
        .await
        .map_err(|err| RototoError::new(format!("package snapshot task failed: {err}")))??;
    Ok(LoadedPackageSource {
        staged: StagedPackage::temporary(target, tempdir),
        fingerprint: None,
        immutable: false,
        layers: vec![SourceLayer {
            source: source_label,
            fingerprint: None,
            immutable: false,
        }],
    })
}

pub(super) async fn stage_file_uri(uri: &SourceUri) -> Result<StagedPackage> {
    if uri.ref_.is_some() || uri.subdir.is_some() {
        return Err(RototoError::new(
            "file:// package sources do not support fragments",
        ));
    }
    stage_local_path(Path::new(&uri.base)).await
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    let metadata = std::fs::metadata(source).map_err(|err| {
        RototoError::new(format!(
            "failed to inspect package {}: {err}",
            source.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "package source is not a directory: {}",
            source.display()
        )));
    }
    std::fs::create_dir_all(target).map_err(|err| {
        RototoError::new(format!(
            "failed to create package snapshot {}: {err}",
            target.display()
        ))
    })?;
    for entry in std::fs::read_dir(source).map_err(|err| {
        RototoError::new(format!(
            "failed to read package directory {}: {err}",
            source.display()
        ))
    })? {
        let entry = entry
            .map_err(|err| RototoError::new(format!("failed to read package entry: {err}")))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata().map_err(|err| {
            RototoError::new(format!(
                "failed to inspect package entry {}: {err}",
                source_path.display()
            ))
        })?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if metadata.is_file() {
            std::fs::copy(&source_path, &target_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to copy package entry {}: {err}",
                    source_path.display()
                ))
            })?;
        } else {
            return Err(RototoError::new(format!(
                "package snapshot contains unsupported entry type: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}
