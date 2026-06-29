//! Project a reviewed package into a deterministic, content-addressed archive.
//!
//! A distributable archive is the release-time boundary between a Git-backed
//! package and the running fleet. Operators upload the archive to object
//! storage, move a channel pointer at it, and let instances refresh. For that
//! workflow to be safe, the archive must be reproducible: the same package tree
//! must always produce the same bytes, so the same digest, so the same URL.
//! Determinism is what makes the digest a stable rollback target rather than a
//! value that drifts every time the release pipeline runs.
//!
//! We get determinism by removing every incidental input to the archive bytes:
//! entries are sorted by path, permissions are fixed, modification times are
//! zeroed, ownership is dropped, and compression runs at a fixed level. The
//! resulting digest is the same `sha256:<digest>` content hash the SDK derives
//! when it later downloads the archive, so the name an operator publishes and
//! the identity an instance reports are the same value.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::GzBuilder;

use crate::error::{Result, RototoError};
use crate::lint::lint_package;
use crate::source::{SourceOptions, stage_package_source};

const PACKAGE_MANIFEST: &str = "rototo-package.toml";

/// A package projected into a deterministic, content-addressed archive.
#[derive(Debug, Clone)]
pub struct PackagedArchive {
    /// The content-addressed release identity, for example `sha256:<digest>`.
    /// This matches the content-hash fingerprint the SDK derives when it later
    /// downloads the archive, so a release pipeline can target this value.
    pub release_id: String,
    /// The archive file name, `<release-id>.tar.gz`.
    pub file_name: String,
    /// The gzip-compressed tar archive bytes.
    pub bytes: Vec<u8>,
}

/// Loads `source`, requires it to be lint-clean, and projects it into a
/// deterministic, content-addressed `.tar.gz` archive.
///
/// The staged tree is already self-contained: any `extends` parents are merged
/// in by source loading, so the archive carries the effective package a runtime
/// would resolve. The merged manifest's `extends` key is dropped so the archive
/// loads without re-fetching parent sources.
pub async fn pack_package(source: &str, options: &SourceOptions) -> Result<PackagedArchive> {
    let staged = stage_package_source(source, options).await?;

    // A distributable archive is an immutable release artifact; refuse to ship
    // a package that does not pass its own validation.
    let lint = lint_package(staged.path()).await?;
    if lint.has_errors() {
        let errors = lint
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::diagnostics::Severity::Error)
            .count();
        return Err(RototoError::new(format!(
            "cannot package `{source}`: {errors} lint error(s); run `rototo lint {source}` for details"
        )));
    }

    let root = staged.path().to_path_buf();
    let bytes = tokio::task::spawn_blocking(move || build_archive(&root))
        .await
        .map_err(|err| RototoError::new(format!("package archive task failed: {err}")))??;

    let release_id = format!("sha256:{}", sha256_hex(&bytes));
    let file_name = format!("{release_id}.tar.gz");
    Ok(PackagedArchive {
        release_id,
        file_name,
        bytes,
    })
}

/// Builds the deterministic gzip-compressed tar archive for the package rooted
/// at `root`. Synchronous; callers run it on a blocking thread.
fn build_archive(root: &Path) -> Result<Vec<u8>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    // Sort by archive path so entry order does not depend on directory
    // iteration order, which the filesystem does not guarantee.
    files.sort_by(|(left, _), (right, _)| left.cmp(right));

    // mtime(0) keeps the gzip header free of a wall-clock timestamp; a fixed
    // compression level keeps the compressed bytes reproducible.
    let encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::new(6));
    let mut builder = tar::Builder::new(encoder);
    for (archive_path, absolute) in &files {
        let contents = if archive_path == PACKAGE_MANIFEST {
            manifest_bytes(absolute)?
        } else {
            std::fs::read(absolute).map_err(|err| {
                RototoError::new(format!(
                    "failed to read package file {}: {err}",
                    absolute.display()
                ))
            })?
        };
        append_file(&mut builder, archive_path, &contents)?;
    }

    let encoder = builder
        .into_inner()
        .map_err(|err| RototoError::new(format!("failed to finish package archive: {err}")))?;
    encoder
        .finish()
        .map_err(|err| RototoError::new(format!("failed to compress package archive: {err}")))
}

