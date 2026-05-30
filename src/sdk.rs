use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use jsonschema::Validator;
use serde_json::Value as JsonValue;
use tokio::sync::{Mutex, RwLock, watch};
use tokio::task::JoinHandle;

use crate::error::{Result, RototoError};
use crate::model::{QualifierResolution, VariableResolution, WorkspaceInspection, WorkspaceLint};
use crate::source::{
    SourceAuth, SourceFingerprint, SourceOptions, SourceProbe, StagedWorkspace,
    load_workspace_source, load_workspace_source_snapshot, probe_workspace_source,
};
use crate::workspace::{inspect_workspace, read_toml};

#[derive(Debug)]
pub struct Workspace {
    staged: StagedWorkspace,
    inspection: WorkspaceInspection,
    context_schema: Option<JsonValue>,
    context_validator: Option<Validator>,
    source_fingerprint: Option<SourceFingerprint>,
    immutable_source: bool,
}

impl Workspace {
    pub async fn load(source: impl AsRef<str>) -> Result<Self> {
        Self::load_with_options(source, LoadOptions::default()).await
    }

    pub async fn load_with_options(source: impl AsRef<str>, options: LoadOptions) -> Result<Self> {
        let workspace = Self::stage_and_inspect(source, options.source()).await?;
        if options.lint() == LintMode::Deny {
            let lint = crate::lint_workspace(workspace.root()).await?;
            if !lint.diagnostics.is_empty() {
                return Err(RototoError::new(format!(
                    "workspace lint failed with {} diagnostic(s)",
                    lint.diagnostics.len()
                )));
            }
        }
        Ok(workspace)
    }

    async fn load_snapshot_with_options(
        source: impl AsRef<str>,
        options: LoadOptions,
    ) -> Result<Self> {
        let workspace = Self::stage_snapshot_and_inspect(source, options.source()).await?;
        if options.lint() == LintMode::Deny {
            let lint = crate::lint_workspace(workspace.root()).await?;
            if !lint.diagnostics.is_empty() {
                return Err(RototoError::new(format!(
                    "workspace lint failed with {} diagnostic(s)",
                    lint.diagnostics.len()
                )));
            }
        }
        Ok(workspace)
    }

    pub async fn inspect(source: impl AsRef<str>) -> Result<Self> {
        Self::inspect_with_source_options(source, &SourceOptions::default()).await
    }

    pub async fn inspect_with_source_options(
        source: impl AsRef<str>,
        options: &SourceOptions,
    ) -> Result<Self> {
        Self::stage_and_inspect(source, options).await
    }

    async fn stage_and_inspect(source: impl AsRef<str>, options: &SourceOptions) -> Result<Self> {
        let loaded = load_workspace_source(source, options).await?;
        Self::inspect_loaded(loaded).await
    }

    async fn stage_snapshot_and_inspect(
        source: impl AsRef<str>,
        options: &SourceOptions,
    ) -> Result<Self> {
        let loaded = load_workspace_source_snapshot(source, options).await?;
        Self::inspect_loaded(loaded).await
    }

    async fn inspect_loaded(loaded: crate::source::LoadedWorkspaceSource) -> Result<Self> {
        let source_fingerprint = loaded.fingerprint().cloned();
        let immutable_source = loaded.immutable();
        let staged = loaded.into_staged();
        let root = staged.path().to_path_buf();

        let inspection = inspect_workspace(&root).await?;
        let context_schema = read_context_schema(&root).await?;
        let context_validator = context_schema
            .as_ref()
            .map(jsonschema::validator_for)
            .transpose()
            .map_err(|err| RototoError::new(format!("context schema is invalid: {err}")))?;

        Ok(Self {
            staged,
            inspection,
            context_schema,
            context_validator,
            source_fingerprint,
            immutable_source,
        })
    }

    pub fn root(&self) -> &Path {
        self.staged.path()
    }

    pub fn inspection(&self) -> &WorkspaceInspection {
        &self.inspection
    }

    pub fn context_schema(&self) -> Option<&JsonValue> {
        self.context_schema.as_ref()
    }

    pub fn source_fingerprint(&self) -> Option<&SourceFingerprint> {
        self.source_fingerprint.as_ref()
    }

    pub fn immutable_source(&self) -> bool {
        self.immutable_source
    }

    pub async fn lint(&self) -> Result<WorkspaceLint> {
        crate::lint_workspace(self.root()).await
    }

