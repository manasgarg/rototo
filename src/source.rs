use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::{Result, RototoError};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
const ERROR_BODY_PREVIEW_BYTES: u64 = 4096;

#[derive(Clone, Debug)]
pub struct SourceOptions {
    auth: SourceAuth,
    git_timeout: Duration,
    http_timeout: Duration,
    max_archive_bytes: u64,
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
}

impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            auth: SourceAuth::None,
            git_timeout: Duration::from_secs(60),
            http_timeout: Duration::from_secs(30),
            max_archive_bytes: DEFAULT_MAX_ARCHIVE_BYTES,
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
    staged: StagedWorkspace,
    fingerprint: Option<SourceFingerprint>,
    immutable: bool,
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

    fn temporary(path: PathBuf, tempdir: TempDir) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceUri {
    scheme: String,
    base: String,
    ref_: Option<String>,
    subdir: Option<String>,
}

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
    let source = source.as_ref();
    match SourceUri::parse(source)? {
        None => Ok(LoadedWorkspaceSource {
            staged: stage_local_path(Path::new(source)).await?,
            fingerprint: None,
            immutable: false,
        }),
        Some(uri) => match uri.scheme.as_str() {
            "file" => Ok(LoadedWorkspaceSource {
                staged: stage_file_uri(&uri).await?,
                fingerprint: None,
                immutable: false,
            }),
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
        _ => load_workspace_source(source, options).await,
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

async fn stage_local_path(path: &Path) -> Result<StagedWorkspace> {
    Ok(StagedWorkspace::local(path.to_path_buf()))
}

async fn snapshot_local_path(path: &Path) -> Result<LoadedWorkspaceSource> {
    let source = path.to_path_buf();
    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let target = tempdir.path().join("workspace");
    let target_for_task = target.clone();
    tokio::task::spawn_blocking(move || copy_dir_recursive(&source, &target_for_task))
        .await
        .map_err(|err| RototoError::new(format!("workspace snapshot task failed: {err}")))??;
    Ok(LoadedWorkspaceSource {
        staged: StagedWorkspace::temporary(target, tempdir),
        fingerprint: None,
        immutable: false,
    })
}

async fn stage_file_uri(uri: &SourceUri) -> Result<StagedWorkspace> {
    if uri.ref_.is_some() || uri.subdir.is_some() {
        return Err(RototoError::new(
            "file:// workspace sources do not support fragments",
        ));
    }
    stage_local_path(Path::new(&uri.base)).await
}

async fn stage_git_repo(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    let inner_scheme = uri
        .scheme
        .strip_prefix("git+")
        .ok_or_else(|| RototoError::new("invalid git workspace source"))?;
    if !matches!(inner_scheme, "file" | "https" | "ssh") {
        return Err(RototoError::new(format!(
            "git workspace source scheme is not supported: git+{inner_scheme}"
        )));
    }
    let clone_url = format!("{inner_scheme}://{}", uri.base);
    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let clone_dir = tempdir.path().join("clone");

    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command.arg("clone").arg("--quiet");
    let pinned_commit = uri.ref_.as_deref().is_some_and(is_full_git_commit);
    if !pinned_commit {
        command.arg("--depth=1");
    }
    if let Some(ref_) = &uri.ref_
        && !pinned_commit
    {
        command.arg("--branch").arg(ref_);
    }
    command.arg(&clone_url).arg(&clone_dir);

    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| {
            RototoError::new(format!(
                "git fetch timed out for workspace source: {original}"
            ))
        })?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git fetch failed for workspace source: {}",
            stderr.trim()
        )));
    }

    if let Some(ref_) = uri.ref_.as_deref()
        && pinned_commit
    {
        git_checkout(&clone_dir, ref_, options).await?;
    }
    let commit = git_rev_parse_head(&clone_dir, options).await?;
    let root = select_subdir(&clone_dir, uri.subdir.as_deref(), original).await?;
    Ok(LoadedWorkspaceSource {
        staged: StagedWorkspace::temporary(root, tempdir),
        fingerprint: Some(SourceFingerprint::GitCommit(commit.clone())),
        immutable: pinned_commit,
    })
}

