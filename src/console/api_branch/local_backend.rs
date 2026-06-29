use std::path::{Path as FsPath, PathBuf};

use crate::console::api::{ApiError, ApiResult, ConsoleState};
use crate::console::local_git;
use crate::console::store::PackageRecord;

pub(super) async fn local_source_root(
    state: &ConsoleState,
    package: &PackageRecord,
) -> ApiResult<PathBuf> {
    let source = state
        .fixed_package_source
        .as_deref()
        .unwrap_or(&package.source);
    let root =
        local_git::package_root(source).map_err(|err| ApiError::bad_request(err.to_string()))?;
    tokio::fs::canonicalize(&root).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to resolve local package source {}: {err}",
            root.display()
        ))
    })
}

pub(super) fn local_package_scope(state: &ConsoleState, package: &PackageRecord) -> String {
    if state.fixed_package_source.is_some() {
        package.path.clone()
    } else {
        ".".to_owned()
    }
}

pub(super) fn local_relative_path(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<String> {
    let relative = if state.fixed_package_source.is_some() || package.path == "." {
        file_path.trim()
    } else {
        file_path
            .strip_prefix(&format!("{}/", package.path))
            .ok_or_else(|| ApiError::bad_request("file path does not belong to package"))?
    };
    if relative.is_empty()
        || relative.starts_with('/')
        || relative
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ApiError::bad_request("file path is not valid"));
    }
    Ok(relative.to_owned())
}

async fn local_existing_file_path(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<PathBuf> {
    let root = local_source_root(state, package).await?;
    let relative = local_relative_path(state, package, file_path)?;
    let path = root.join(relative);
    let canonical = tokio::fs::canonicalize(&path).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ApiError::not_found(format!("file not found: {file_path}"))
        } else {
            ApiError::bad_request(format!("failed to resolve {file_path}: {err}"))
        }
    })?;
    if !canonical.starts_with(&root) {
        return Err(ApiError::bad_request(
            "file path escapes the local package source",
        ));
    }
    Ok(canonical)
}

async fn local_writable_file_path(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<PathBuf> {
    let root = local_source_root(state, package).await?;
    let relative = local_relative_path(state, package, file_path)?;
    ensure_local_parent_dir(&root, &relative).await?;
    let path = root.join(relative);
    if let Ok(metadata) = tokio::fs::symlink_metadata(&path).await
        && metadata.file_type().is_symlink()
    {
        return Err(ApiError::bad_request(
            "local package edits do not follow symlink files",
        ));
    }
    Ok(path)
}

pub(super) async fn ensure_local_parent_dir(root: &FsPath, relative: &str) -> ApiResult<()> {
    let parent = FsPath::new(relative)
        .parent()
        .ok_or_else(|| ApiError::bad_request("file path must have a parent directory"))?;
    let mut current = root.to_path_buf();
    for component in parent.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(ApiError::bad_request("file path is not valid"));
        };
        current.push(segment);
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ApiError::bad_request(
                        "local package edits do not follow symlink directories",
                    ));
                }
                if !metadata.is_dir() {
                    return Err(ApiError::bad_request(format!(
                        "local package path is not a directory: {}",
                        current.display()
                    )));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&current).await.map_err(|err| {
                    ApiError::bad_request(format!(
                        "failed to create parent directory {}: {err}",
                        current.display()
                    ))
                })?;
            }
            Err(err) => {
                return Err(ApiError::bad_request(format!(
                    "failed to resolve parent directory {}: {err}",
                    current.display()
                )));
            }
        }
    }
    let canonical_parent = tokio::fs::canonicalize(&current).await.map_err(|err| {
        ApiError::bad_request(format!("failed to resolve parent directory: {err}"))
    })?;
    if !canonical_parent.starts_with(root) {
        return Err(ApiError::bad_request(
            "file path escapes the local package source",
        ));
    }
    Ok(())
}

pub(super) async fn local_file_exists(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<bool> {
    match local_existing_file_path(state, package, file_path).await {
        Ok(path) => Ok(path.is_file()),
        Err(err) if err.status == axum::http::StatusCode::NOT_FOUND => Ok(false),
        Err(err) => Err(err),
    }
}

pub(super) async fn read_local_file(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<String> {
    let path = local_existing_file_path(state, package, file_path).await?;
    tokio::fs::read_to_string(&path).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to read local file {}: {err}",
            path.display()
        ))
    })
}

pub(super) async fn write_local_file(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
    content: &str,
) -> ApiResult<()> {
    let path = local_writable_file_path(state, package, file_path).await?;
    tokio::fs::write(&path, content).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to write local file {}: {err}",
            path.display()
        ))
    })
}

pub(super) async fn delete_local_file(
    state: &ConsoleState,
    package: &PackageRecord,
    file_path: &str,
) -> ApiResult<()> {
    let path = local_existing_file_path(state, package, file_path).await?;
    tokio::fs::remove_file(&path).await.map_err(|err| {
        ApiError::bad_request(format!(
            "failed to delete local file {}: {err}",
            path.display()
        ))
    })
}
