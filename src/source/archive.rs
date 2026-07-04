use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

use crate::error::{Result, RototoError};

use super::PACKAGE_MANIFEST;
use super::auth::SourceAuth;
use super::path::{async_is_file, relative_path_is_safe, select_subdir};
#[cfg(feature = "console")]
use super::types::StagedSourceTree;
use super::types::{
    LoadedPackageSource, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe, StagedPackage,
};
use super::uri::SourceUri;

const ERROR_BODY_PREVIEW_BYTES: u64 = 4096;
const HTTP_USER_AGENT: &str = concat!("rototo/", env!("CARGO_PKG_VERSION"));

pub(super) async fn stage_https_archive(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
) -> Result<LoadedPackageSource> {
    if uri.ref_.is_some() {
        return Err(RototoError::new(
            "https package sources only support #:subdir fragments",
        ));
    }

    let ExtractedArchive {
        extract_dir,
        fingerprint,
        tempdir,
    } = extract_https_archive(uri, original, options).await?;
    let root = match uri.subdir.as_deref() {
        Some(subdir) => select_archive_subdir(&extract_dir, subdir, original).await?,
        None => infer_archive_package_root(&extract_dir, original).await?,
    };
    Ok(LoadedPackageSource {
        staged: StagedPackage::temporary(root, tempdir),
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
    let request = apply_archive_auth(client.get(&url), &url, options)?;
    let response = request
        .send()
        .await
        .map_err(|err| RototoError::new(format!("failed to fetch package archive: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        let preview = response_preview(response, ERROR_BODY_PREVIEW_BYTES).await?;
        let detail = if preview.is_empty() {
            String::new()
        } else {
            format!(": {preview}")
        };
        return Err(RototoError::new(format!(
            "failed to fetch package archive: HTTP {status}{detail}{}",
            auth_failure_hint(status, &url, options)
        )));
    }
    let fingerprint = response_fingerprint(&response);
    if let Some(length) = response.content_length()
        && length > options.max_archive_bytes()
    {
        return Err(RototoError::new(format!(
            "package archive is too large: {length} bytes exceeds limit of {} bytes",
            options.max_archive_bytes()
        )));
    }

    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let archive_path = tempdir.path().join("package.tar.gz");
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
            "https package sources only support #:subdir fragments",
        ));
    }
    let url = format!("{}://{}", uri.scheme, uri.base);
    let client = https_archive_client(options)?;
    let request = apply_archive_auth(client.head(&url), &url, options)?;
    let response = request
        .send()
        .await
        .map_err(|err| RototoError::new(format!("failed to check package archive: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(RototoError::new(format!(
            "failed to check package archive: HTTP {status}{}",
            auth_failure_hint(status, &url, options)
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

/// Attaches the bearer token this request's URL is entitled to. A bare token
/// binds to the load graph's single archive origin (a second distinct origin
/// fails the load); scoped tokens go to the longest matching prefix, and a
/// URL no prefix matches goes out anonymous.
///
/// Tokens are attached per request, so on a redirect reqwest owns the header:
/// its redirect layer strips `Authorization` whenever the redirect target's
/// host or port differs from the previous hop
/// (`reqwest::redirect::remove_sensitive_headers`), so a token never follows
/// a cross-origin redirect.
fn apply_archive_auth(
    request: reqwest::RequestBuilder,
    url: &str,
    options: &SourceOptions,
) -> Result<reqwest::RequestBuilder> {
    match options.auth() {
        SourceAuth::None => Ok(request),
        SourceAuth::Bearer(token) => {
            options.bearer_origin().bind(url)?;
            Ok(request.bearer_auth(token))
        }
        SourceAuth::Scoped(tokens) => match tokens.token_for(url) {
            Some(token) => Ok(request.bearer_auth(token)),
            None => Ok(request),
        },
    }
}

/// Names what credential a failed archive request carried, so a 401 or 403
/// says which entry to fix instead of leaving the operator to guess.
fn auth_failure_hint(status: reqwest::StatusCode, url: &str, options: &SourceOptions) -> String {
    if !matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return String::new();
    }
    let origin = super::auth::archive_origin(url);
    match options.auth() {
        SourceAuth::None => format!(
            " (no package token configured; pass --package-token {origin}/...=TOKEN or set ROTOTO_PACKAGE_TOKEN)"
        ),
        SourceAuth::Bearer(_) => " (sent the bare package token)".to_owned(),
        SourceAuth::Scoped(tokens) => match tokens.matching_prefix(url) {
            Some(prefix) => format!(" (sent the token scoped to {prefix})"),
            None => format!(" (no package token entry matched this URL; add {origin}/...=TOKEN)"),
        },
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
            return attempt.error("package archive redirects must stay on https");
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
            "failed to read package archive {}: {err}",
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
            "failed to create package archive {}: {err}",
            path.display()
        ))
    })?;
    let mut total = 0_u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| RototoError::new(format!("failed to read package archive: {err}")))?
    {
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| RototoError::new("package archive is too large"))?;
        if total > max_bytes {
            return Err(RototoError::new(format!(
                "package archive is too large: exceeded limit of {max_bytes} bytes"
            )));
        }
        file.write_all(&chunk)
            .await
            .map_err(|err| RototoError::new(format!("failed to write package archive: {err}")))?;
    }
    file.flush()
        .await
        .map_err(|err| RototoError::new(format!("failed to write package archive: {err}")))?;
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
            "failed to open package archive {}: {err}",
            archive_path.display()
        ))
    })?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut entry_count = 0_u64;
    let mut decompressed_bytes = 0_u64;
    for entry in archive
        .entries()
        .map_err(|err| RototoError::new(format!("failed to read package archive: {err}")))?
    {
        let mut entry = entry
            .map_err(|err| RototoError::new(format!("failed to read archive entry: {err}")))?;
        let entry_type = entry.header().entry_type();
        entry_count = entry_count
            .checked_add(1)
            .ok_or_else(|| RototoError::new("package archive contains too many entries"))?;
        if entry_count > max_entries {
            return Err(RototoError::new(format!(
                "package archive contains too many entries: exceeded limit of {max_entries}"
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
                "package archive contains unsafe path: {}",
                path.display()
            )));
        }
        if !(entry_type.is_file() || entry_type.is_dir()) {
            return Err(RototoError::new(format!(
                "package archive contains unsupported entry type at: {}",
                path.display()
            )));
        }
        if entry_type.is_file() {
            decompressed_bytes = decompressed_bytes
                .checked_add(entry.header().size().map_err(|err| {
                    RototoError::new(format!("archive entry size is invalid: {err}"))
                })?)
                .ok_or_else(|| RototoError::new("package archive is too large"))?;
            if decompressed_bytes > max_decompressed_bytes {
                return Err(RototoError::new(format!(
                    "package archive decompressed content is too large: exceeded limit of {max_decompressed_bytes} bytes"
                )));
            }
        }
        entry
            .unpack_in(extract_dir)
            .map_err(|err| RototoError::new(format!("failed to extract package archive: {err}")))?;
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

async fn infer_archive_package_root(extract_dir: &Path, original: &str) -> Result<PathBuf> {
    if async_is_file(&extract_dir.join(PACKAGE_MANIFEST)).await {
        return Ok(extract_dir.to_path_buf());
    }

    let mut dirs = Vec::new();
    let mut entries = tokio::fs::read_dir(extract_dir)
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect package archive: {err}")))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect package archive: {err}")))?
    {
        let path = entry.path();
        if entry
            .metadata()
            .await
            .map_err(|err| RototoError::new(format!("failed to inspect package archive: {err}")))?
            .is_dir()
        {
            dirs.push(path);
        }
    }
    if dirs.len() == 1 && async_is_file(&dirs[0].join(PACKAGE_MANIFEST)).await {
        return Ok(dirs.remove(0));
    }
    Err(RototoError::new(format!(
        "package archive from `{original}` does not contain a clear package root; use #:subdir"
    )))
}