async fn stage_https_archive(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    if uri.ref_.is_some() {
        return Err(RototoError::new(
            "https workspace sources only support #:subdir fragments",
        ));
    }

    let url = format!("{}://{}", uri.scheme, uri.base);
    let client = reqwest::Client::builder()
        .timeout(options.http_timeout())
        .build()
        .map_err(|err| RototoError::new(format!("failed to build HTTP client: {err}")))?;
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
    tokio::task::spawn_blocking(move || extract_archive(&archive_path, &extract_dir_for_task))
        .await
        .map_err(|err| RototoError::new(format!("archive extraction task failed: {err}")))??;

    let root = match uri.subdir.as_deref() {
        Some(subdir) => select_subdir(&extract_dir, Some(subdir), original).await?,
        None => infer_archive_workspace_root(&extract_dir, original).await?,
    };
    Ok(LoadedWorkspaceSource {
        staged: StagedWorkspace::temporary(root, tempdir),
        fingerprint,
        immutable: false,
    })
}

async fn probe_git_repo(
    uri: &SourceUri,
    original: &str,
    options: &SourceOptions,
    previous: Option<&SourceFingerprint>,
) -> Result<SourceProbe> {
    let Some(ref_) = uri.ref_.as_deref() else {
        return Ok(SourceProbe::Unknown);
    };
    if is_full_git_commit(ref_) {
        return Ok(SourceProbe::ImmutablePinned(SourceFingerprint::GitCommit(
            ref_.to_owned(),
        )));
    }
    let commit = git_ls_remote(uri, original, options).await?;
    let fingerprint = SourceFingerprint::GitCommit(commit);
    if previous == Some(&fingerprint) {
        Ok(SourceProbe::Unchanged)
    } else {
        Ok(SourceProbe::Changed(Some(fingerprint)))
    }
}

