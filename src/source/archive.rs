use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

use crate::error::{Result, RototoError};

use super::WORKSPACE_MANIFEST;
use super::path::{async_is_file, relative_path_is_safe, select_subdir};
#[cfg(feature = "console")]
use super::types::StagedSourceTree;
use super::types::{
    LoadedWorkspaceSource, SourceAuth, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe,
    StagedWorkspace,
};
use super::uri::SourceUri;

const ERROR_BODY_PREVIEW_BYTES: u64 = 4096;
const HTTP_USER_AGENT: &str = concat!("rototo/", env!("CARGO_PKG_VERSION"));

pub(super) async fn stage_https_archive(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    if uri.ref_.is_some() {
        return Err(RototoError::new(
            "https workspace sources only support #:subdir fragments",
        ));
    }

    let ExtractedArchive {
        extract_dir,
        fingerprint,
        tempdir,
    } = extract_https_archive(uri, original, options).await?;
    let root = match uri.subdir.as_deref() {
        Some(subdir) => select_archive_subdir(&extract_dir, subdir, original).await?,
        None => infer_archive_workspace_root(&extract_dir, original).await?,
    };
    Ok(LoadedWorkspaceSource {
        staged: StagedWorkspace::temporary(root, tempdir),
        fingerprint: fingerprint.clone(),
        immutable: false,
        layers: vec![SourceLayer {
            source: original.to_owned(),
            fingerprint,
            immutable: false,
        }],
    })
}

#[cfg(feature = "console")]
pub(super) async fn stage_https_archive_tree(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
) -> Result<StagedSourceTree> {
    if uri.ref_.is_some() {
        return Err(RototoError::new(
            "https source trees only support #:subdir fragments",
        ));
    }

    let ExtractedArchive {
        extract_dir,
        fingerprint,
        tempdir,
    } = extract_https_archive(uri, original, options).await?;
    let root = match uri.subdir.as_deref() {
        Some(subdir) => select_archive_subdir(&extract_dir, subdir, original).await?,
        None => extract_dir,
    };
    Ok(StagedSourceTree::temporary(
        root,
        tempdir,
        fingerprint,
        false,
    ))
}

struct ExtractedArchive {
    extract_dir: PathBuf,
    fingerprint: Option<SourceFingerprint>,
    tempdir: TempDir,
}

async fn extract_https_archive(
    uri: &SourceUri,
    _original: &str,
    options: &SourceOptions,
) -> Result<ExtractedArchive> {
    let url = format!("{}://{}", uri.scheme, uri.base);
    let client = https_archive_client(options)?;
    let mut request = client.get(&url);
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
    let fingerprint = response_fingerprint(&response);
    if let Some(length) = response.content_length()
        && length > options.max_archive_bytes()
    {
        return Err(RototoError::new(format!(
            "workspace archive is too large: {length} bytes exceeds limit of {} bytes",
            options.max_archive_bytes()
        )));
    }

    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let archive_path = tempdir.path().join("workspace.tar.gz");
    write_response_to_file(response, &archive_path, options.max_archive_bytes()).await?;
    let fingerprint = match fingerprint {
        Some(fingerprint) => Some(fingerprint),
        None => Some(content_hash_fingerprint(&archive_path).await?),
    };
    let extract_dir = tempdir.path().join("extract");
    tokio::fs::create_dir_all(&extract_dir)
        .await
        .map_err(|err| RototoError::new(format!("failed to create extraction directory: {err}")))?;

    let extract_dir_for_task = extract_dir.clone();
    let max_decompressed_bytes = options.max_decompressed_archive_bytes();
    let max_entries = options.max_archive_entries();
    tokio::task::spawn_blocking(move || {
        extract_archive(
            &archive_path,
            &extract_dir_for_task,
            max_decompressed_bytes,
            max_entries,
        )
    })
    .await
    .map_err(|err| RototoError::new(format!("archive extraction task failed: {err}")))??;

    Ok(ExtractedArchive {
        extract_dir,
        fingerprint,
        tempdir,
    })
}

