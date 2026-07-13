pub(crate) mod context;
pub(crate) mod diff;
pub(crate) mod docs;
pub(crate) mod fixtures;
pub(crate) mod init;
pub(crate) mod inspect;
pub(crate) mod lint;
pub(crate) mod package;
pub(crate) mod resolve;
pub(crate) mod selectors;
pub(crate) mod setup;

use std::path::Path;

use rototo::diagnostics::Severity;
use rototo::{
    Result, RototoError, SourceOptions, StagedPackage, find_package_root, stage_package_source,
};

pub(crate) async fn path_exists(path: &Path) -> Result<bool> {
    match tokio::fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(RototoError::new(format!(
            "failed to inspect {}: {err}",
            path.display()
        ))),
    }
}

pub(crate) async fn package_source_or_current(
    package: Option<String>,
    source_options: &SourceOptions,
) -> Result<StagedPackage> {
    match package {
        Some(package) => stage_package_source(package, source_options).await,
        None => {
            let current_dir = tokio::task::spawn_blocking(std::env::current_dir)
                .await
                .map_err(|err| RototoError::new(format!("current directory task failed: {err}")))?
                .map_err(|err| {
                    RototoError::new(format!("failed to read current directory: {err}"))
                })?;
            Ok(StagedPackage::local(find_package_root(&current_dir).await?))
        }
    }
}

/// Resolves an optional package source into a source string, falling back to
/// the package discovered from the current directory. Used by commands that need
/// the source as a string (to both load it and echo it back to the user) rather
/// than a [`StagedPackage`].
pub(crate) async fn package_source_string_or_current(package: Option<String>) -> Result<String> {
    match package {
        Some(package) => Ok(package),
        None => {
            let current_dir = tokio::task::spawn_blocking(std::env::current_dir)
                .await
                .map_err(|err| RototoError::new(format!("current directory task failed: {err}")))?
                .map_err(|err| {
                    RototoError::new(format!("failed to read current directory: {err}"))
                })?;
            Ok(find_package_root(&current_dir).await?.display().to_string())
        }
    }
}

pub(crate) fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}