async fn probe_https_archive(
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
    let client = reqwest::Client::builder()
        .timeout(options.http_timeout())
        .build()
        .map_err(|err| RototoError::new(format!("failed to build HTTP client: {err}")))?;
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

async fn git_rev_parse_head(repo: &Path, options: &SourceOptions) -> Result<String> {
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command
        .current_dir(repo)
        .arg("rev-parse")
        .arg("HEAD")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new("git rev-parse timed out for workspace source"))?
        .map_err(|err| RototoError::new(format!("failed to run git: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git rev-parse failed for workspace source: {}",
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn git_checkout(repo: &Path, ref_: &str, options: &SourceOptions) -> Result<()> {
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command
        .current_dir(repo)
        .arg("checkout")
        .arg("--quiet")
        .arg(ref_)
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new("git checkout timed out for workspace source"))?
        .map_err(|err| RototoError::new(format!("failed to run git: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git checkout failed for workspace source: {}",
            stderr.trim()
        )));
    }
    Ok(())
}

async fn git_ls_remote(uri: &SourceUri, original: &str, options: &SourceOptions) -> Result<String> {
    let inner_scheme = uri
        .scheme
        .strip_prefix("git+")
        .ok_or_else(|| RototoError::new("invalid git workspace source"))?;
    let clone_url = format!("{inner_scheme}://{}", uri.base);
    let ref_ = uri
        .ref_
        .as_deref()
        .ok_or_else(|| RototoError::new("git workspace source has no ref"))?;
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command.arg("ls-remote").arg(&clone_url).arg(ref_);
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| {
            RototoError::new(format!(
                "git check timed out for workspace source: {original}"
            ))
        })?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git check failed for workspace source: {}",
            stderr.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .ok_or_else(|| RototoError::new(format!("git ref `{ref_}` was not found in `{original}`")))
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

fn is_full_git_commit(ref_: &str) -> bool {
    ref_.len() == 40 && ref_.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn content_hash_fingerprint(path: &Path) -> Result<SourceFingerprint> {
    let bytes = tokio::fs::read(path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to read workspace archive {}: {err}",
            path.display()
        ))
    })?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(SourceFingerprint::ContentHash(format!(
        "{:016x}",
        hasher.finish()
    )))
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

fn extract_archive(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path).map_err(|err| {
        RototoError::new(format!(
            "failed to open workspace archive {}: {err}",
            archive_path.display()
        ))
    })?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .map_err(|err| RototoError::new(format!("failed to read workspace archive: {err}")))?
    {
        let mut entry = entry
            .map_err(|err| RototoError::new(format!("failed to read archive entry: {err}")))?;
        let path = entry
            .path()
            .map_err(|err| RototoError::new(format!("archive entry path is invalid: {err}")))?;
        if !archive_path_is_safe(&path) {
            return Err(RototoError::new(format!(
                "workspace archive contains unsafe path: {}",
                path.display()
            )));
        }
        let entry_type = entry.header().entry_type();
        if !(entry_type.is_file() || entry_type.is_dir()) {
            return Err(RototoError::new(format!(
                "workspace archive contains unsupported entry type at: {}",
                path.display()
            )));
        }
        entry.unpack_in(extract_dir).map_err(|err| {
            RototoError::new(format!("failed to extract workspace archive: {err}"))
        })?;
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    let metadata = std::fs::metadata(source).map_err(|err| {
        RototoError::new(format!(
            "failed to inspect workspace {}: {err}",
            source.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "workspace source is not a directory: {}",
            source.display()
        )));
    }
    std::fs::create_dir_all(target).map_err(|err| {
        RototoError::new(format!(
            "failed to create workspace snapshot {}: {err}",
            target.display()
        ))
    })?;
    for entry in std::fs::read_dir(source).map_err(|err| {
        RototoError::new(format!(
            "failed to read workspace directory {}: {err}",
            source.display()
        ))
    })? {
        let entry = entry
            .map_err(|err| RototoError::new(format!("failed to read workspace entry: {err}")))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata().map_err(|err| {
            RototoError::new(format!(
                "failed to inspect workspace entry {}: {err}",
                source_path.display()
            ))
        })?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if metadata.is_file() {
            std::fs::copy(&source_path, &target_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to copy workspace entry {}: {err}",
                    source_path.display()
                ))
            })?;
        } else {
            return Err(RototoError::new(format!(
                "workspace snapshot contains unsupported entry type: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
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

async fn async_is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
}

async fn select_subdir(root: &Path, subdir: Option<&str>, original: &str) -> Result<PathBuf> {
    let Some(subdir) = subdir else {
        return Ok(root.to_path_buf());
    };
    if !relative_path_is_safe(Path::new(subdir)) {
        return Err(RototoError::new(format!(
            "workspace source subdir is unsafe: {subdir}"
        )));
    }
    let target = root.join(subdir);
    let metadata = tokio::fs::metadata(&target).await.map_err(|_| {
        RototoError::new(format!(
            "workspace source subdir `{subdir}` was not found in `{original}`"
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "workspace source subdir `{subdir}` is not a directory"
        )));
    }
    Ok(target)
}

fn archive_path_is_safe(path: &Path) -> bool {
    relative_path_is_safe(path)
}

fn relative_path_is_safe(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

impl SourceUri {
    fn parse(source: &str) -> Result<Option<Self>> {
        let Some((scheme, rest)) = source.split_once("://") else {
            return Ok(None);
        };
        if scheme.is_empty() || rest.is_empty() {
            return Err(RototoError::new(format!(
                "workspace source URI is invalid: {source}"
            )));
        }
        let (base, fragment) = match rest.split_once('#') {
            Some((base, fragment)) => (base, Some(fragment)),
            None => (rest, None),
        };
        if base.is_empty() {
            return Err(RototoError::new(format!(
                "workspace source URI is invalid: {source}"
            )));
        }
        let (ref_, subdir) = match fragment {
            Some(fragment) => match fragment.split_once(':') {
                Some((ref_, subdir)) => (
                    (!ref_.is_empty()).then(|| ref_.to_owned()),
                    (!subdir.is_empty()).then(|| subdir.to_owned()),
                ),
                None => ((!fragment.is_empty()).then(|| fragment.to_owned()), None),
            },
            None => (None, None),
        };
        Ok(Some(Self {
            scheme: scheme.to_ascii_lowercase(),
            base: base.to_owned(),
            ref_,
            subdir,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

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

    #[test]
    fn source_uri_rejects_malformed_uris() {
        assert!(SourceUri::parse("examples/basic").unwrap().is_none());
        assert!(SourceUri::parse("://example.com/workspace.tar.gz").is_err());
        assert!(SourceUri::parse("https://").is_err());
        assert!(SourceUri::parse("https://#main").is_err());
    }

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

        let err = extract_archive(&archive_path, &temp.path().join("extract")).unwrap_err();

        assert!(err.to_string().contains("unsupported entry type"));
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

    #[test]
    fn full_git_commit_detection_requires_forty_hex_characters() {
        assert!(is_full_git_commit(
            "0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(!is_full_git_commit("main"));
        assert!(!is_full_git_commit(
            "0123456789abcdef0123456789abcdef0123456g"
        ));
    }
}
