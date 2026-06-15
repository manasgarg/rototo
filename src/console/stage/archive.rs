use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

use crate::error::{Result, RototoError};
use crate::source::{SourceAuth, SourceOptions};

use super::path::relative_path_is_safe;
use super::source::{auth, hex_digest};
use super::types::{ArtifactHandle, ArtifactKeepAlive, ArtifactRefresh};

const ERROR_BODY_PREVIEW_BYTES: u64 = 4096;

pub(super) async fn stage_archive_artifact(
    token: &str,
    url: &str,
    source: &str,
    identity: String,
    previous: Option<Arc<ArtifactHandle>>,
) -> Result<ArtifactRefresh> {
    let options = SourceOptions::default().with_auth(auth(token));
    if let Some(previous) = previous
        && let Some(fingerprint) = probe_archive_fingerprint(url, &options).await?
        && previous.fingerprint == fingerprint
    {
        return Ok(ArtifactRefresh::Unchanged(previous));
    }

    let client = reqwest::Client::builder()
        .timeout(options.http_timeout())
        .user_agent(concat!("rototo/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| {
            RototoError::new(format!(
                "failed to configure workspace archive fetch: {err}"
            ))
        })?;
    let mut request = client.get(url);
    if let SourceAuth::Bearer(token) = options.auth() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|err| RototoError::new(format!("failed to fetch workspace archive: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        let preview = response_preview(response, ERROR_BODY_PREVIEW_BYTES).await?;
        let detail = if preview.is_empty() {
            String::new()
        } else {
            format!(": {preview}")
        };
        return Err(RototoError::new(format!(
            "failed to fetch workspace archive: HTTP {status}{detail}"
        )));
    }
    if let Some(length) = response.content_length()
        && length > options.max_archive_bytes()
    {
        return Err(RototoError::new(format!(
            "workspace archive is too large: {length} bytes exceeds limit of {} bytes",
            options.max_archive_bytes()
        )));
    }
    let header_fingerprint = response_fingerprint(&response);
    let tempdir = Arc::new(
        TempDir::new()
            .map_err(|err| RototoError::new(format!("failed to create archive staging: {err}")))?,
    );
    let archive_path = tempdir.path().join("workspace.tar.gz");
    write_response_to_file(response, &archive_path, options.max_archive_bytes()).await?;
    let fingerprint = match header_fingerprint {
        Some(fingerprint) => fingerprint,
        None => content_hash_fingerprint(&archive_path).await?,
    };
    let root = tempdir.path().join("extract");
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|err| RototoError::new(format!("failed to create extraction directory: {err}")))?;

    let root_for_task = root.clone();
    let max_decompressed_bytes = options.max_decompressed_archive_bytes();
    let max_entries = options.max_archive_entries();
    tokio::task::spawn_blocking(move || {
        extract_archive(
            &archive_path,
            &root_for_task,
            max_decompressed_bytes,
            max_entries,
        )
    })
    .await
    .map_err(|err| RototoError::new(format!("archive extraction task failed: {err}")))??;
    tracing::debug!(source = %source, "console workspace archive extracted");

    Ok(ArtifactRefresh::Changed(Arc::new(ArtifactHandle {
        identity,
        root,
        fingerprint,
        immutable: false,
        _keep_alive: ArtifactKeepAlive::Archive { _tempdir: tempdir },
    })))
}

async fn probe_archive_fingerprint(url: &str, options: &SourceOptions) -> Result<Option<String>> {
    let client = reqwest::Client::builder()
        .timeout(options.http_timeout())
        .user_agent(concat!("rototo/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| {
            RototoError::new(format!(
                "failed to configure workspace archive check: {err}"
            ))
        })?;
    let mut request = client.head(url);
    if let SourceAuth::Bearer(token) = options.auth() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|err| RototoError::new(format!("failed to check workspace archive: {err}")))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    Ok(response_fingerprint(&response))
}

