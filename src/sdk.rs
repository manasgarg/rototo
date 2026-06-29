use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use serde_json::Value as JsonValue;
use tokio::sync::{Mutex, broadcast, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::error::{Result, RototoError};
use crate::lint::{
    LintInput, RuntimePackage, compile_runtime_package_from_snapshot, lint_package_snapshot,
};
use crate::model::{PackageInspection, PackageLint, VariableResolution};
use crate::package::inspect_package;
use crate::source::{
    SourceAuth, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe, StagedPackage,
    load_package_source, load_package_source_snapshot, probe_package_source,
};

#[derive(Debug)]
pub struct Package {
    staged: StagedPackage,
    inspection: PackageInspection,
    runtime: Option<RuntimePackage>,
    source: String,
    loaded_at: SystemTime,
    source_fingerprint: Option<SourceFingerprint>,
    immutable_source: bool,
    source_layers: Vec<SourceLayer>,
}

impl Package {
    pub async fn load(source: impl AsRef<str>) -> Result<Self> {
        Self::load_with_options(source, LoadOptions::default()).await
    }

    pub async fn load_with_options(source: impl AsRef<str>, options: LoadOptions) -> Result<Self> {
        let mut package = Self::stage_and_inspect(source, options.source()).await?;
        if options.lint() == LintMode::Deny {
            package.compile_runtime_after_lint().await?;
        }
        Ok(package)
    }

    pub(crate) async fn load_snapshot_with_options(
        source: impl AsRef<str>,
        options: LoadOptions,
    ) -> Result<Self> {
        let mut package = Self::stage_snapshot_and_inspect(source, options.source()).await?;
        if options.lint() == LintMode::Deny {
            package.compile_runtime_after_lint().await?;
        }
        Ok(package)
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
        let source = source.as_ref().to_owned();
        let loaded = load_package_source(&source, options).await?;
        Self::inspect_loaded(source, loaded).await
    }

    async fn stage_snapshot_and_inspect(
        source: impl AsRef<str>,
        options: &SourceOptions,
    ) -> Result<Self> {
        let source = source.as_ref().to_owned();
        let loaded = load_package_source_snapshot(&source, options).await?;
        Self::inspect_loaded(source, loaded).await
    }

    async fn inspect_loaded(
        source: String,
        loaded: crate::source::LoadedPackageSource,
    ) -> Result<Self> {
        let source_fingerprint = loaded.fingerprint().cloned();
        let immutable_source = loaded.immutable();
        let source_layers = loaded.layers().to_vec();
        let staged = loaded.into_staged();
        let root = staged.path().to_path_buf();

        let inspection = inspect_package(&root).await?;

        Ok(Self {
            staged,
            inspection,
            runtime: None,
            source,
            loaded_at: SystemTime::now(),
            source_fingerprint,
            immutable_source,
            source_layers,
        })
    }

    async fn compile_runtime_after_lint(&mut self) -> Result<()> {
        let snapshot = lint_package_snapshot(LintInput::new(self.root().to_path_buf())).await?;
        if snapshot.lint.has_errors() {
            return Err(RototoError::new(format!(
                "package lint failed with {} diagnostic(s)",
                snapshot.lint.diagnostics.len()
            )));
        }
        self.runtime = Some(compile_runtime_package_from_snapshot(&snapshot)?);
        Ok(())
    }

    pub fn root(&self) -> &Path {
        self.staged.path()
    }

    pub fn inspection(&self) -> &PackageInspection {
        &self.inspection
    }

    pub fn context_schema(&self) -> Option<&JsonValue> {
        self.runtime
            .as_ref()
            .and_then(|runtime| runtime.evaluation_contexts.values().next())
            .map(|evaluation_context| &evaluation_context.schema)
    }

    pub fn source_fingerprint(&self) -> Option<&SourceFingerprint> {
        self.source_fingerprint.as_ref()
    }

    pub fn immutable_source(&self) -> bool {
        self.immutable_source
    }

    pub fn source_layers(&self) -> &[SourceLayer] {
        &self.source_layers
    }

    /// Time at which this package instance was accepted by the SDK. For an
    /// initial load it is the successful load time; a refreshed package carries
    /// the time the new snapshot was built and became current.
    pub fn loaded_at(&self) -> SystemTime {
        self.loaded_at
    }

    /// Stable, serializable identity of this loaded package: redacted source,
    /// fingerprint, derived release id, load time, immutability, and per-layer
    /// identity for layered packages.
    pub fn identity(&self) -> PackageIdentity {
        PackageIdentity {
            source: RedactedPackageSource::new(&self.source),
            fingerprint: self.source_fingerprint.clone(),
            release_id: self
                .source_fingerprint
                .as_ref()
                .and_then(release_id_from_fingerprint),
            loaded_at: self.loaded_at,
            immutable: self.immutable_source,
            layers: self
                .source_layers
                .iter()
                .map(PackageLayerIdentity::from_layer)
                .collect(),
        }
    }

    pub async fn lint(&self) -> Result<PackageLint> {
        crate::lint_package(self.root()).await
    }

    /// The semantic model of this package: entities, references, and source
    /// ranges as rototo parses the staged files. Tools should consume this
    /// instead of parsing package files themselves.
    pub async fn semantic_model(&self) -> Result<crate::lint::PackageSemanticModel> {
        crate::lint::package_semantic_model(self.root()).await
    }

    pub fn validate_context(&self, context: &EvaluationContext) -> Result<()> {
        self.runtime()?.validate_context(context.value())
    }

    pub fn resolve_qualifier(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
    ) -> Result<bool> {
        self.resolve_qualifier_with_options(id, context, ResolveOptions::default())
    }

    pub fn resolve_qualifier_with_options(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
        options: ResolveOptions,
    ) -> Result<bool> {
        if options.validate_context {
            self.runtime()?
                .validate_context_for_qualifier(id.as_ref(), context.value())?;
        }
        crate::resolve::resolve_qualifier_unchecked(self.runtime()?, id.as_ref(), context.value())
    }

    pub fn resolve_variable(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
    ) -> Result<VariableResolution> {
        self.resolve_variable_with_options(id, context, ResolveOptions::default())
    }

    pub fn resolve_variable_with_options(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
        options: ResolveOptions,
    ) -> Result<VariableResolution> {
        if options.validate_context {
            self.runtime()?
                .validate_context_for_variable(id.as_ref(), context.value())?;
        }
        crate::resolve::resolve_variable_unchecked(self.runtime()?, id.as_ref(), context.value())
    }

    fn runtime(&self) -> Result<&RuntimePackage> {
        self.runtime.as_ref().ok_or_else(|| {
            RototoError::new(
                "package was loaded without a runtime model; use Package::load with lint enabled",
            )
        })
    }
}

pub struct RefreshingPackage {
    source: String,
    load_options: LoadOptions,
    refresh_options: RefreshOptions,
    state: RefreshState,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct RefreshState {
    current: Arc<RwLock<Arc<Package>>>,
    status: Arc<RwLock<RefreshStatus>>,
    refresh_lock: Arc<Mutex<()>>,
    observer: Option<Arc<dyn RefreshObserver>>,
    events: broadcast::Sender<RefreshEvent>,
    last_event: Arc<RwLock<Option<RefreshEventSummary>>>,
}

impl RefreshingPackage {
    pub async fn load(source: impl AsRef<str>, refresh: RefreshOptions) -> Result<Self> {
        Self::load_with_options(source, LoadOptions::default(), refresh).await
    }

    pub async fn load_with_options(
        source: impl AsRef<str>,
        load_options: LoadOptions,
        refresh_options: RefreshOptions,
    ) -> Result<Self> {
        let source = source.as_ref().to_owned();
        let attempted_at = SystemTime::now();
        let package =
            Arc::new(Package::load_snapshot_with_options(&source, load_options.clone()).await?);
        let loaded_at = package.loaded_at();
        let identity = package.identity();
        let immutable = package.immutable_source();
        let status = Arc::new(RwLock::new(RefreshStatus {
            current_fingerprint: package.source_fingerprint().cloned(),
            last_success: Some(loaded_at),
            last_attempt: None,
            consecutive_failures: 0,
            last_error: None,
            refreshing: false,
            immutable,
        }));
        if immutable && refresh_options.period().is_some() {
            tracing::warn!(
                source = %redacted_source(&source),
                "package source is pinned to an immutable commit; periodic refresh is disabled"
            );
        }
        let (events, _) = broadcast::channel(REFRESH_EVENT_CHANNEL_CAPACITY);
        let state = RefreshState {
            current: Arc::new(RwLock::new(package)),
            status: status.clone(),
            refresh_lock: Arc::new(Mutex::new(())),
            observer: refresh_options.observer.clone(),
            events,
            last_event: Arc::new(RwLock::new(None)),
        };
        emit_event(
            &state,
            RefreshEvent::new(
                RefreshEventType::Loaded,
                &source,
                None,
                Some(identity),
                attempted_at,
                loaded_at,
                None,
                0,
                None,
            ),
        );
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

    pub fn current(&self) -> Arc<Package> {
        self.state
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn status(&self) -> RefreshStatus {
        self.state
            .status
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Identity of the package currently active in this process.
    pub fn identity(&self) -> PackageIdentity {
        self.current().identity()
    }

    /// Current refresh state joined with package identity. This is the better
    /// surface for operational export and rollout-completion checks: it answers
    /// what is true now, where events answer what changed.
    pub fn snapshot(&self) -> RefreshSnapshot {
        let status = self.status();
        let last_event = self
            .state
            .last_event
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        RefreshSnapshot {
            identity: self.identity(),
            last_attempt: status.last_attempt,
            last_success: status.last_success,
            last_event,
            consecutive_failures: status.consecutive_failures,
            last_error: status.last_error,
            refreshing: status.refreshing,
            immutable: status.immutable,
        }
    }

    /// Subscribe to refresh state-transition events. The returned receiver is a
    /// bounded broadcast channel: it never blocks refresh, and a lagging
    /// consumer drops the oldest events rather than stalling. Recover ground
    /// truth from `snapshot()` or `identity()` after a lag.
    pub fn subscribe_refresh_events(&self) -> broadcast::Receiver<RefreshEvent> {
        self.state.events.subscribe()
    }

    pub async fn refresh_now(&self) -> Result<RefreshOutcome> {
        refresh_once(&self.source, &self.load_options, &self.state).await
    }

    pub async fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        let now = SystemTime::now();
        emit_event(
            &self.state,
            RefreshEvent::new(
                RefreshEventType::Shutdown,
                &self.source,
                None,
                Some(self.current().identity()),
                now,
                now,
                None,
                self.status().consecutive_failures,
                None,
            ),
        );
    }

    pub fn resolve_qualifier(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
    ) -> Result<bool> {
        self.current().resolve_qualifier(id.as_ref(), context)
    }

    pub fn resolve_qualifier_with_options(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
        options: ResolveOptions,
    ) -> Result<bool> {
        self.current()
            .resolve_qualifier_with_options(id.as_ref(), context, options)
    }

    pub fn resolve_variable(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
    ) -> Result<VariableResolution> {
        self.current().resolve_variable(id.as_ref(), context)
    }

    pub fn resolve_variable_with_options(
        &self,
        id: impl AsRef<str>,
        context: &EvaluationContext,
        options: ResolveOptions,
    ) -> Result<VariableResolution> {
        self.current()
            .resolve_variable_with_options(id.as_ref(), context, options)
    }

    pub fn refresh_options(&self) -> &RefreshOptions {
        &self.refresh_options
    }
}

impl Drop for RefreshingPackage {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub struct RefreshOptions {
    period: Option<Duration>,
    max_staleness: Option<Duration>,
    min_failure_backoff: Duration,
    max_failure_backoff: Duration,
    observer: Option<Arc<dyn RefreshObserver>>,
}

impl std::fmt::Debug for RefreshOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshOptions")
            .field("period", &self.period)
            .field("max_staleness", &self.max_staleness)
            .field("min_failure_backoff", &self.min_failure_backoff)
            .field("max_failure_backoff", &self.max_failure_backoff)
            .field("observer", &self.observer.is_some())
            .finish()
    }
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

    /// Attach a synchronous, best-effort observer for refresh events. The
    /// observer must not block or perform I/O; bridge async work through
    /// `RefreshingPackage::subscribe_refresh_events` instead. Observer failure
    /// never affects refresh.
    pub fn with_observer<O>(mut self, observer: O) -> Self
    where
        O: RefreshObserver,
    {
        self.observer = Some(Arc::new(observer));
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
            observer: None,
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
                    state
                        .status
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .consecutive_failures,
                    &refresh_options,
                );
                tracing::warn!(
                    source = %redacted_source(&source),
                    error = %err,
                    backoff_ms = delay.as_millis(),
                    "package refresh failed; continuing to serve last known good package"
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
    let attempted_at = SystemTime::now();
    {
        let mut status = state
            .status
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.last_attempt = Some(attempted_at);
        status.refreshing = true;
    }
    let result = refresh_once_inner(source, load_options, state, attempted_at).await;
    {
        let mut status = state
            .status
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.refreshing = false;
        if let Err(err) = &result {
            status.consecutive_failures = status.consecutive_failures.saturating_add(1);
            status.last_error = Some(err.to_string());
        }
    }
    if let Err(err) = &result {
        let consecutive_failures = state
            .status
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .consecutive_failures;
        // The failed package must not be reported as current: keep last-known-good
        // as `current` and omit `previous` per the spec.
        let current = state
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .identity();
        emit_event(
            state,
            RefreshEvent::new(
                RefreshEventType::Failed,
                source,
                None,
                Some(current),
                attempted_at,
                SystemTime::now(),
                None,
                consecutive_failures,
                Some(err.to_string()),
            ),
        );
    }
    result
}

async fn refresh_once_inner(
    source: &str,
    load_options: &LoadOptions,
    state: &RefreshState,
    attempted_at: SystemTime,
) -> Result<RefreshOutcome> {
    let (previous_fingerprint, previous_identity, layers) = {
        let current = state
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            current.source_fingerprint().cloned(),
            current.identity(),
            current.source_layers().to_vec(),
        )
    };
    match probe_package_source_graph(
        source,
        load_options.source(),
        previous_fingerprint.as_ref(),
        &layers,
    )
    .await?
    {
        SourceProbe::Unchanged => {
            tracing::debug!(source = %redacted_source(source), "package source is unchanged");
            emit_event(
                state,
                RefreshEvent::new(
                    RefreshEventType::Unchanged,
                    source,
                    None,
                    Some(previous_identity),
                    attempted_at,
                    SystemTime::now(),
                    Some(RefreshOutcome::Unchanged),
                    0,
                    None,
                ),
            );
            return Ok(RefreshOutcome::Unchanged);
        }
        SourceProbe::ImmutablePinned(fingerprint) => {
            tracing::warn!(
                source = %redacted_source(source),
                "package source is pinned to an immutable commit; periodic refresh is disabled"
            );
            {
                let mut status = state
                    .status
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                status.current_fingerprint = Some(fingerprint);
                status.immutable = true;
            }
            emit_event(
                state,
                RefreshEvent::new(
                    RefreshEventType::Immutable,
                    source,
                    None,
                    Some(state.current_identity()),
                    attempted_at,
                    SystemTime::now(),
                    Some(RefreshOutcome::Immutable),
                    0,
                    None,
                ),
            );
            return Ok(RefreshOutcome::Immutable);
        }
        SourceProbe::Changed(_) => {
            tracing::info!(source = %redacted_source(source), "package source changed");
        }
        SourceProbe::Unknown => {
            tracing::debug!(
                source = %redacted_source(source),
                "package source change status is unknown; attempting refresh"
            );
        }
    }

    let package =
        Arc::new(Package::load_snapshot_with_options(source, load_options.clone()).await?);
    let fingerprint = package.source_fingerprint().cloned();
    let immutable = package.immutable_source();
    let loaded_at = package.loaded_at();
    let current_identity = package.identity();
    {
        let mut current = state
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = package;
    }
    {
        let mut status = state
            .status
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.current_fingerprint = fingerprint;
        status.last_success = Some(loaded_at);
        status.consecutive_failures = 0;
        status.last_error = None;
        status.immutable = immutable;
    }
    tracing::info!(
        source = %redacted_source(source),
        event_type = "refreshed",
        release_id = current_identity.release_id.as_deref().unwrap_or(""),
        previous_release_id = previous_identity.release_id.as_deref().unwrap_or(""),
        "package refresh succeeded"
    );
    emit_event(
        state,
        RefreshEvent::new(
            RefreshEventType::Refreshed,
            source,
            Some(previous_identity),
            Some(current_identity),
            attempted_at,
            loaded_at,
            Some(RefreshOutcome::Refreshed),
            0,
            None,
        ),
    );
    Ok(RefreshOutcome::Refreshed)
}

async fn probe_package_source_graph(
    source: &str,
    options: &SourceOptions,
    previous: Option<&SourceFingerprint>,
    layers: &[SourceLayer],
) -> Result<SourceProbe> {
    if layers.len() <= 1 {
        return probe_package_source(source, options, previous).await;
    }

    for layer in layers {
        match probe_package_source(layer.source(), options, layer.fingerprint()).await? {
            SourceProbe::Unchanged => {}
            SourceProbe::ImmutablePinned(_) if layer.immutable() => {}
            SourceProbe::ImmutablePinned(_) => return Ok(SourceProbe::Unchanged),
            SourceProbe::Changed(fingerprint) => return Ok(SourceProbe::Changed(fingerprint)),
            SourceProbe::Unknown => return Ok(SourceProbe::Unknown),
        }
    }
    Ok(SourceProbe::Unchanged)
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

/// Bounded capacity for the refresh-event broadcast channel. A lagging consumer
/// drops the oldest events rather than blocking refresh; recover from
/// `snapshot()`/`identity()`.
const REFRESH_EVENT_CHANNEL_CAPACITY: usize = 64;

impl RefreshState {
    fn current_identity(&self) -> PackageIdentity {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .identity()
    }
}

fn emit_event(state: &RefreshState, event: RefreshEvent) {
    {
        let mut last = state
            .last_event
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *last = Some(event.summary());
    }
    if let Some(observer) = &state.observer {
        let observer = observer.clone();
        let event = event.clone();
        // Observer failure must never break refresh; a panicking observer is
        // contained here rather than propagated into the refresh path.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            observer.observe(event);
        }));
    }
    // No subscribers (or a full lagging channel) is not an error for refresh.
    let _ = state.events.send(event);
}

/// Synchronous, best-effort sink for refresh events. Implementations must not
/// block or perform I/O. Any blocking or async forwarding should go through
/// `RefreshingPackage::subscribe_refresh_events` instead.
pub trait RefreshObserver: Send + Sync + 'static {
    fn observe(&self, event: RefreshEvent);
}

