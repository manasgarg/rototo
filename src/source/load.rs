use std::path::Path;

use crate::error::{Result, RototoError};

use super::archive::{probe_https_archive, stage_https_archive};
use super::git::{probe_git_repo, stage_git_repo};
use super::layer::load_workspace_source_graph;
use super::local::{snapshot_local_path, stage_file_uri, stage_local_path};
use super::types::{
    LoadedWorkspaceSource, LocalStageMode, SourceFingerprint, SourceLayer, SourceOptions,
    SourceProbe, StagedWorkspace,
};
use super::uri::SourceUri;

pub async fn stage_workspace_source(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<StagedWorkspace> {
    Ok(load_workspace_source(source, options).await?.into_staged())
}

pub async fn load_workspace_source(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    let mut stack = Vec::new();
    load_workspace_source_graph(
        source.as_ref(),
        options,
        LocalStageMode::Borrow,
        None,
        &mut stack,
    )
    .await
}

pub(super) async fn load_single_workspace_source(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
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
                "http:// workspace sources are not supported; use https://",
            )),
            scheme if scheme.starts_with("git+") => stage_git_repo(&uri, source, options).await,
            scheme => Err(RototoError::new(format!(
                "workspace source scheme is not supported: {scheme}"
            ))),
        },
    }
}

pub async fn load_workspace_source_snapshot(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    let mut stack = Vec::new();
    load_workspace_source_graph(
        source.as_ref(),
        options,
        LocalStageMode::Snapshot,
        None,
        &mut stack,
    )
    .await
}

pub(super) async fn load_single_workspace_source_snapshot(
    source: impl AsRef<str>,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    let source = source.as_ref();
    match SourceUri::parse(source)? {
        None => snapshot_local_path(Path::new(source)).await,
        Some(uri) if uri.scheme == "file" => {
            if uri.ref_.is_some() || uri.subdir.is_some() {
                return Err(RototoError::new(
                    "file:// workspace sources do not support fragments",
                ));
            }
            snapshot_local_path(Path::new(&uri.base)).await
        }
        _ => load_single_workspace_source(source, options).await,
    }
}

pub async fn probe_workspace_source(
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
            "http:// workspace sources are not supported; use https://",
        )),
        scheme if scheme.starts_with("git+") => {
            probe_git_repo(&uri, source, options, previous).await
        }
        scheme => Err(RototoError::new(format!(
            "workspace source scheme is not supported: {scheme}"
        ))),
    }
}

fn loaded_single_layer(
    source: &str,
    staged: StagedWorkspace,
    fingerprint: Option<SourceFingerprint>,
    immutable: bool,
) -> LoadedWorkspaceSource {
    LoadedWorkspaceSource {
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
    async fn stage_workspace_source_rejects_http() {
        let err = stage_workspace_source(
            "http://example.com/workspace.tar.gz",
            &SourceOptions::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "http:// workspace sources are not supported; use https://"
        );
    }

    #[tokio::test]
    async fn stage_workspace_source_rejects_git_http() {
        let err = stage_workspace_source(
            "git+http://example.com/workspace.git",
            &SourceOptions::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "git workspace source scheme is not supported: git+http"
        );
    }

    #[tokio::test]
    async fn stage_workspace_source_rejects_leading_dash_git_refs_before_running_git() {
        let err = stage_workspace_source(
            "git+file://example.com/workspace.git#--upload-pack=/tmp/evil",
            &SourceOptions::default(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("must not begin with '-'"));
    }
}
