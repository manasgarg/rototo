use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use serde_json::Value as JsonValue;
use tokio::sync::{Mutex, broadcast, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::error::Result;
use crate::model::VariableResolution;
use crate::source::{
    SourceFingerprint, SourceLayer, SourceOptions, SourceProbe, probe_package_source,
};

use super::{
    EvaluationContext, LoadOptions, Package, PackageIdentity, RedactedPackageSource,
    ResolveOptions, SdkIdentity, TraceChannel, TraceSubscription, fallback_load_error,
    redacted_source, system_time_to_unix_seconds,
};

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
    events: broadcast::Sender<RefreshEvent>,
    last_event: Arc<RwLock<Option<RefreshEventSummary>>>,
    /// Shared across every package this handle loads, so trace subscriptions
    /// survive the `current` package being swapped on refresh.
    trace: Arc<TraceChannel>,
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
        let mut primary_error = None;
        let package = match Package::load_snapshot_with_options(&source, load_options.clone()).await
        {
            Ok(package) => package,
            Err(primary_err) => {
                let Some(fallback) = load_options.fallback_source() else {
                    return Err(primary_err);
                };
                let fallback = fallback.to_owned();
                tracing::warn!(
                    source = %redacted_source(&source),
                    fallback = %redacted_source(&fallback),
                    error = %primary_err,
                    "primary package source failed to load; starting on the fallback package"
                );
                let mut package =
                    Package::load_snapshot_with_options(&fallback, load_options.clone())
                        .await
                        .map_err(|fallback_err| {
                            fallback_load_error(&source, &primary_err, &fallback, &fallback_err)
                        })?;
                package.served_fallback = true;
                primary_error = Some(primary_err.to_string());
                package
            }
        };
        let serving_fallback = package.served_fallback();
        let loaded_at = package.loaded_at();
        let identity = package.identity();
        // While serving the fallback, the primary's mutability is unknown, so
        // the refresh loop must keep running to recover the primary.
        let immutable = package.immutable_source() && !serving_fallback;
        let status = Arc::new(RwLock::new(RefreshStatus {
            current_fingerprint: package.source_fingerprint().cloned(),
            last_success: Some(loaded_at),
            last_attempt: None,
            consecutive_failures: 0,
            last_error: primary_error.clone(),
            refreshing: false,
            immutable,
            serving_fallback,
        }));
        if immutable && refresh_options.period().is_some() {
            tracing::warn!(
                source = %redacted_source(&source),
                "package source is pinned to an immutable commit; periodic refresh is disabled"
            );
        }
        let (events, _) = broadcast::channel(load_options.refresh_capacity());
        let trace = Arc::new(TraceChannel::new(load_options.trace_capacity()));
        let package = Arc::new(package.with_trace_channel(trace.clone()));
        let state = RefreshState {
            current: Arc::new(RwLock::new(package)),
            status: status.clone(),
            refresh_lock: Arc::new(Mutex::new(())),
            events,
            last_event: Arc::new(RwLock::new(None)),
            trace,
        };
        let event_type = if serving_fallback {
            RefreshEventType::FallbackLoaded
        } else {
            RefreshEventType::Loaded
        };
        emit_event(
            &state,
            RefreshEvent::new(
                event_type,
                &source,
                None,
                Some(identity),
                attempted_at,
                loaded_at,
                None,
                0,
                // A fallback start carries why the primary failed.
                primary_error,
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
            serving_fallback: status.serving_fallback,
        }
    }

    /// Subscribe to refresh state-transition events. The returned receiver is a
    /// bounded broadcast channel: it never blocks refresh, and a lagging
    /// consumer drops the oldest events rather than stalling. Recover ground
    /// truth from `snapshot()` or `identity()` after a lag.
    pub fn subscribe_refresh_events(&self) -> broadcast::Receiver<RefreshEvent> {
        self.state.events.subscribe()
    }

    /// Subscribe to resolution trace events. The returned subscription is a
    /// bounded broadcast stream shared across refreshes: it never blocks
    /// resolution, and a lagging consumer drops the oldest events and observes a
    /// [`TraceStreamItem::Dropped`] carrying how many were lost. Tracing only
    /// happens while at least one subscription is live.
    pub fn subscribe_trace_events(&self) -> TraceSubscription {
        self.state.trace.subscribe()
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
    /// True while the serving package came from the fallback source instead of
    /// the primary. Pairs with [`RefreshStatus::stale`]: an app can alarm on
    /// running degraded for too long.
    pub serving_fallback: bool,
}

impl RefreshStatus {
    pub fn stale(&self, max_staleness: Duration) -> bool {
        self.last_success
            .and_then(|last_success| last_success.elapsed().ok())
            .is_some_and(|age| age > max_staleness)
    }

    /// True while the serving package came from the fallback source instead of
    /// the primary. Clears on the first successful refresh from the primary.
    pub fn serving_fallback(&self) -> bool {
        self.serving_fallback
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
    let (previous_fingerprint, previous_identity, layers, serving_fallback) = {
        let current = state
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            current.source_fingerprint().cloned(),
            current.identity(),
            current.source_layers().to_vec(),
            current.served_fallback(),
        )
    };
    // While serving the fallback, the current fingerprint describes the
    // fallback package, not the primary; skip change probing and attempt a
    // full load from the primary until it recovers.
    let probe = if serving_fallback {
        SourceProbe::Unknown
    } else {
        probe_package_source_graph(
            source,
            load_options.source(),
            previous_fingerprint.as_ref(),
            &layers,
        )
        .await?
    };
    match probe {
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

    let package = Arc::new(
        Package::load_snapshot_with_options(source, load_options.clone())
            .await?
            .with_trace_channel(state.trace.clone()),
    );
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
        // A successful refresh always loads from the primary, so a fallback
        // start ends here; primary recovery is this ordinary refreshed event.
        status.serving_fallback = false;
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
    // No subscribers (or a full lagging channel) is not an error for refresh.
    let _ = state.events.send(event);
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
    pub serving_fallback: bool,
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
            "servingFallback": self.serving_fallback,
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
    /// The primary source failed at startup and the fallback package is
    /// serving. The event's `error` carries the primary failure reason.
    FallbackLoaded,
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
            RefreshEventType::FallbackLoaded => "fallback_loaded",
            RefreshEventType::RefreshStarted => "refresh_started",
            RefreshEventType::Unchanged => "unchanged",
            RefreshEventType::Refreshed => "refreshed",
            RefreshEventType::Failed => "failed",
            RefreshEventType::Immutable => "immutable",
            RefreshEventType::Shutdown => "shutdown",
        }
    }
}

fn refresh_outcome_str(outcome: RefreshOutcome) -> &'static str {
    match outcome {
        RefreshOutcome::Unchanged => "unchanged",
        RefreshOutcome::Refreshed => "refreshed",
        RefreshOutcome::Immutable => "immutable",
    }
}