impl<F> RefreshObserver for F
where
    F: Fn(RefreshEvent) + Send + Sync + 'static,
{
    fn observe(&self, event: RefreshEvent) {
        self(event)
    }
}

/// Source string with credentials removed: userinfo stripped, never carrying a
/// bearer token. Scheme, host, path, ref, and subdir are preserved when safe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedactedPackageSource(String);

impl RedactedPackageSource {
    pub fn new(source: &str) -> Self {
        Self(redacted_source(source))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RedactedPackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable, serializable identity of a loaded package.
#[derive(Clone, Debug)]
pub struct PackageIdentity {
    pub source: RedactedPackageSource,
    pub fingerprint: Option<SourceFingerprint>,
    pub release_id: Option<String>,
    pub loaded_at: SystemTime,
    pub immutable: bool,
    pub layers: Vec<PackageLayerIdentity>,
}

impl PackageIdentity {
    pub fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "source": self.source.as_str(),
            "releaseId": self.release_id,
            "fingerprint": self.fingerprint.as_ref().map(source_fingerprint_to_json),
            "loadedAt": system_time_to_unix_seconds(Some(self.loaded_at)),
            "immutable": self.immutable,
            "layers": self.layers.iter().map(PackageLayerIdentity::to_json).collect::<Vec<_>>(),
        })
    }
}

