use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;

const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
pub(super) const DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES: u64 = 200 * 1024 * 1024;
pub(super) const DEFAULT_MAX_ARCHIVE_ENTRIES: u64 = 10_000;

#[derive(Clone, Debug)]
pub struct SourceOptions {
    auth: SourceAuth,
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
            git_timeout: Duration::from_secs(60),
            http_timeout: Duration::from_secs(30),
            max_archive_bytes: DEFAULT_MAX_ARCHIVE_BYTES,
            max_decompressed_archive_bytes: DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES,
            max_archive_entries: DEFAULT_MAX_ARCHIVE_ENTRIES,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SourceAuth {
    None,
    Bearer(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceFingerprint {
    GitCommit(String),
    HttpValidator(String),
    ContentHash(String),
    WorkspaceLayers(Vec<SourceFingerprint>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceProbe {
    Unchanged,
    Changed(Option<SourceFingerprint>),
    ImmutablePinned(SourceFingerprint),
    Unknown,
}

#[derive(Debug)]
pub struct LoadedWorkspaceSource {
    pub(super) staged: StagedWorkspace,
    pub(super) fingerprint: Option<SourceFingerprint>,
    pub(super) immutable: bool,
    pub(super) layers: Vec<SourceLayer>,
}

impl LoadedWorkspaceSource {
    pub fn staged(&self) -> &StagedWorkspace {
        &self.staged
    }

    pub fn into_staged(self) -> StagedWorkspace {
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
pub struct StagedWorkspace {
    path: PathBuf,
    _tempdir: Option<TempDir>,
}

impl StagedWorkspace {
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