pub(super) async fn probe_https_archive(
    uri: &SourceUri,
    options: &SourceOptions,
    previous: Option<&SourceFingerprint>,
) -> Result<SourceProbe> {
    if uri.ref_.is_some() {
        return Err(RototoError::new(
            "https workspace sources only support #:subdir fragments",
        ));
    }
    let url = format!("{}://{}", uri.scheme, uri.base);
    let client = https_archive_client(options)?;
    let mut request = client.head(&url);
    if let SourceAuth::Bearer(token) = options.auth() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|err| RototoError::new(format!("failed to check workspace archive: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(RototoError::new(format!(
            "failed to check workspace archive: HTTP {status}"
        )));
    }
    let Some(fingerprint) = response_fingerprint(&response) else {
        return Ok(SourceProbe::Unknown);
    };
    if previous == Some(&fingerprint) {
        Ok(SourceProbe::Unchanged)
    } else {
        Ok(SourceProbe::Changed(Some(fingerprint)))
    }
}

fn https_archive_client(options: &SourceOptions) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(options.http_timeout())
        .redirect(https_only_redirect_policy())
        .user_agent(HTTP_USER_AGENT)
        .build()
        .map_err(|err| RototoError::new(format!("failed to build HTTP client: {err}")))
}

fn https_only_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > 10 {
            return attempt.error("too many redirects");
        }
        if attempt.url().scheme() != "https" {
            return attempt.error("workspace archive redirects must stay on https");
        }
        attempt.follow()
    })
}

fn response_fingerprint(response: &reqwest::Response) -> Option<SourceFingerprint> {
    http_validator_fingerprint(response.headers())
}

fn http_validator_fingerprint(headers: &reqwest::header::HeaderMap) -> Option<SourceFingerprint> {
    if let Some(etag) = headers
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
    {
        return Some(SourceFingerprint::HttpValidator(format!("etag:{etag}")));
    }
    headers
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(|last_modified| {
            SourceFingerprint::HttpValidator(format!("last-modified:{last_modified}"))
        })
}

async fn content_hash_fingerprint(path: &Path) -> Result<SourceFingerprint> {
    let bytes = tokio::fs::read(path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to read workspace archive {}: {err}",
            path.display()
        ))
    })?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    Ok(SourceFingerprint::ContentHash(format!(
        "sha256:{}",
        hex_digest(digest.as_ref())
    )))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
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
        if !archive_path_is_safe(&path) {
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

async fn infer_archive_workspace_root(extract_dir: &Path, original: &str) -> Result<PathBuf> {
    if async_is_file(&extract_dir.join(WORKSPACE_MANIFEST)).await {
        return Ok(extract_dir.to_path_buf());
    }

    let mut dirs = Vec::new();
    let mut entries = tokio::fs::read_dir(extract_dir)
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect workspace archive: {err}")))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect workspace archive: {err}")))?
    {
        let path = entry.path();
        if entry
            .metadata()
            .await
            .map_err(|err| RototoError::new(format!("failed to inspect workspace archive: {err}")))?
            .is_dir()
        {
            dirs.push(path);
        }
    }
    if dirs.len() == 1 && async_is_file(&dirs[0].join(WORKSPACE_MANIFEST)).await {
        return Ok(dirs.remove(0));
    }
    Err(RototoError::new(format!(
        "workspace archive from `{original}` does not contain a clear workspace root; use #:subdir"
    )))
}

async fn select_archive_subdir(
    extract_dir: &Path,
    subdir: &str,
    original: &str,
) -> Result<PathBuf> {
    if !relative_path_is_safe(Path::new(subdir)) {
        return Err(RototoError::new(format!(
            "workspace source subdir is unsafe: {subdir}"
        )));
    }

    match select_subdir(extract_dir, Some(subdir), original).await {
        Ok(root) => Ok(root),
        Err(err) => {
            let Some(wrapper) = single_archive_directory(extract_dir).await? else {
                return Err(err);
            };
            match select_subdir(&wrapper, Some(subdir), original).await {
                Ok(root) => Ok(root),
                Err(_) => Err(err),
            }
        }
    }
}