async fn select_archive_subdir(
    extract_dir: &Path,
    subdir: &str,
    original: &str,
) -> Result<PathBuf> {
    if !relative_path_is_safe(Path::new(subdir)) {
        return Err(RototoError::new(format!(
            "package source subdir is unsafe: {subdir}"
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
        .map_err(|err| RototoError::new(format!("failed to inspect package archive: {err}")))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to inspect package archive: {err}")))?
    {
        let path = entry.path();
        if entry
            .metadata()
            .await
            .map_err(|err| RototoError::new(format!("failed to inspect package archive: {err}")))?
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
        let package = temp.path().join("repo-root").join("examples/basic");
        tokio::fs::create_dir_all(&package).await.unwrap();
        tokio::fs::write(package.join(PACKAGE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();

        let root = select_archive_subdir(temp.path(), "examples/basic", "test.tar.gz")
            .await
            .unwrap();

        assert_eq!(root, tokio::fs::canonicalize(package).await.unwrap());
    }

    #[test]
    fn extract_archive_rejects_unsafe_paths() {
        assert!(!archive_path_is_safe(Path::new("../evil")));
        assert!(!archive_path_is_safe(Path::new("/tmp/evil")));
        assert!(!archive_path_is_safe(Path::new("package/../evil")));
    }

    #[test]
    fn extract_archive_rejects_special_entries() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("package.tar.gz");
        write_archive_with_entry(&archive_path, "package/fifo", tar::EntryType::Fifo).unwrap();

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
        let archive_path = temp.path().join("package.tar.gz");
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
                    "package/rototo-package.toml",
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

        assert!(extract_dir.join("package/rototo-package.toml").is_file());
    }

    #[test]
    fn extract_archive_skips_link_entries() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("package.tar.gz");
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
                    "package/ignored-link",
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
                    "package/rototo-package.toml",
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

        assert!(!extract_dir.join("package/ignored-link").exists());
        assert!(extract_dir.join("package/rototo-package.toml").is_file());
    }

    #[test]
    fn extract_archive_rejects_decompressed_size_over_limit() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("package.tar.gz");
        write_archive_with_file(&archive_path, "package/rototo-package.toml", b"12345").unwrap();

        let err = extract_archive(&archive_path, &temp.path().join("extract"), 4, 10).unwrap_err();

        assert!(
            err.to_string()
                .contains("decompressed content is too large")
        );
    }

    #[test]
    fn extract_archive_rejects_entry_count_over_limit() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("package.tar.gz");
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut archive = tar::Builder::new(encoder);
            for entry_path in ["package/a.toml", "package/b.toml"] {
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
        let archive_path = temp.path().join("package.tar.gz");
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

#[cfg(test)]
mod auth_tests {
    use super::super::auth::ScopedBearerTokens;
    use super::*;

    fn authorization_header(request: reqwest::RequestBuilder) -> Option<String> {
        let request = request.build().unwrap();
        request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .map(|value| value.to_str().unwrap().to_owned())
    }

    fn get(url: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new().get(url)
    }

    #[test]
    fn anonymous_requests_carry_no_authorization_header() {
        let options = SourceOptions::new();
        let url = "https://config.example.com/pkg.tar.gz";
        let request = apply_archive_auth(get(url), url, &options).unwrap();
        assert_eq!(authorization_header(request), None);
    }

    #[test]
    fn a_bare_token_attaches_and_binds_to_the_first_origin() {
        let options = SourceOptions::new().with_auth(SourceAuth::Bearer("secret".to_owned()));
        let url = "https://config.example.com/pkg.tar.gz";
        let request = apply_archive_auth(get(url), url, &options).unwrap();
        assert_eq!(
            authorization_header(request).as_deref(),
            Some("Bearer secret")
        );

        // The same origin keeps working; a second origin is refused rather
        // than receiving a token minted for the first.
        let same_origin = "https://config.example.com/other.tar.gz";
        let _ = apply_archive_auth(get(same_origin), same_origin, &options).unwrap();
        let other_origin = "https://cdn.example.net/pkg.tar.gz";
        let err = apply_archive_auth(get(other_origin), other_origin, &options).unwrap_err();
        assert!(err.to_string().contains("https://cdn.example.net"));
    }

    #[test]
    fn scoped_tokens_attach_on_match_and_stay_silent_otherwise() {
        let tokens = ScopedBearerTokens::new()
            .with_prefix("https://config.example.com/team-a", "team-a-token")
            .unwrap();
        let options = SourceOptions::new().with_auth(SourceAuth::Scoped(tokens));

        let matching = "https://config.example.com/team-a/pkg.tar.gz";
        let request = apply_archive_auth(get(matching), matching, &options).unwrap();
        assert_eq!(
            authorization_header(request).as_deref(),
            Some("Bearer team-a-token")
        );

        let unmatched = "https://config.example.com/team-b/pkg.tar.gz";
        let request = apply_archive_auth(get(unmatched), unmatched, &options).unwrap();
        assert_eq!(authorization_header(request), None);
    }

    #[test]
    fn auth_failure_hints_name_the_credential_that_was_sent() {
        let url = "https://config.example.com/team-a/pkg.tar.gz";

        let anonymous = SourceOptions::new();
        let hint = auth_failure_hint(reqwest::StatusCode::UNAUTHORIZED, url, &anonymous);
        assert!(hint.contains("no package token configured"));
        assert!(hint.contains("https://config.example.com"));

        let bare = SourceOptions::new().with_auth(SourceAuth::Bearer("secret".to_owned()));
        let hint = auth_failure_hint(reqwest::StatusCode::FORBIDDEN, url, &bare);
        assert!(hint.contains("bare package token"));

        let tokens = ScopedBearerTokens::new()
            .with_prefix("https://config.example.com/team-a", "token")
            .unwrap();
        let scoped = SourceOptions::new().with_auth(SourceAuth::Scoped(tokens.clone()));
        let hint = auth_failure_hint(reqwest::StatusCode::UNAUTHORIZED, url, &scoped);
        assert!(hint.contains("scoped to https://config.example.com/team-a"));

        let unmatched_url = "https://cdn.example.net/pkg.tar.gz";
        let hint = auth_failure_hint(reqwest::StatusCode::UNAUTHORIZED, unmatched_url, &scoped);
        assert!(hint.contains("no package token entry matched"));

        // A non-auth status never gets an auth hint.
        let hint = auth_failure_hint(reqwest::StatusCode::NOT_FOUND, url, &scoped);
        assert!(hint.is_empty());
    }
}