    pub async fn validate_context(&self, context: &ResolveContext) -> Result<()> {
        let Some(validator) = &self.context_validator else {
            return Ok(());
        };
        validator.validate(context.value()).map_err(|err| {
            RototoError::new(format!("resolve context does not match schema: {err}"))
        })
    }

    pub async fn resolve_qualifier(
        &self,
        id: impl AsRef<str>,
        context: &ResolveContext,
    ) -> Result<QualifierResolution> {
        self.resolve_qualifier_with_options(id, context, ResolveOptions::default())
            .await
    }

    pub async fn resolve_qualifier_with_options(
        &self,
        id: impl AsRef<str>,
        context: &ResolveContext,
        options: ResolveOptions,
    ) -> Result<QualifierResolution> {
        if options.validate_context {
            self.validate_context(context).await?;
        }
        crate::resolve::resolve_qualifier_unchecked(&self.inspection, id.as_ref(), context.value())
            .await
    }

    pub async fn resolve_variable(
        &self,
        id: impl AsRef<str>,
        environment: &Environment,
        context: &ResolveContext,
    ) -> Result<VariableResolution> {
        self.resolve_variable_with_options(id, environment, context, ResolveOptions::default())
            .await
    }

    pub async fn resolve_variable_with_options(
        &self,
        id: impl AsRef<str>,
        environment: &Environment,
        context: &ResolveContext,
        options: ResolveOptions,
    ) -> Result<VariableResolution> {
        if !self
            .inspection
            .environments
            .iter()
            .any(|known| known == environment.name())
        {
            return Err(RototoError::new(format!(
                "unknown environment: {}",
                environment.name()
            )));
        }
        if options.validate_context {
            self.validate_context(context).await?;
        }
        crate::resolve::resolve_variable_unchecked(
            &self.inspection,
            id.as_ref(),
            environment.name(),
            context.value(),
        )
        .await
    }
}

pub struct RefreshingWorkspace {
    source: String,
    load_options: LoadOptions,
    refresh_options: RefreshOptions,
    state: RefreshState,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct RefreshState {
    current: Arc<RwLock<Arc<Workspace>>>,
    status: Arc<RwLock<RefreshStatus>>,
    refresh_lock: Arc<Mutex<()>>,
}

impl RefreshingWorkspace {
    pub async fn load(source: impl AsRef<str>, refresh: RefreshOptions) -> Result<Self> {
        Self::load_with_options(source, LoadOptions::default(), refresh).await
    }

    pub async fn load_with_options(
        source: impl AsRef<str>,
        load_options: LoadOptions,
        refresh_options: RefreshOptions,
    ) -> Result<Self> {
        let source = source.as_ref().to_owned();
        let workspace =
            Arc::new(Workspace::load_snapshot_with_options(&source, load_options.clone()).await?);
        let immutable = workspace.immutable_source();
        let status = Arc::new(RwLock::new(RefreshStatus {
            current_fingerprint: workspace.source_fingerprint().cloned(),
            last_success: Some(SystemTime::now()),
            last_attempt: None,
            consecutive_failures: 0,
            last_error: None,
            refreshing: false,
            immutable,
        }));
        if immutable && refresh_options.period().is_some() {
            tracing::warn!(
                source = %redacted_source(&source),
                "workspace source is pinned to an immutable commit; periodic refresh is disabled"
            );
        }
        let state = RefreshState {
            current: Arc::new(RwLock::new(workspace)),
            status: status.clone(),
            refresh_lock: Arc::new(Mutex::new(())),
        };
        let (shutdown, receiver) = watch::channel(false);
        let task = refresh_options.period().and_then(|period| {
            (!immutable).then(|| {
                spawn_refresh_loop(
                    source.clone(),
                    load_options.clone(),
                    refresh_options.clone(),
                    period,
                    state.clone(),
                    receiver,
                )
            })
        });

        Ok(Self {
            source,
            load_options,
            refresh_options,
            state,
            shutdown,
            task,
        })
    }

    pub async fn current(&self) -> Arc<Workspace> {
        self.state.current.read().await.clone()
    }

    pub async fn status(&self) -> RefreshStatus {
        self.state.status.read().await.clone()
    }

    pub async fn refresh_now(&self) -> Result<RefreshOutcome> {
        refresh_once(&self.source, &self.load_options, &self.state).await
    }

    pub async fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    pub async fn resolve_qualifier(
        &self,
        id: impl AsRef<str>,
        context: &ResolveContext,
    ) -> Result<QualifierResolution> {
        self.current()
            .await
            .resolve_qualifier(id.as_ref(), context)
            .await
    }