/// Identity of a single layer in a layered package.
#[derive(Clone, Debug)]
pub struct PackageLayerIdentity {
    pub source: RedactedPackageSource,
    pub fingerprint: Option<SourceFingerprint>,
    pub release_id: Option<String>,
    pub immutable: bool,
}

impl PackageLayerIdentity {
    fn from_layer(layer: &SourceLayer) -> Self {
        Self {
            source: RedactedPackageSource::new(layer.source()),
            fingerprint: layer.fingerprint().cloned(),
            release_id: layer.fingerprint().and_then(release_id_from_fingerprint),
            immutable: layer.immutable(),
        }
    }

    pub fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "source": self.source.as_str(),
            "releaseId": self.release_id,
            "fingerprint": self.fingerprint.as_ref().map(source_fingerprint_to_json),
            "immutable": self.immutable,
        })
    }
}

/// Best-effort stable release label derived deterministically from a
/// fingerprint. `None` only when the source has no fingerprint (for example a
/// local directory), so callers can distinguish "no release identity" from a
/// derived one.
fn release_id_from_fingerprint(fingerprint: &SourceFingerprint) -> Option<String> {
    Some(match fingerprint {
        SourceFingerprint::GitCommit(commit) => format!("git:{commit}"),
        SourceFingerprint::ContentHash(hash) => hash.clone(),
        SourceFingerprint::HttpValidator(value) => release_id_from_http_validator(value),
        SourceFingerprint::PackageLayers(layers) => {
            let joined = layers
                .iter()
                .map(|layer| {
                    release_id_from_fingerprint(layer).unwrap_or_else(|| "none".to_owned())
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("layers:{}", stable_hash(&joined))
        }
    })
}

fn release_id_from_http_validator(value: &str) -> String {
    if let Some(index) = value.find("sha256:") {
        let digest: String = value[index..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == ':')
            .collect();
        if digest.len() > "sha256:".len() {
            return digest;
        }
    }
    format!("http:{}", stable_hash(value))
}

/// Deterministic, platform-stable short hash (SHA-256 truncated to 8 bytes,
/// hex). Used for opaque HTTP validators and layered release ids.
fn stable_hash(value: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, value.as_bytes());
    digest.as_ref()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Refresh state joined with package identity. Answers "what is true now".
#[derive(Clone, Debug)]
pub struct RefreshSnapshot {
    pub identity: PackageIdentity,
    pub last_attempt: Option<SystemTime>,
    pub last_success: Option<SystemTime>,
    pub last_event: Option<RefreshEventSummary>,
    pub consecutive_failures: u64,
    pub last_error: Option<String>,
    pub refreshing: bool,
    pub immutable: bool,
}

impl RefreshSnapshot {
    pub fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "identity": self.identity.to_json(),
            "lastAttempt": system_time_to_unix_seconds(self.last_attempt),
            "lastSuccess": system_time_to_unix_seconds(self.last_success),
            "lastEvent": self.last_event.as_ref().map(RefreshEventSummary::to_json),
            "consecutiveFailures": self.consecutive_failures,
            "lastError": self.last_error,
            "refreshing": self.refreshing,
            "immutable": self.immutable,
        })
    }
}

