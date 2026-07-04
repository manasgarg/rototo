use std::path::Path;

use crate::error::{Result, RototoError};

#[cfg(feature = "console")]
use super::archive::stage_https_archive_tree;
use super::archive::{probe_https_archive, stage_https_archive};
#[cfg(feature = "console")]
use super::git::stage_git_source_tree;
use super::git::{probe_git_repo, stage_git_repo};
use super::layer::load_package_source_graph;
use super::local::{snapshot_local_path, stage_file_uri, stage_local_path};
#[cfg(feature = "console")]
use super::local::{stage_file_uri_tree, stage_local_tree};
#[cfg(feature = "console")]
use super::types::StagedSourceTree;
use super::types::{
    LoadedPackageSource, LocalStageMode, SourceFingerprint, SourceLayer, SourceOptions,
    SourceProbe, StagedPackage,
};
use super::uri::SourceUri;

pub async fn stage_package_source(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<StagedPackage> {
    Ok(load_package_source(source, options).await?.into_staged())
}

#[cfg(feature = "console")]
pub(crate) async fn stage_source_tree(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<StagedSourceTree> {
    let source = source.as_ref();
    match SourceUri::parse(source)? {
        None => stage_local_tree(Path::new(source)).await,
        Some(uri) => match uri.scheme.as_str() {
            "file" => stage_file_uri_tree(&uri).await,
            "https" => stage_https_archive_tree(&uri, source, options).await,
            "http" => Err(RototoError::new(
                "http:// source trees are not supported; use https://",
            )),
            scheme if scheme.starts_with("git+") => {
                stage_git_source_tree(&uri, source, options).await
            }
            scheme => Err(RototoError::new(format!(
                "source tree scheme is not supported: {scheme}"
            ))),
        },
    }
}

pub async fn load_package_source(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedPackageSource> {
    let mut stack = Vec::new();
    load_package_source_graph(
        source.as_ref(),
        options,
        LocalStageMode::Borrow,
        None,
        &mut stack,
    )
    .await
}

pub(super) async fn load_single_package_source(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedPackageSource> {
    let source = source.as_ref();
    match SourceUri::parse(source)? {
        None => Ok(loaded_single_layer(
            source,
            stage_local_path(Path::new(source)).await?,
            None,
            false,
        )),
        Some(uri) => match uri.scheme.as_str() {
            "file" => Ok(loaded_single_layer(
                source,
                stage_file_uri(&uri).await?,
                None,
                false,
            )),
            "https" => stage_https_archive(&uri, source, options).await,
            "http" => Err(RototoError::new(
                "http:// package sources are not supported; use https://",
            )),
            scheme if scheme.starts_with("git+") => stage_git_repo(&uri, source, options).await,
            scheme => Err(RototoError::new(format!(
                "package source scheme is not supported: {scheme}"
            ))),
        },
    }
}

pub async fn load_package_source_snapshot(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedPackageSource> {
    let mut stack = Vec::new();
    load_package_source_graph(
        source.as_ref(),
        options,
        LocalStageMode::Snapshot,
        None,
        &mut stack,
    )
    .await
}

pub(super) async fn load_single_package_source_snapshot(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedPackageSource> {
    let source = source.as_ref();
    match SourceUri::parse(source)? {
        None => snapshot_local_path(Path::new(source)).await,
        Some(uri) if uri.scheme == "file" => {
            if uri.ref_.is_some() || uri.subdir.is_some() {
                return Err(RototoError::new(
                    "file:// package sources do not support fragments",
                ));
            }
            snapshot_local_path(Path::new(&uri.base)).await
        }
        _ => load_single_package_source(source, options).await,
    }
}

pub async fn probe_package_source(
    source: impl AsRef<str>,
    options: &SourceOptions,
    previous: Option<&SourceFingerprint>,
) -> Result<SourceProbe> {
    let source = source.as_ref();
    let Some(uri) = SourceUri::parse(source)? else {
        return Ok(SourceProbe::Unknown);
    };
    match uri.scheme.as_str() {
        "file" => Ok(SourceProbe::Unknown),
        "https" => probe_https_archive(&uri, options, previous).await,
        "http" => Err(RototoError::new(
            "http:// package sources are not supported; use https://",
        )),
        scheme if scheme.starts_with("git+") => {
            probe_git_repo(&uri, source, options, previous).await
        }
        scheme => Err(RototoError::new(format!(
            "package source scheme is not supported: {scheme}"
        ))),
    }
}

fn loaded_single_layer(
    source: &str,
    staged: StagedPackage,
    fingerprint: Option<SourceFingerprint>,
    immutable: bool,
) -> LoadedPackageSource {
    LoadedPackageSource {
        staged,
        fingerprint: fingerprint.clone(),
        immutable,
        layers: vec![SourceLayer {
            source: source.to_owned(),
            fingerprint,
            immutable,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stage_package_source_rejects_http() {
        let err = stage_package_source(
            "http://example.com/package.tar.gz",
            &SourceOptions::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "http:// package sources are not supported; use https://"
        );
    }

    #[tokio::test]
    async fn stage_package_source_rejects_git_http() {
        let err = stage_package_source(
            "git+http://example.com/package.git",
            &SourceOptions::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "git package source scheme is not supported: git+http"
        );
    }

    #[tokio::test]
    async fn stage_package_source_rejects_leading_dash_git_refs_before_running_git() {
        let err = stage_package_source(
            "git+file://example.com/package.git#--upload-pack=/tmp/evil",
            &SourceOptions::default(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("must not begin with '-'"));
    }
}

#[cfg(test)]
mod fragment_tests {
    use super::*;

    #[tokio::test]
    async fn file_sources_reject_fragments() {
        let err = stage_package_source("file:///tmp/package#main", &SourceOptions::new())
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("file:// package sources do not support fragments")
        );

        let err = stage_package_source("file:///tmp/package#:subdir", &SourceOptions::new())
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("file:// package sources do not support fragments")
        );
    }
}