async fn write_response_to_file(
    mut response: reqwest::Response,
    path: &Path,
    max_bytes: u64,
) -> Result<()> {
    let mut file = tokio::fs::File::create(path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to create workspace archive {}: {err}",
            path.display()
        ))
    })?;
    let mut total = 0_u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| RototoError::new(format!("failed to read workspace archive: {err}")))?
    {
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| RototoError::new("workspace archive is too large"))?;
        if total > max_bytes {
            return Err(RototoError::new(format!(
                "workspace archive is too large: exceeded limit of {max_bytes} bytes"
            )));
        }
        file.write_all(&chunk)
            .await
            .map_err(|err| RototoError::new(format!("failed to write workspace archive: {err}")))?;
    }
    file.flush()
        .await
        .map_err(|err| RototoError::new(format!("failed to write workspace archive: {err}")))?;
    Ok(())
}

async fn response_preview(mut response: reqwest::Response, max_bytes: u64) -> Result<String> {
    let mut body = Vec::new();
    while (body.len() as u64) < max_bytes {
        let Some(chunk) = response
            .chunk()
            .await
            .map_err(|err| RototoError::new(format!("failed to read error response: {err}")))?
        else {
            break;
        };
        let remaining = (max_bytes as usize).saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    Ok(String::from_utf8_lossy(&body).trim().to_owned())
}

fn extract_archive(
    archive_path: &Path,
    extract_dir: &Path,
    max_decompressed_bytes: u64,
    max_entries: u64,
) -> Result<()> {
    let file = std::fs::File::open(archive_path).map_err(|err| {
        RototoError::new(format!(
            "failed to open workspace archive {}: {err}",
            archive_path.display()
        ))
    })?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut entry_count = 0_u64;
    let mut decompressed_bytes = 0_u64;
    for entry in archive
        .entries()
        .map_err(|err| RototoError::new(format!("failed to read workspace archive: {err}")))?
    {
        let mut entry = entry
            .map_err(|err| RototoError::new(format!("failed to read archive entry: {err}")))?;
        let entry_type = entry.header().entry_type();
        entry_count = entry_count
            .checked_add(1)
            .ok_or_else(|| RototoError::new("workspace archive contains too many entries"))?;
        if entry_count > max_entries {
            return Err(RototoError::new(format!(
                "workspace archive contains too many entries: exceeded limit of {max_entries}"
            )));
        }
        if archive_skipped_entry(entry_type) {
            continue;
        }
        let path = entry
            .path()
            .map_err(|err| RototoError::new(format!("archive entry path is invalid: {err}")))?;
        if !relative_path_is_safe(&path) {
            return Err(RototoError::new(format!(
                "workspace archive contains unsafe path: {}",
                path.display()
            )));
        }
        if !(entry_type.is_file() || entry_type.is_dir()) {
            return Err(RototoError::new(format!(
                "workspace archive contains unsupported entry type at: {}",
                path.display()
            )));
        }
        if entry_type.is_file() {
            decompressed_bytes = decompressed_bytes
                .checked_add(entry.header().size().map_err(|err| {
                    RototoError::new(format!("archive entry size is invalid: {err}"))
                })?)
                .ok_or_else(|| RototoError::new("workspace archive is too large"))?;
            if decompressed_bytes > max_decompressed_bytes {
                return Err(RototoError::new(format!(
                    "workspace archive decompressed content is too large: exceeded limit of {max_decompressed_bytes} bytes"
                )));
            }
        }
        entry.unpack_in(extract_dir).map_err(|err| {
            RototoError::new(format!("failed to extract workspace archive: {err}"))
        })?;
    }
    Ok(())
}

fn archive_skipped_entry(entry_type: tar::EntryType) -> bool {
    entry_type.is_pax_global_extensions()
        || entry_type.is_pax_local_extensions()
        || entry_type.is_gnu_longname()
        || entry_type.is_gnu_longlink()
        || entry_type.is_symlink()
        || entry_type.is_hard_link()
}

fn response_fingerprint(response: &reqwest::Response) -> Option<String> {
    if let Some(etag) = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
    {
        return Some(format!("etag:{etag}"));
    }
    if let Some(last_modified) = response
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
    {
        return Some(format!("last-modified:{last_modified}"));
    }
    None
}

async fn content_hash_fingerprint(path: &Path) -> Result<String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|err| RototoError::new(format!("failed to read workspace archive: {err}")))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    Ok(format!("sha256:{}", hex_digest(digest.as_ref())))
}