/// Compact record of the most recent refresh event, carried on a snapshot so a
/// late subscriber that missed the live event can still see what last happened.
#[derive(Clone, Debug)]
pub struct RefreshEventSummary {
    pub event_id: Uuid,
    pub event_type: RefreshEventType,
    pub release_id: Option<String>,
    pub completed_at: SystemTime,
}

impl RefreshEventSummary {
    pub fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "eventId": self.event_id.to_string(),
            "eventType": self.event_type.as_str(),
            "releaseId": self.release_id,
            "completedAt": system_time_to_unix_seconds(Some(self.completed_at)),
        })
    }
}

/// A refresh state-transition event.
#[derive(Clone, Debug)]
pub struct RefreshEvent {
    pub event_id: Uuid,
    pub event_type: RefreshEventType,
    pub source: RedactedPackageSource,
    pub previous: Option<PackageIdentity>,
    pub current: Option<PackageIdentity>,
    pub attempted_at: SystemTime,
    pub completed_at: SystemTime,
    pub duration: Duration,
    pub outcome: Option<RefreshOutcome>,
    pub consecutive_failures: u64,
    pub error: Option<String>,
    pub sdk: SdkIdentity,
}

impl RefreshEvent {
    #[allow(clippy::too_many_arguments)]
    fn new(
        event_type: RefreshEventType,
        source: &str,
        previous: Option<PackageIdentity>,
        current: Option<PackageIdentity>,
        attempted_at: SystemTime,
        completed_at: SystemTime,
        outcome: Option<RefreshOutcome>,
        consecutive_failures: u64,
        error: Option<String>,
    ) -> Self {
        let duration = completed_at
            .duration_since(attempted_at)
            .unwrap_or_default();
        Self {
            event_id: Uuid::new_v4(),
            event_type,
            source: RedactedPackageSource::new(source),
            previous,
            current,
            attempted_at,
            completed_at,
            duration,
            outcome,
            consecutive_failures,
            error,
            sdk: SdkIdentity::rust(),
        }
    }

