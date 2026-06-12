use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::{Result, RototoError};
use crate::workspace::workspace_extends_sources;

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
const DEFAULT_MAX_DECOMPRESSED_ARCHIVE_BYTES: u64 = 200 * 1024 * 1024;
const DEFAULT_MAX_ARCHIVE_ENTRIES: u64 = 10_000;
const ERROR_BODY_PREVIEW_BYTES: u64 = 4096;
const MAX_WORKSPACE_EXTENDS_DEPTH: usize = 32;
const HTTP_USER_AGENT: &str = concat!("rototo/", env!("CARGO_PKG_VERSION"));

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
    staged: StagedWorkspace,
    fingerprint: Option<SourceFingerprint>,
    immutable: bool,
    layers: Vec<SourceLayer>,
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
    source: String,
    fingerprint: Option<SourceFingerprint>,
    immutable: bool,
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
enum LocalStageMode {
    Borrow,
    Snapshot,
}

#[derive(Clone, Copy, Debug)]
struct ExtendSourceBase<'a> {
    path: &'a Path,
    temporary: bool,
}

#[derive(Debug)]
struct ResolvedExtendSource {
    source: String,
    inherited_temporary_base: bool,
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

async fn load_single_workspace_source(
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

async fn load_single_workspace_source_snapshot(
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

async fn stage_local_path(path: &Path) -> Result<StagedWorkspace> {
    Ok(StagedWorkspace::local(path.to_path_buf()))
}

async fn snapshot_local_path(path: &Path) -> Result<LoadedWorkspaceSource> {
    let source_label = path.to_string_lossy().into_owned();
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
        layers: vec![SourceLayer {
            source: source_label,
            fingerprint: None,
            immutable: false,
        }],
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
    if let Some(ref_) = uri.ref_.as_deref() {
        validate_git_ref(ref_)?;
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
    scrub_git_process_variables(&mut command);

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
        layers: vec![SourceLayer {
            source: original.to_owned(),
            fingerprint: Some(SourceFingerprint::GitCommit(commit)),
            immutable: pinned_commit,
        }],
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

fn load_workspace_source_graph<'a>(
    source: &'a str,
    options: &'a SourceOptions,
    local_mode: LocalStageMode,
    base: Option<ExtendSourceBase<'a>>,
    stack: &'a mut Vec<String>,
) -> Pin<Box<dyn Future<Output = Result<LoadedWorkspaceSource>> + Send + 'a>> {
    Box::pin(async move {
        if stack.len() >= MAX_WORKSPACE_EXTENDS_DEPTH {
            return Err(RototoError::new(format!(
                "workspace extends depth exceeded {MAX_WORKSPACE_EXTENDS_DEPTH}"
            )));
        }

        let resolved_source = resolve_extend_source(source, base)?;
        let loaded = match local_mode {
            LocalStageMode::Borrow => {
                load_single_workspace_source(&resolved_source.source, options).await?
            }
            LocalStageMode::Snapshot => {
                load_single_workspace_source_snapshot(&resolved_source.source, options).await?
            }
        };
        let layer_key = workspace_source_key(&resolved_source.source, loaded.staged()).await?;
        if let Some(cycle_start) = stack.iter().position(|key| key == &layer_key) {
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(layer_key);
            return Err(RototoError::new(format!(
                "workspace extends cycle detected: {}",
                cycle.join(" -> ")
            )));
        }

        stack.push(layer_key);
        let result = project_workspace_source_graph(
            loaded,
            options,
            local_mode,
            resolved_source.inherited_temporary_base,
            stack,
        )
        .await;
        stack.pop();
        result
    })
}

async fn project_workspace_source_graph(
    loaded: LoadedWorkspaceSource,
    options: &SourceOptions,
    local_mode: LocalStageMode,
    inherited_temporary_base: bool,
    stack: &mut Vec<String>,
) -> Result<LoadedWorkspaceSource> {
    let extends = read_workspace_extends(loaded.staged().path()).await?;
    if extends.is_empty() {
        return Ok(loaded);
    }

    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let target = tempdir.path().join("workspace");
    let base_path = extend_source_base_path(&loaded);
    let base = ExtendSourceBase {
        path: &base_path,
        temporary: inherited_temporary_base
            || (loaded.staged().is_temporary() && base_path == loaded.staged().path()),
    };
    let mut layers = Vec::new();
    let mut immutable = true;
    for parent_source in &extends {
        let parent =
            load_workspace_source_graph(parent_source, options, local_mode, Some(base), stack)
                .await?;
        copy_workspace_layer(parent.staged().path(), &target, false).await?;
        immutable &= parent.immutable();
        layers.extend(parent.layers().iter().cloned());
    }

    copy_workspace_layer(loaded.staged().path(), &target, true).await?;
    immutable &= loaded.immutable();
    layers.extend(loaded.layers().iter().cloned());
    let fingerprint = combined_layer_fingerprint(&layers);
    Ok(LoadedWorkspaceSource {
        staged: StagedWorkspace::temporary(target, tempdir),
        fingerprint,
        immutable,
        layers,
    })
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

async fn git_rev_parse_head(repo: &Path, options: &SourceOptions) -> Result<String> {
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command.current_dir(repo).arg("rev-parse").arg("HEAD");
    scrub_git_process_variables(&mut command);
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
    validate_git_ref(ref_)?;
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command
        .current_dir(repo)
        .arg("checkout")
        .arg("--quiet")
        .arg(ref_);
    scrub_git_process_variables(&mut command);
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
    validate_git_ref(ref_)?;
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    command.arg("ls-remote").arg(&clone_url).arg("--").arg(ref_);
    scrub_git_process_variables(&mut command);
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

fn validate_git_ref(ref_: &str) -> Result<()> {
    if ref_.starts_with('-') {
        return Err(RototoError::new(format!(
            "git workspace ref must not begin with '-': {ref_}"
        )));
    }
    Ok(())
}

fn scrub_git_process_variables(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    for key in [
        "GIT_INDEX_FILE",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
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

fn https_archive_client(options: &SourceOptions) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(options.http_timeout())
        .redirect(https_only_redirect_policy())
        .user_agent(HTTP_USER_AGENT)
        .build()
        .map_err(|err| RototoError::new(format!("failed to build HTTP client: {err}")))
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

async fn copy_workspace_layer(source: &Path, target: &Path, include_manifest: bool) -> Result<()> {
    let source = source.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || {
        copy_workspace_layer_recursive(&source, &target, include_manifest, true)
    })
    .await
    .map_err(|err| RototoError::new(format!("workspace layer copy task failed: {err}")))?
}

fn copy_workspace_layer_recursive(
    source: &Path,
    target: &Path,
    include_manifest: bool,
    root: bool,
) -> Result<()> {
    let metadata = std::fs::metadata(source).map_err(|err| {
        RototoError::new(format!(
            "failed to inspect workspace layer {}: {err}",
            source.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "workspace layer source is not a directory: {}",
            source.display()
        )));
    }
    std::fs::create_dir_all(target).map_err(|err| {
        RototoError::new(format!(
            "failed to create workspace projection {}: {err}",
            target.display()
        ))
    })?;
    for entry in std::fs::read_dir(source).map_err(|err| {
        RototoError::new(format!(
            "failed to read workspace layer {}: {err}",
            source.display()
        ))
    })? {
        let entry = entry.map_err(|err| {
            RototoError::new(format!("failed to read workspace layer entry: {err}"))
        })?;
        let file_name = entry.file_name();
        if root && !include_manifest && file_name == WORKSPACE_MANIFEST {
            continue;
        }
        let source_path = entry.path();
        let target_path = target.join(&file_name);
        let metadata = entry.metadata().map_err(|err| {
            RototoError::new(format!(
                "failed to inspect workspace layer entry {}: {err}",
                source_path.display()
            ))
        })?;
        if metadata.is_dir() {
            if target_path.is_file() {
                std::fs::remove_file(&target_path).map_err(|err| {
                    RototoError::new(format!(
                        "failed to replace projected workspace file {}: {err}",
                        target_path.display()
                    ))
                })?;
            }
            copy_workspace_layer_recursive(&source_path, &target_path, include_manifest, false)?;
        } else if metadata.is_file() {
            if target_path.is_dir() {
                std::fs::remove_dir_all(&target_path).map_err(|err| {
                    RototoError::new(format!(
                        "failed to replace projected workspace directory {}: {err}",
                        target_path.display()
                    ))
                })?;
            }
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    RototoError::new(format!(
                        "failed to create projected workspace directory {}: {err}",
                        parent.display()
                    ))
                })?;
            }
            std::fs::copy(&source_path, &target_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to copy workspace layer entry {}: {err}",
                    source_path.display()
                ))
            })?;
        } else {
            return Err(RototoError::new(format!(
                "workspace layer contains unsupported entry type: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

async fn read_workspace_extends(root: &Path) -> Result<Vec<String>> {
    let path = root.join(WORKSPACE_MANIFEST);
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(_) => return Ok(Vec::new()),
    };
    let manifest = text.parse::<toml::Value>().map_err(|err| {
        RototoError::new(format!(
            "failed to parse workspace manifest {}: {err}",
            path.display()
        ))
    })?;
    workspace_extends_sources(&manifest)
}

fn resolve_extend_source(
    source: &str,
    base: Option<ExtendSourceBase<'_>>,
) -> Result<ResolvedExtendSource> {
    let uri = SourceUri::parse(source)?;
    if let Some(base) = base
        && base.temporary
    {
        if let Some(uri) = uri.as_ref() {
            if workspace_source_uri_is_local_filesystem(uri) {
                return Err(RototoError::new(format!(
                    "workspace extends source escapes a staged workspace: {source}"
                )));
            }
            return Ok(ResolvedExtendSource {
                source: source.to_owned(),
                inherited_temporary_base: false,
            });
        }
        if Path::new(source).is_absolute() || !relative_path_is_safe(Path::new(source)) {
            return Err(RototoError::new(format!(
                "relative workspace extends source escapes a staged workspace: {source}"
            )));
        }
        return Ok(ResolvedExtendSource {
            source: base.path.join(source).to_string_lossy().into_owned(),
            inherited_temporary_base: true,
        });
    }
    if uri.is_some() || Path::new(source).is_absolute() {
        return Ok(ResolvedExtendSource {
            source: source.to_owned(),
            inherited_temporary_base: false,
        });
    }
    let Some(base) = base else {
        return Ok(ResolvedExtendSource {
            source: source.to_owned(),
            inherited_temporary_base: false,
        });
    };
    Ok(ResolvedExtendSource {
        source: base.path.join(source).to_string_lossy().into_owned(),
        inherited_temporary_base: false,
    })
}

async fn workspace_source_key(source: &str, staged: &StagedWorkspace) -> Result<String> {
    if SourceUri::parse(source)?.is_some() {
        return Ok(source.to_owned());
    }
    let path = if source.is_empty() {
        staged.path()
    } else {
        Path::new(source)
    };
    tokio::fs::canonicalize(path)
        .await
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|err| {
            RototoError::new(format!(
                "failed to canonicalize workspace source {}: {err}",
                path.display()
            ))
        })
}

fn extend_source_base_path(loaded: &LoadedWorkspaceSource) -> PathBuf {
    if loaded.staged().is_temporary()
        && let [layer] = loaded.layers()
        && SourceUri::parse(layer.source()).ok().flatten().is_none()
    {
        return PathBuf::from(layer.source());
    }
    loaded.staged().path().to_path_buf()
}

fn combined_layer_fingerprint(layers: &[SourceLayer]) -> Option<SourceFingerprint> {
    let mut fingerprints = Vec::with_capacity(layers.len());
    for layer in layers {
        fingerprints.push(layer.fingerprint.clone()?);
    }
    match fingerprints.len() {
        0 => None,
        1 => fingerprints.pop(),
        _ => Some(SourceFingerprint::WorkspaceLayers(fingerprints)),
    }
}

fn workspace_source_uri_is_local_filesystem(uri: &SourceUri) -> bool {
    matches!(uri.scheme.as_str(), "file" | "git+file")
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

async fn async_is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
}

async fn select_subdir(root: &Path, subdir: Option<&str>, original: &str) -> Result<PathBuf> {
    let canonical_root = tokio::fs::canonicalize(root).await.map_err(|err| {
        RototoError::new(format!(
            "failed to canonicalize staged workspace root {}: {err}",
            root.display()
        ))
    })?;
    let Some(subdir) = subdir else {
        return Ok(canonical_root);
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
    let canonical_target = tokio::fs::canonicalize(&target).await.map_err(|err| {
        RototoError::new(format!(
            "failed to canonicalize workspace source subdir `{subdir}`: {err}"
        ))
    })?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(RototoError::new(format!(
            "workspace source subdir `{subdir}` escapes staged workspace"
        )));
    }
    Ok(canonical_target)
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

    #[test]
    fn source_uri_rejects_malformed_uris() {
        assert!(SourceUri::parse("examples/basic").unwrap().is_none());
        assert!(SourceUri::parse("://example.com/workspace.tar.gz").is_err());
        assert!(SourceUri::parse("https://").is_err());
        assert!(SourceUri::parse("https://#main").is_err());
    }

    #[test]
    fn staged_extend_base_rejects_local_filesystem_escape_sources() {
        let staged = tempfile::TempDir::new().unwrap();
        let base = ExtendSourceBase {
            path: staged.path(),
            temporary: true,
        };

        for source in [
            "/tmp/outside",
            "../outside",
            "file:///tmp/outside",
            "git+file:///tmp/outside.git",
        ] {
            let err = resolve_extend_source(source, Some(base)).unwrap_err();
            assert!(err.to_string().contains("escapes a staged workspace"));
        }

        let resolved = resolve_extend_source("parent", Some(base)).unwrap();
        assert_eq!(
            resolved.source,
            staged.path().join("parent").display().to_string()
        );
        assert!(resolved.inherited_temporary_base);
    }

    #[tokio::test]
    async fn read_workspace_extends_rejects_blank_sources() {
        let temp = tempfile::TempDir::new().unwrap();
        tokio::fs::write(
            temp.path().join(WORKSPACE_MANIFEST),
            r#"schema_version = 1
extends = ["../base", "  "]
"#,
        )
        .await
        .unwrap();

        let err = read_workspace_extends(temp.path()).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("workspace extends source must not be blank")
        );
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

    #[tokio::test]
    async fn parent_layer_copy_skips_only_root_manifest() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        tokio::fs::create_dir_all(source.join("resources/config-objects"))
            .await
            .unwrap();
        tokio::fs::write(source.join(WORKSPACE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();
        tokio::fs::write(
            source
                .join("resources/config-objects")
                .join(WORKSPACE_MANIFEST),
            "value = true\n",
        )
        .await
        .unwrap();

        copy_workspace_layer(&source, &target, false).await.unwrap();

        assert!(!target.join(WORKSPACE_MANIFEST).exists());
        assert!(
            target
                .join("resources/config-objects")
                .join(WORKSPACE_MANIFEST)
                .is_file()
        );
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