    pub async fn resolve_qualifier_with_options(
        &self,
        id: impl AsRef<str>,
        context: &ResolveContext,
        options: ResolveOptions,
    ) -> Result<QualifierResolution> {
        self.current()
            .await
            .resolve_qualifier_with_options(id.as_ref(), context, options)
            .await
    }

    pub async fn resolve_variable(
        &self,
        id: impl AsRef<str>,
        environment: &Environment,
        context: &ResolveContext,
    ) -> Result<VariableResolution> {
        self.current()
            .await
            .resolve_variable(id.as_ref(), environment, context)
            .await
    }

    pub async fn resolve_variable_with_options(
        &self,
        id: impl AsRef<str>,
        environment: &Environment,
        context: &ResolveContext,
        options: ResolveOptions,
    ) -> Result<VariableResolution> {
        self.current()
            .await
            .resolve_variable_with_options(id.as_ref(), environment, context, options)
            .await
    }

    pub fn refresh_options(&self) -> &RefreshOptions {
        &self.refresh_options
    }
}

impl Drop for RefreshingWorkspace {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefreshOptions {
    period: Option<Duration>,
    max_staleness: Option<Duration>,
    min_failure_backoff: Duration,
    max_failure_backoff: Duration,
}

impl RefreshOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn period(&self) -> Option<Duration> {
        self.period
    }

    pub fn max_staleness(&self) -> Option<Duration> {
        self.max_staleness
    }

    pub fn min_failure_backoff(&self) -> Duration {
        self.min_failure_backoff
    }

    pub fn max_failure_backoff(&self) -> Duration {
        self.max_failure_backoff
    }

    pub fn with_period(mut self, period: Duration) -> Self {
        self.period = Some(period);
        self
    }

    pub fn with_max_staleness(mut self, max_staleness: Duration) -> Self {
        self.max_staleness = Some(max_staleness);
        self
    }

    pub fn with_failure_backoff(mut self, min: Duration, max: Duration) -> Self {
        self.min_failure_backoff = min;
        self.max_failure_backoff = max;
        self
    }
}

impl Default for RefreshOptions {
    fn default() -> Self {
        Self {
            period: None,
            max_staleness: None,
            min_failure_backoff: Duration::from_secs(5),
            max_failure_backoff: Duration::from_secs(300),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefreshStatus {
    pub current_fingerprint: Option<SourceFingerprint>,
    pub last_success: Option<SystemTime>,
    pub last_attempt: Option<SystemTime>,
    pub consecutive_failures: u64,
    pub last_error: Option<String>,
    pub refreshing: bool,
    pub immutable: bool,
}

impl RefreshStatus {
    pub fn stale(&self, max_staleness: Duration) -> bool {
        self.last_success
            .and_then(|last_success| last_success.elapsed().ok())
            .is_some_and(|age| age > max_staleness)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshOutcome {
    Unchanged,
    Refreshed,
    Immutable,
}

fn spawn_refresh_loop(
    source: String,
    load_options: LoadOptions,
    refresh_options: RefreshOptions,
    period: Duration,
    state: RefreshState,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(period) => {}
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                    continue;
                }
            }

            let outcome = refresh_once(&source, &load_options, &state).await;
            if let Err(err) = &outcome {
                let delay = failure_backoff(
                    state.status.read().await.consecutive_failures,
                    &refresh_options,
                );
                tracing::warn!(
                    source = %redacted_source(&source),
                    error = %err,
                    backoff_ms = delay.as_millis(),
                    "workspace refresh failed; continuing to serve last known good workspace"
                );
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        }
    })
}

async fn refresh_once(
    source: &str,
    load_options: &LoadOptions,
    state: &RefreshState,
) -> Result<RefreshOutcome> {
    let _guard = state.refresh_lock.lock().await;
    {
        let mut status = state.status.write().await;
        status.last_attempt = Some(SystemTime::now());
        status.refreshing = true;
    }
    let result = refresh_once_inner(source, load_options, state).await;
    {
        let mut status = state.status.write().await;
        status.refreshing = false;
        if let Err(err) = &result {
            status.consecutive_failures = status.consecutive_failures.saturating_add(1);
            status.last_error = Some(err.to_string());
        }
    }
    result
}

async fn refresh_once_inner(
    source: &str,
    load_options: &LoadOptions,
    state: &RefreshState,
) -> Result<RefreshOutcome> {
    let previous = state.current.read().await.source_fingerprint().cloned();
    match probe_workspace_source(source, load_options.source(), previous.as_ref()).await? {
        SourceProbe::Unchanged => {
            tracing::debug!(source = %redacted_source(source), "workspace source is unchanged");
            return Ok(RefreshOutcome::Unchanged);
        }
        SourceProbe::ImmutablePinned(fingerprint) => {
            tracing::warn!(
                source = %redacted_source(source),
                "workspace source is pinned to an immutable commit; periodic refresh is disabled"
            );
            let mut status = state.status.write().await;
            status.current_fingerprint = Some(fingerprint);
            status.immutable = true;
            return Ok(RefreshOutcome::Immutable);
        }
        SourceProbe::Changed(_) => {
            tracing::info!(source = %redacted_source(source), "workspace source changed");
        }
        SourceProbe::Unknown => {
            tracing::debug!(
                source = %redacted_source(source),
                "workspace source change status is unknown; attempting refresh"
            );
        }
    }

    let workspace =
        Arc::new(Workspace::load_snapshot_with_options(source, load_options.clone()).await?);
    let fingerprint = workspace.source_fingerprint().cloned();
    let immutable = workspace.immutable_source();
    {
        let mut current = state.current.write().await;
        *current = workspace;
    }
    {
        let mut status = state.status.write().await;
        status.current_fingerprint = fingerprint;
        status.last_success = Some(SystemTime::now());
        status.consecutive_failures = 0;
        status.last_error = None;
        status.immutable = immutable;
    }
    tracing::info!(source = %redacted_source(source), "workspace refresh succeeded");
    Ok(RefreshOutcome::Refreshed)
}

fn failure_backoff(failures: u64, options: &RefreshOptions) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    let shift = failures.saturating_sub(1).min(20) as u32;
    let multiplier = 1_u32.checked_shl(shift).unwrap_or(u32::MAX);
    options
        .min_failure_backoff()
        .saturating_mul(multiplier)
        .min(options.max_failure_backoff())
}

fn redacted_source(source: &str) -> String {
    match source.split_once("://") {
        Some((scheme, rest)) if rest.contains('@') => {
            let host = rest.rsplit_once('@').map(|(_, host)| host).unwrap_or(rest);
            format!("{scheme}://<redacted>@{host}")
        }
        _ => source.to_owned(),
    }
}

#[derive(Clone, Debug)]
pub struct LoadOptions {
    lint: LintMode,
    source: SourceOptions,
}

impl LoadOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lint(&self) -> LintMode {
        self.lint
    }