    fn summary(&self) -> RefreshEventSummary {
        RefreshEventSummary {
            event_id: self.event_id,
            event_type: self.event_type,
            release_id: self
                .current
                .as_ref()
                .and_then(|identity| identity.release_id.clone()),
            completed_at: self.completed_at,
        }
    }

    pub fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "schemaVersion": 1,
            "eventId": self.event_id.to_string(),
            "eventType": self.event_type.as_str(),
            "source": self.source.as_str(),
            "previous": self.previous.as_ref().map(PackageIdentity::to_json),
            "current": self.current.as_ref().map(PackageIdentity::to_json),
            "attemptedAt": system_time_to_unix_seconds(Some(self.attempted_at)),
            "completedAt": system_time_to_unix_seconds(Some(self.completed_at)),
            "durationMs": self.duration.as_millis() as u64,
            "outcome": self.outcome.map(refresh_outcome_str),
            "consecutiveFailures": self.consecutive_failures,
            "error": self.error,
            "sdk": self.sdk.to_json(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshEventType {
    Loaded,
    RefreshStarted,
    Unchanged,
    Refreshed,
    Failed,
    Immutable,
    Shutdown,
}

impl RefreshEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RefreshEventType::Loaded => "loaded",
            RefreshEventType::RefreshStarted => "refresh_started",
            RefreshEventType::Unchanged => "unchanged",
            RefreshEventType::Refreshed => "refreshed",
            RefreshEventType::Failed => "failed",
            RefreshEventType::Immutable => "immutable",
            RefreshEventType::Shutdown => "shutdown",
        }
    }
}