async fn single_archive_directory(extract_dir: &Path) -> Result<Option<PathBuf>> {
    let mut dirs = Vec::new();
    let mut entries = tokio::fs::read_dir(extract_dir)
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect workspace archive: {err}")))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect workspace archive: {err}")))?
    {
        let path = entry.path();
        if entry
            .metadata()
            .await
            .map_err(|err| RototoError::new(format!("failed to inspect workspace archive: {err}")))?
            .is_dir()
        {
            dirs.push(path);
        }
    }
    Ok((dirs.len() == 1).then(|| dirs.remove(0)))
}

fn archive_path_is_safe(path: &Path) -> bool {
    relative_path_is_safe(path)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::super::types::{
        DEFAULT_MAX_ARCHIVE_ENTRIES, DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES,
    };
    use super::*;

    fn write_archive_with_entry(
        path: &Path,
        entry_path: &str,
        entry_type: tar::EntryType,
    ) -> Result<()> {
        let file = std::fs::File::create(path)
            .map_err(|err| RototoError::new(format!("failed to create test archive: {err}")))?;
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(entry_type);
        header.set_size(0);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, entry_path, Cursor::new(Vec::<u8>::new()))
            .map_err(|err| RototoError::new(format!("failed to write test archive: {err}")))?;
        archive
            .finish()
            .map_err(|err| RototoError::new(format!("failed to finish test archive: {err}")))?;
        Ok(())
    }

    fn write_archive_with_file(path: &Path, entry_path: &str, contents: &[u8]) -> Result<()> {
        let file = std::fs::File::create(path)
            .map_err(|err| RototoError::new(format!("failed to create test archive: {err}")))?;
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, entry_path, Cursor::new(contents))
            .map_err(|err| RototoError::new(format!("failed to write test archive: {err}")))?;
        archive
            .finish()
            .map_err(|err| RototoError::new(format!("failed to finish test archive: {err}")))?;
        Ok(())
    }

    #[tokio::test]
    async fn select_archive_subdir_falls_back_to_single_wrapper_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let workspace = temp.path().join("repo-root").join("examples/basic");
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::write(workspace.join(WORKSPACE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();

        let root = select_archive_subdir(temp.path(), "examples/basic", "test.tar.gz")
            .await
            .unwrap();

        assert_eq!(root, tokio::fs::canonicalize(workspace).await.unwrap());
    }

    #[test]
    fn extract_archive_rejects_unsafe_paths() {
        assert!(!archive_path_is_safe(Path::new("../evil")));
        assert!(!archive_path_is_safe(Path::new("/tmp/evil")));
        assert!(!archive_path_is_safe(Path::new("workspace/../evil")));
    }

    #[test]
    fn extract_archive_rejects_special_entries() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("workspace.tar.gz");
        write_archive_with_entry(&archive_path, "workspace/fifo", tar::EntryType::Fifo).unwrap();

        let err = extract_archive(
            &archive_path,
            &temp.path().join("extract"),
            DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES,
            DEFAULT_MAX_ARCHIVE_ENTRIES,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unsupported entry type"));
    }

    #[test]
    fn extract_archive_skips_metadata_entries() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("workspace.tar.gz");
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut archive = tar::Builder::new(encoder);

            let mut metadata_header = tar::Header::new_gnu();
            metadata_header.set_entry_type(tar::EntryType::XGlobalHeader);
            metadata_header.set_size(0);
            metadata_header.set_mode(0o644);
            metadata_header.set_cksum();
            archive
                .append_data(
                    &mut metadata_header,
                    "pax_global_header",
                    Cursor::new(Vec::<u8>::new()),
                )
                .unwrap();

            let contents = b"schema_version = 1\n";
            let mut file_header = tar::Header::new_gnu();
            file_header.set_entry_type(tar::EntryType::Regular);
            file_header.set_size(contents.len() as u64);
            file_header.set_mode(0o644);
            file_header.set_cksum();
            archive
                .append_data(
                    &mut file_header,
                    "workspace/rototo-workspace.toml",
                    Cursor::new(contents),
                )
                .unwrap();

            archive.finish().unwrap();
        }

        let extract_dir = temp.path().join("extract");
        std::fs::create_dir_all(&extract_dir).unwrap();
        extract_archive(
            &archive_path,
            &extract_dir,
            DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES,
            DEFAULT_MAX_ARCHIVE_ENTRIES,
        )
        .unwrap();

        assert!(
            extract_dir
                .join("workspace/rototo-workspace.toml")
                .is_file()
        );
    }

    #[test]
    fn extract_archive_skips_link_entries() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("workspace.tar.gz");
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut archive = tar::Builder::new(encoder);

            let mut link_header = tar::Header::new_gnu();
            link_header.set_entry_type(tar::EntryType::Symlink);
            link_header.set_size(0);
            link_header.set_mode(0o777);
            link_header.set_link_name("target").unwrap();
            link_header.set_cksum();
            archive
                .append_data(
                    &mut link_header,
                    "workspace/ignored-link",
                    Cursor::new(Vec::<u8>::new()),
                )
                .unwrap();

            let contents = b"schema_version = 1\n";
            let mut file_header = tar::Header::new_gnu();
            file_header.set_entry_type(tar::EntryType::Regular);
            file_header.set_size(contents.len() as u64);
            file_header.set_mode(0o644);
            file_header.set_cksum();
            archive
                .append_data(
                    &mut file_header,
                    "workspace/rototo-workspace.toml",
                    Cursor::new(contents),
                )
                .unwrap();

            archive.finish().unwrap();
        }

        let extract_dir = temp.path().join("extract");
        std::fs::create_dir_all(&extract_dir).unwrap();
        extract_archive(
            &archive_path,
            &extract_dir,
            DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES,
            DEFAULT_MAX_ARCHIVE_ENTRIES,
        )
        .unwrap();

        assert!(!extract_dir.join("workspace/ignored-link").exists());
        assert!(
            extract_dir
                .join("workspace/rototo-workspace.toml")
                .is_file()
        );
    }

    #[test]
    fn extract_archive_rejects_decompressed_size_over_limit() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("workspace.tar.gz");
        write_archive_with_file(&archive_path, "workspace/rototo-workspace.toml", b"12345")
            .unwrap();

        let err = extract_archive(&archive_path, &temp.path().join("extract"), 4, 10).unwrap_err();

        assert!(
            err.to_string()
                .contains("decompressed content is too large")
        );
    }

    #[test]
    fn extract_archive_rejects_entry_count_over_limit() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("workspace.tar.gz");
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut archive = tar::Builder::new(encoder);
            for entry_path in ["workspace/a.toml", "workspace/b.toml"] {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(0);
                header.set_mode(0o644);
                header.set_cksum();
                archive
                    .append_data(&mut header, entry_path, Cursor::new(Vec::<u8>::new()))
                    .unwrap();
            }
            archive.finish().unwrap();
        }

        let extract_dir = temp.path().join("extract");
        std::fs::create_dir_all(&extract_dir).unwrap();
        let err = extract_archive(&archive_path, &extract_dir, 100, 1).unwrap_err();

        assert!(err.to_string().contains("too many entries"), "{err}");
    }

    #[tokio::test]
    async fn content_hash_fingerprint_uses_stable_sha256_digest() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("workspace.tar.gz");
        tokio::fs::write(&archive_path, b"abc").await.unwrap();

        assert_eq!(
            content_hash_fingerprint(&archive_path).await.unwrap(),
            SourceFingerprint::ContentHash(
                "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                    .to_owned()
            )
        );
    }

    #[test]
    fn http_validator_fingerprint_prefers_etag() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::ETAG, "\"v2\"".parse().unwrap());
        headers.insert(
            reqwest::header::LAST_MODIFIED,
            "Thu, 28 May 2026 12:00:00 GMT".parse().unwrap(),
        );

        assert_eq!(
            http_validator_fingerprint(&headers),
            Some(SourceFingerprint::HttpValidator("etag:\"v2\"".to_owned()))
        );
    }

    #[test]
    fn http_validator_fingerprint_uses_last_modified_without_etag() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::LAST_MODIFIED,
            "Thu, 28 May 2026 12:00:00 GMT".parse().unwrap(),
        );

        assert_eq!(
            http_validator_fingerprint(&headers),
            Some(SourceFingerprint::HttpValidator(
                "last-modified:Thu, 28 May 2026 12:00:00 GMT".to_owned()
            ))
        );
    }
}