/// Recursively collects regular files under `dir` as `(archive_path, absolute)`
/// pairs, where `archive_path` is the slash-separated path relative to `root`.
/// Skips `.git` metadata and symlinks; the loader rejects both on extraction.
fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|err| {
        RototoError::new(format!(
            "failed to read package directory {}: {err}",
            dir.display()
        ))
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|err| RototoError::new(format!("failed to read package entry: {err}")))?;
        let file_name = entry.file_name();
        if file_name == ".git" {
            continue;
        }
        let file_type = entry.file_type().map_err(|err| {
            RototoError::new(format!(
                "failed to inspect package entry {}: {err}",
                entry.path().display()
            ))
        })?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|_| {
                RototoError::new(format!(
                    "package file {} is outside the package root",
                    path.display()
                ))
            })?;
            files.push((archive_path(relative), path));
        }
    }
    Ok(())
}

fn archive_path(relative: &Path) -> String {
    relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// Returns the manifest bytes for the archive, dropping any `extends` key so the
/// archived package loads without re-fetching parent sources. Manifests without
/// `extends` are copied byte for byte, which keeps the common case stable
/// regardless of TOML serialization.
fn manifest_bytes(path: &Path) -> Result<Vec<u8>> {
    let raw = std::fs::read(path).map_err(|err| {
        RototoError::new(format!(
            "failed to read package manifest {}: {err}",
            path.display()
        ))
    })?;
    let text = std::str::from_utf8(&raw)
        .map_err(|err| RototoError::new(format!("package manifest is not valid UTF-8: {err}")))?;
    let mut manifest = text
        .parse::<toml::Value>()
        .map_err(|err| RototoError::new(format!("failed to parse package manifest: {err}")))?;
    if let Some(table) = manifest.as_table_mut()
        && table.remove("extends").is_some()
    {
        return toml::to_string(&manifest)
            .map(String::into_bytes)
            .map_err(|err| RototoError::new(format!("failed to rewrite package manifest: {err}")));
    }
    Ok(raw)
}

fn append_file(
    builder: &mut tar::Builder<impl std::io::Write>,
    archive_path: &str,
    contents: &[u8],
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    builder
        .append_data(&mut header, archive_path, Cursor::new(contents))
        .map_err(|err| {
            RototoError::new(format!(
                "failed to add {archive_path} to package archive: {err}"
            ))
        })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut encoded = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn write_package(root: &Path) {
        tokio::fs::write(root.join(PACKAGE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("variables/flag.toml"),
            "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = true\n",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn pack_package_is_deterministic_and_content_addressed() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("package");
        tokio::fs::create_dir_all(&root).await.unwrap();
        write_package(&root).await;
        let source = root.display().to_string();

        let first = pack_package(&source, &SourceOptions::default())
            .await
            .unwrap();
        let second = pack_package(&source, &SourceOptions::default())
            .await
            .unwrap();

        assert_eq!(first.bytes, second.bytes);
        assert_eq!(first.release_id, second.release_id);
        assert!(first.release_id.starts_with("sha256:"));
        assert_eq!(first.file_name, format!("{}.tar.gz", first.release_id));
        // The release id is the content hash of the archive bytes.
        assert_eq!(
            first.release_id,
            format!("sha256:{}", sha256_hex(&first.bytes))
        );
    }

    #[tokio::test]
    async fn pack_package_strips_extends_from_the_manifest() {
        let temp = tempfile::TempDir::new().unwrap();
        let parent = temp.path().join("parent");
        let child = temp.path().join("child");
        tokio::fs::create_dir_all(&parent).await.unwrap();
        tokio::fs::create_dir_all(&child).await.unwrap();
        write_package(&parent).await;
        tokio::fs::write(
            child.join(PACKAGE_MANIFEST),
            "schema_version = 1\nextends = [\"../parent\"]\n",
        )
        .await
        .unwrap();

        let archive = pack_package(&child.display().to_string(), &SourceOptions::default())
            .await
            .unwrap();

        let manifest = read_archive_entry(&archive.bytes, PACKAGE_MANIFEST);
        let manifest = String::from_utf8(manifest).unwrap();
        assert!(!manifest.contains("extends"), "{manifest}");
        // The merged parent file is carried into the archive.
        assert!(!read_archive_entry(&archive.bytes, "variables/flag.toml").is_empty());
    }

    #[tokio::test]
    async fn pack_package_rejects_lint_failures() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("package");
        tokio::fs::create_dir_all(&root).await.unwrap();
        // Missing schema_version makes the manifest fail lint.
        tokio::fs::write(root.join(PACKAGE_MANIFEST), "name = \"broken\"\n")
            .await
            .unwrap();

        let err = pack_package(&root.display().to_string(), &SourceOptions::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("lint error"), "{err}");
    }

    fn read_archive_entry(bytes: &[u8], wanted: &str) -> Vec<u8> {
        use std::io::Read;
        let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            if path == wanted {
                let mut contents = Vec::new();
                entry.read_to_end(&mut contents).unwrap();
                return contents;
            }
        }
        panic!("archive entry not found: {wanted}");
    }
}