/// Identity of the SDK that emitted an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdkIdentity {
    pub name: &'static str,
    pub version: &'static str,
    pub language: &'static str,
}

impl SdkIdentity {
    pub const fn rust() -> Self {
        Self {
            name: "rototo",
            version: env!("CARGO_PKG_VERSION"),
            language: "rust",
        }
    }

    pub fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "name": self.name,
            "version": self.version,
            "language": self.language,
        })
    }
}

fn refresh_outcome_str(outcome: RefreshOutcome) -> &'static str {
    match outcome {
        RefreshOutcome::Unchanged => "unchanged",
        RefreshOutcome::Refreshed => "refreshed",
        RefreshOutcome::Immutable => "immutable",
    }
}

/// Canonical JSON shape for a source fingerprint, shared across SDKs.
pub fn source_fingerprint_to_json(fingerprint: &SourceFingerprint) -> JsonValue {
    match fingerprint {
        SourceFingerprint::GitCommit(value) => {
            serde_json::json!({ "kind": "git_commit", "value": value })
        }
        SourceFingerprint::HttpValidator(value) => {
            serde_json::json!({ "kind": "http_validator", "value": value })
        }
        SourceFingerprint::ContentHash(value) => {
            serde_json::json!({ "kind": "content_hash", "value": value })
        }
        SourceFingerprint::PackageLayers(layers) => serde_json::json!({
            "kind": "package_layers",
            "layers": layers.iter().map(source_fingerprint_to_json).collect::<Vec<_>>(),
        }),
    }
}