    pub fn source(&self) -> &SourceOptions {
        &self.source
    }

    pub fn with_lint(mut self, lint: LintMode) -> Self {
        self.lint = lint;
        self
    }

    pub fn with_source_auth(mut self, auth: SourceAuth) -> Self {
        self.source = self.source.with_auth(auth);
        self
    }
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            lint: LintMode::Deny,
            source: SourceOptions::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintMode {
    Deny,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveOptions {
    pub validate_context: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            validate_context: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    name: String,
}

impl Environment {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolveContext {
    value: JsonValue,
}

impl ResolveContext {
    pub fn from_json(value: JsonValue) -> Result<Self> {
        if !value.is_object() {
            return Err(RototoError::new("resolve context must be a JSON object"));
        }
        Ok(Self { value })
    }

    pub fn value(&self) -> &JsonValue {
        &self.value
    }
}

async fn read_context_schema(root: &Path) -> Result<Option<JsonValue>> {
    let manifest = read_toml(&root.join("rototo-workspace.toml")).await?;
    let Some(context) = manifest.get("context") else {
        return Ok(None);
    };
    let context = context
        .as_table()
        .ok_or_else(|| RototoError::new("[context] must be a table"))?;
    let schema_ref = context
        .get("schema")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| RototoError::new("[context] must declare schema"))?;
    let path = context_schema_path(root, schema_ref)?;
    let text = tokio::fs::read_to_string(&path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to read context schema {}: {err}",
            path.display()
        ))
    })?;
    let schema = serde_json::from_str(&text).map_err(|err| {
        RototoError::new(format!(
            "failed to parse context schema {}: {err}",
            path.display()
        ))
    })?;
    Ok(Some(schema))
}

fn context_schema_path(root: &Path, schema_ref: &str) -> Result<PathBuf> {
    let schema_ref = Path::new(schema_ref);
    if schema_ref.as_os_str().is_empty()
        || schema_ref.is_absolute()
        || schema_ref
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(RototoError::new(
            "context schema path must be a relative path inside the workspace",
        ));
    }
    Ok(root.join(schema_ref))
}
