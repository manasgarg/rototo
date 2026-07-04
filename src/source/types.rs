use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;

const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
pub(super) const DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES: u64 = 200 * 1024 * 1024;
pub(super) const DEFAULT_MAX_ARCHIVE_ENTRIES: u64 = 10_000;

use super::auth::{BearerOriginBinding, SourceAuth};

#[derive(Clone, Debug)]
pub struct SourceOptions {
    auth: SourceAuth,
    /// The origin a bare bearer token has bound to, shared across clones so
    /// every fetch in one load graph agrees.
    bearer_origin: BearerOriginBinding,
    git_timeout: Duration,
    http_timeout: Duration,
    max_archive_bytes: u64,
    max_decompressed_archive_bytes: u64,
    max_archive_entries: u64,
}

impl SourceOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn auth(&self) -> &SourceAuth {
        &self.auth
    }

    pub(super) fn bearer_origin(&self) -> &BearerOriginBinding {
        &self.bearer_origin
    }

    pub fn git_timeout(&self) -> Duration {
        self.git_timeout
    }

    pub fn http_timeout(&self) -> Duration {
        self.http_timeout
    }

    pub fn max_archive_bytes(&self) -> u64 {
        self.max_archive_bytes
    }

    pub fn max_decompressed_archive_bytes(&self) -> u64 {
        self.max_decompressed_archive_bytes
    }

    pub fn max_archive_entries(&self) -> u64 {
        self.max_archive_entries
    }

    pub fn with_auth(mut self, auth: SourceAuth) -> Self {
        self.auth = auth;
        self
    }

    pub fn with_git_timeout(mut self, timeout: Duration) -> Self {
        self.git_timeout = timeout;
        self
    }

    pub fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }

    pub fn with_max_archive_bytes(mut self, bytes: u64) -> Self {
        self.max_archive_bytes = bytes;
        self
    }

    pub fn with_max_decompressed_archive_bytes(mut self, bytes: u64) -> Self {
        self.max_decompressed_archive_bytes = bytes;
        self
    }

    pub fn with_max_archive_entries(mut self, entries: u64) -> Self {
        self.max_archive_entries = entries;
        self
    }
}

impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            auth: SourceAuth::None,
            bearer_origin: BearerOriginBinding::default(),
            git_timeout: Duration::from_secs(60),
            http_timeout: Duration::from_secs(30),
            max_archive_bytes: DEFAULT_MAX_ARCHIVE_BYTES,
            max_decompressed_archive_bytes: DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES,
            max_archive_entries: DEFAULT_MAX_ARCHIVE_ENTRIES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceFingerprint {
    GitCommit(String),
    HttpValidator(String),
    ContentHash(String),
    PackageLayers(Vec<SourceFingerprint>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceProbe {
    Unchanged,
    Changed(Option<SourceFingerprint>),
    ImmutablePinned(SourceFingerprint),
    Unknown,
}

#[derive(Debug)]
pub struct LoadedPackageSource {
    pub(super) staged: StagedPackage,
    pub(super) fingerprint: Option<SourceFingerprint>,
    pub(super) immutable: bool,
    pub(super) layers: Vec<SourceLayer>,
}

impl LoadedPackageSource {
    pub fn staged(&self) -> &StagedPackage {
        &self.staged
    }

    pub fn into_staged(self) -> StagedPackage {
        self.staged
    }

    pub fn fingerprint(&self) -> Option<&SourceFingerprint> {
        self.fingerprint.as_ref()
    }

    pub fn immutable(&self) -> bool {
        self.immutable
    }

    pub fn layers(&self) -> &[SourceLayer] {
        &self.layers
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLayer {
    pub(super) source: String,
    pub(super) fingerprint: Option<SourceFingerprint>,
    pub(super) immutable: bool,
}

impl SourceLayer {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn fingerprint(&self) -> Option<&SourceFingerprint> {
        self.fingerprint.as_ref()
    }

    pub fn immutable(&self) -> bool {
        self.immutable
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum LocalStageMode {
    Borrow,
    Snapshot,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ExtendSourceBase<'a> {
    pub(super) path: &'a Path,
    pub(super) temporary: bool,
}

#[derive(Debug)]
pub(super) struct ResolvedExtendSource {
    pub(super) source: String,
    pub(super) inherited_temporary_base: bool,
}

#[derive(Debug)]
pub struct StagedPackage {
    path: PathBuf,
    _tempdir: Option<TempDir>,
}

impl StagedPackage {
    pub fn local(path: PathBuf) -> Self {
        Self {
            path,
            _tempdir: None,
        }
    }

    pub(super) fn temporary(path: PathBuf, tempdir: TempDir) -> Self {
        Self {
            path,
            _tempdir: Some(tempdir),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_temporary(&self) -> bool {
        self._tempdir.is_some()
    }
}

#[derive(Debug)]
pub(crate) struct StagedSourceTree {
    root: PathBuf,
    fingerprint: Option<SourceFingerprint>,
    immutable: bool,
    _tempdir: Option<TempDir>,
}

impl StagedSourceTree {
    #[cfg(feature = "console")]
    pub(super) fn local(root: PathBuf) -> Self {
        Self {
            root,
            fingerprint: None,
            immutable: false,
            _tempdir: None,
        }
    }

    pub(super) fn temporary(
        root: PathBuf,
        tempdir: TempDir,
        fingerprint: Option<SourceFingerprint>,
        immutable: bool,
    ) -> Self {
        Self {
            root,
            fingerprint,
            immutable,
            _tempdir: Some(tempdir),
        }
    }

    #[cfg(feature = "console")]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn fingerprint(&self) -> Option<&SourceFingerprint> {
        self.fingerprint.as_ref()
    }

    pub(super) fn immutable(&self) -> bool {
        self.immutable
    }

    pub(super) fn into_staged_package(self) -> StagedPackage {
        StagedPackage {
            path: self.root,
            _tempdir: self._tempdir,
        }
    }
}