fn system_time_to_unix_seconds(time: Option<SystemTime>) -> JsonValue {
    match time.and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok()) {
        Some(duration) => serde_json::json!(duration.as_secs_f64()),
        None => JsonValue::Null,
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

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationContext {
    value: JsonValue,
}

impl EvaluationContext {
    pub fn from_json(value: JsonValue) -> Result<Self> {
        if !value.is_object() {
            return Err(RototoError::new("evaluation context must be a JSON object"));
        }
        Ok(Self { value })
    }

    pub fn value(&self) -> &JsonValue {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceFingerprint;

    #[test]
    fn release_id_from_git_commit_is_prefixed() {
        let id = release_id_from_fingerprint(&SourceFingerprint::GitCommit("abc123".into()));
        assert_eq!(id.as_deref(), Some("git:abc123"));
    }

    #[test]
    fn release_id_from_content_hash_is_the_digest() {
        let id = release_id_from_fingerprint(&SourceFingerprint::ContentHash("sha256:4d1c".into()));
        assert_eq!(id.as_deref(), Some("sha256:4d1c"));
    }

    #[test]
    fn release_id_from_http_validator_extracts_digest() {
        let id = release_id_from_fingerprint(&SourceFingerprint::HttpValidator(
            "etag:\"sha256:2222abcd\"".into(),
        ));
        assert_eq!(id.as_deref(), Some("sha256:2222abcd"));
    }

    #[test]
    fn release_id_from_opaque_http_validator_is_stable_and_prefixed() {
        let value = SourceFingerprint::HttpValidator("W/\"opaque-etag\"".into());
        let first = release_id_from_fingerprint(&value).unwrap();
        let second = release_id_from_fingerprint(&value).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("http:"), "got {first}");
        assert!(!first.contains("sha256:"));
    }

    #[test]
    fn release_id_from_layers_is_stable_hash() {
        let layers = SourceFingerprint::PackageLayers(vec![
            SourceFingerprint::GitCommit("aaa".into()),
            SourceFingerprint::ContentHash("sha256:bbb".into()),
        ]);
        let id = release_id_from_fingerprint(&layers).unwrap();
        assert!(id.starts_with("layers:"), "got {id}");
        // Deterministic across calls.
        assert_eq!(id, release_id_from_fingerprint(&layers).unwrap());
        // Order-sensitive: a different layer order yields a different id.
        let swapped = SourceFingerprint::PackageLayers(vec![
            SourceFingerprint::ContentHash("sha256:bbb".into()),
            SourceFingerprint::GitCommit("aaa".into()),
        ]);
        assert_ne!(id, release_id_from_fingerprint(&swapped).unwrap());
    }

    #[test]
    fn redacted_source_strips_userinfo_and_tokens() {
        let redacted = RedactedPackageSource::new(
            "git+https://user:secret-token@github.com/acme/cfg.git#main:p",
        );
        assert!(!redacted.as_str().contains("secret-token"));
        assert!(!redacted.as_str().contains("user"));
        assert!(redacted.as_str().contains("github.com/acme/cfg.git#main:p"));
    }

    #[test]
    fn redacted_source_preserves_clean_source() {
        let source = "https://config.acme.com/rototo/checkout/prod/current.tar.gz";
        assert_eq!(RedactedPackageSource::new(source).as_str(), source);
    }

    #[test]
    fn event_type_names_are_snake_case() {
        assert_eq!(RefreshEventType::Loaded.as_str(), "loaded");
        assert_eq!(RefreshEventType::RefreshStarted.as_str(), "refresh_started");
        assert_eq!(RefreshEventType::Refreshed.as_str(), "refreshed");
        assert_eq!(RefreshEventType::Failed.as_str(), "failed");
        assert_eq!(RefreshEventType::Immutable.as_str(), "immutable");
        assert_eq!(RefreshEventType::Shutdown.as_str(), "shutdown");
    }
}
