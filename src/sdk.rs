use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use serde_json::Value as JsonValue;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::error::{Result, RototoError};
use crate::lint::{
    LintInput, RuntimePackage, compile_runtime_package_from_snapshot, lint_package_snapshot,
};
use crate::model::{
    ListConfig, PackageInspection, PackageLint, VariableResolution, VariableResolutionTrace,
};
use crate::package::inspect_package;
use crate::source::{
    SourceAuth, SourceFingerprint, SourceLayer, SourceOptions, StagedPackage, load_package_source,
    load_package_source_snapshot,
};

mod refresh;

pub use refresh::{
    RefreshEvent, RefreshEventSummary, RefreshEventType, RefreshOptions, RefreshOutcome,
    RefreshSnapshot, RefreshStatus, RefreshingPackage,
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
    /// True when this package was loaded from the fallback source because the
    /// primary failed. See [`LoadOptions::with_fallback_source`].
    served_fallback: bool,
    /// Sink for resolution trace events. A standalone package owns its channel;
    /// a `RefreshingPackage` injects one shared channel into every package it
    /// loads so subscriptions survive refresh swaps.
    trace: Arc<TraceChannel>,
}

impl Package {
    pub async fn load(source: impl AsRef<str>) -> Result<Self> {
        Self::load_with_options(source, LoadOptions::default()).await
    }

    pub async fn load_with_options(source: impl AsRef<str>, options: LoadOptions) -> Result<Self> {
        let source = source.as_ref();
        let primary = match Self::load_one_with_options(source, &options).await {
            Ok(mut package) => {
                package.trace = Arc::new(TraceChannel::new(options.trace_capacity()));
                return Ok(package);
            }
            Err(err) => err,
        };
        let Some(fallback) = options.fallback_source() else {
            return Err(primary);
        };
        let fallback = fallback.to_owned();
        match Self::load_one_with_options(&fallback, &options).await {
            Ok(mut package) => {
                package.served_fallback = true;
                package.trace = Arc::new(TraceChannel::new(options.trace_capacity()));
                Ok(package)
            }
            Err(fallback_err) => Err(fallback_load_error(
                source,
                &primary,
                &fallback,
                &fallback_err,
            )),
        }
    }

    async fn load_one_with_options(source: &str, options: &LoadOptions) -> Result<Self> {
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
            served_fallback: false,
            trace: Arc::new(TraceChannel::new(DEFAULT_TRACE_EVENT_CAPACITY)),
        })
    }

    /// True when this package was loaded from the fallback source because the
    /// primary source failed. Health checks should read this instead of
    /// string-matching URIs in [`Package::identity`].
    pub fn served_fallback(&self) -> bool {
        self.served_fallback
    }

    /// Replace this package's trace channel. Used to size the channel from
    /// `LoadOptions` on standalone load, and to share one channel across a
    /// `RefreshingPackage`'s reloads.
    fn with_trace_channel(mut self, trace: Arc<TraceChannel>) -> Self {
        self.trace = trace;
        self
    }

    /// Subscribe to resolution trace events emitted by this package. See
    /// [`RefreshingPackage::subscribe_trace_events`] for the delivery contract.
    pub fn subscribe_trace_events(&self) -> TraceSubscription {
        self.trace.subscribe()
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

    /// Tracing runs only when someone is listening and there is something to
    /// emit: an app-requested trace or at least one `[[trace]]` policy. With no
    /// subscriber, the trace is never computed.
    fn tracing_active(&self, options: &ResolveOptions, runtime: &RuntimePackage) -> bool {
        self.trace.has_subscribers() && (options.trace || !runtime.trace_policies.is_empty())
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
        let runtime = self.runtime()?;
        if options.validate_context {
            runtime.validate_context_for_variable(id.as_ref(), context.value())?;
        }
        if !self.tracing_active(&options, runtime) {
            return crate::resolve::resolve_variable_unchecked(
                runtime,
                id.as_ref(),
                context.value(),
            );
        }
        let (resolution, capture) = crate::resolve::resolve_variable_traced_unchecked(
            runtime,
            id.as_ref(),
            context.value(),
            options.trace,
        )?;
        if let Some(capture) = capture {
            self.trace.emit(TraceEvent::new(
                TraceTarget::Variable {
                    id: id.as_ref().to_owned(),
                },
                context.value().clone(),
                TraceDetail::Variable(Box::new(capture.trace)),
                TraceProvenance {
                    app_requested: options.trace,
                    policies: capture.policies,
                },
                self.identity(),
                SystemTime::now(),
            ));
        }
        Ok(resolution)
    }

    fn runtime(&self) -> Result<&RuntimePackage> {
        self.runtime.as_ref().ok_or_else(|| {
            RototoError::new(
                "package was loaded without a runtime model; use Package::load with lint enabled",
            )
        })
    }

    /// Every list id in the loaded package.
    pub fn list_ids(&self) -> Result<Vec<String>> {
        Ok(self.runtime()?.lists.keys().cloned().collect())
    }

    /// One list, contract and members together.
    pub fn read_list(&self, id: impl AsRef<str>) -> Result<ListConfig> {
        let id = id.as_ref();
        let declaration = self
            .runtime()?
            .lists
            .get(id)
            .ok_or_else(|| RototoError::new(format!("list not found: list={id}")))?;
        Ok(ListConfig {
            id: id.to_owned(),
            description: declaration.description.clone(),
            member_type: declaration.member_type.clone(),
            members: declaration.members.clone(),
        })
    }

    /// Every entry id of one catalog. Ids only: enumerating a large catalog
    /// should not clone its values.
    pub fn entry_ids(&self, catalog: impl AsRef<str>) -> Result<Vec<String>> {
        let catalog = catalog.as_ref();
        let entries = self
            .runtime()?
            .catalog_entries
            .get(catalog)
            .ok_or_else(|| RototoError::new(format!("catalog not found: catalog={catalog}")))?;
        Ok(entries.keys().cloned().collect())
    }

    /// One raw entry, exactly as authored: no reference hydration, no id
    /// injection (the caller passed the id in).
    pub fn read_entry(
        &self,
        catalog: impl AsRef<str>,
        entry: impl AsRef<str>,
    ) -> Result<JsonValue> {
        let catalog = catalog.as_ref();
        let entry = entry.as_ref();
        self.runtime()?
            .catalog_entries
            .get(catalog)
            .and_then(|entries| entries.get(entry))
            .cloned()
            .ok_or_else(|| {
                RototoError::new(format!(
                    "catalog entry not found: catalog={catalog}:entry={entry}"
                ))
            })
    }

    /// Follows one reference the way hydration would have, on demand: same
    /// entry lookup, same pointer application, same first-match rule for
    /// multi-catalog pins. One hop; the returned value is the raw target.
    pub fn resolve_reference(&self, reference: &ValueRef) -> Result<JsonValue> {
        let runtime = self.runtime()?;
        let catalog = reference
            .catalogs
            .iter()
            .find(|catalog| {
                runtime
                    .catalog_entries
                    .get(catalog.as_str())
                    .is_some_and(|entries| entries.contains_key(&reference.entry))
            })
            .ok_or_else(|| {
                RototoError::new(format!(
                    "reference does not resolve: {} (no pinned catalog has the entry)",
                    reference.address()
                ))
            })?;
        let value = runtime.catalog_entries[catalog.as_str()][&reference.entry].clone();
        match reference.pointer.as_deref() {
            None | Some("") => Ok(value),
            Some(pointer) => value.pointer(pointer).cloned().ok_or_else(|| {
                RototoError::new(format!(
                    "reference points at a missing path: {}",
                    reference.address()
                ))
            }),
        }
    }

    /// [`Package::resolve_reference`] from an address string:
    /// `catalog=email_template:entry=welcome#/body`.
    pub fn resolve_reference_at(&self, address: impl AsRef<str>) -> Result<JsonValue> {
        self.resolve_reference(&ValueRef::parse(address.as_ref())?)
    }

    /// Walks a value together with its catalog's schema and reports every
    /// field whose schema carries `x-rototo-ref`, as (JSON pointer into the
    /// value, parsed reference). The reporting twin of hydration: apps
    /// hydrate selectively with [`Package::resolve_reference`].
    pub fn references_in(
        &self,
        catalog: impl AsRef<str>,
        value: &JsonValue,
    ) -> Result<Vec<(String, ValueRef)>> {
        let runtime = self.runtime()?;
        Ok(
            crate::resolve::hydrate::collect_catalog_references(runtime, catalog.as_ref(), value)
                .into_iter()
                .map(|reference| {
                    (
                        reference.pointer,
                        ValueRef {
                            catalogs: reference.catalogs,
                            entry: reference.entry,
                            pointer: reference.entry_pointer,
                        },
                    )
                })
                .collect(),
        )
    }
}

/// Default bounded capacity for the refresh-event broadcast channel. A lagging
/// consumer drops the oldest events rather than blocking refresh; recover from
/// `snapshot()`/`identity()`. Refresh events are timer-paced, so this stays
/// small; override via [`LoadOptions::with_refresh_capacity`].
const DEFAULT_REFRESH_EVENT_CAPACITY: usize = 64;

/// Default bounded capacity for the trace-event broadcast channel. Trace events
/// are traffic-paced, so this is larger than the refresh default; override via
/// [`LoadOptions::with_trace_capacity`] to match a deployment's traffic and
/// memory budget.
const DEFAULT_TRACE_EVENT_CAPACITY: usize = 256;

/// One error for a start where both the primary and the fallback source
/// failed, naming both attempts and reasons, primary first. Sources are
/// redacted; the reasons are the underlying load errors verbatim.
pub(super) fn fallback_load_error(
    primary: &str,
    primary_err: &RototoError,
    fallback: &str,
    fallback_err: &RototoError,
) -> RototoError {
    RototoError::new(format!(
        "failed to load package from primary `{}`: {primary_err}; fallback `{}` also failed: {fallback_err}",
        redacted_source(primary),
        redacted_source(fallback),
    ))
}

/// Redact credentials from a package source string: userinfo is replaced with
/// `<redacted>`, never carrying a bearer token. Scheme, host, path, ref, and
/// subdir are preserved when safe. Shared by [`RedactedPackageSource`] and the
/// refresh runtime's structured logging.
pub(super) fn redacted_source(source: &str) -> String {
    match source.split_once("://") {
        Some((scheme, rest)) if rest.contains('@') => {
            let host = rest.rsplit_once('@').map(|(_, host)| host).unwrap_or(rest);
            format!("{scheme}://<redacted>@{host}")
        }
        _ => source.to_owned(),
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

// ---- Resolution tracing ----

/// Broadcast channel for resolution trace events. Delivery is channel-only (no
/// synchronous observer): a consumer reads a [`TraceSubscription`] and does any
/// I/O off the resolve path. Drop-oldest under lag bounds memory; the lost count
/// surfaces as [`TraceStreamItem::Dropped`].
#[derive(Debug)]
struct TraceChannel {
    events: broadcast::Sender<Arc<TraceEvent>>,
}

impl TraceChannel {
    fn new(capacity: usize) -> Self {
        let (events, _) = broadcast::channel(capacity.max(1));
        Self { events }
    }

    fn subscribe(&self) -> TraceSubscription {
        TraceSubscription {
            receiver: self.events.subscribe(),
        }
    }

    /// Whether any subscription is currently live. Resolution skips building and
    /// emitting trace events when this is false.
    fn has_subscribers(&self) -> bool {
        self.events.receiver_count() > 0
    }

    fn emit(&self, event: TraceEvent) {
        // No subscribers (or a full lagging channel) is not an error.
        let _ = self.events.send(Arc::new(event));
    }
}

/// A live subscription to a package's resolution trace events.
pub struct TraceSubscription {
    receiver: broadcast::Receiver<Arc<TraceEvent>>,
}

impl TraceSubscription {
    /// Receive the next trace stream item. Returns `None` once the package and
    /// all senders are gone. A lagging consumer receives
    /// [`TraceStreamItem::Dropped`] with the number of events skipped, then
    /// resumes from the oldest still-buffered event.
    pub async fn recv(&mut self) -> Option<TraceStreamItem> {
        match self.receiver.recv().await {
            Ok(event) => Some(TraceStreamItem::Trace(event)),
            Err(broadcast::error::RecvError::Lagged(count)) => {
                Some(TraceStreamItem::Dropped { count })
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

/// One item from a [`TraceSubscription`].
#[derive(Clone, Debug)]
pub enum TraceStreamItem {
    /// A captured resolution trace.
    Trace(Arc<TraceEvent>),
    /// The consumer lagged and `count` trace events were dropped before this
    /// point. Surfaced so silence is never ambiguous between "not traced" and
    /// "traced but dropped".
    Dropped { count: u64 },
}

impl TraceStreamItem {
    pub fn to_json(&self) -> JsonValue {
        match self {
            TraceStreamItem::Trace(event) => serde_json::json!({
                "kind": "trace",
                "trace": event.to_json(),
            }),
            TraceStreamItem::Dropped { count } => serde_json::json!({
                "kind": "dropped",
                "count": count,
            }),
        }
    }
}

/// The entity a trace describes.
#[derive(Clone, Debug)]
pub enum TraceTarget {
    Variable { id: String },
}

impl TraceTarget {
    pub fn kind(&self) -> &'static str {
        match self {
            TraceTarget::Variable { .. } => "variable",
        }
    }

    pub fn id(&self) -> &str {
        match self {
            TraceTarget::Variable { id } => id,
        }
    }
}

impl TraceEvent {
    /// The id of the entity this trace describes.
    pub fn target_id(&self) -> &str {
        self.target.id()
    }

    /// `"variable"`.
    pub fn target_kind(&self) -> &'static str {
        self.target.kind()
    }
}

/// The captured execution detail of a resolution, full (no level knob).
#[derive(Clone, Debug)]
pub enum TraceDetail {
    Variable(Box<VariableResolutionTrace>),
}

impl TraceDetail {
    fn to_json(&self) -> JsonValue {
        match self {
            TraceDetail::Variable(trace) => serde_json::to_value(trace).unwrap_or(JsonValue::Null),
        }
    }
}

/// Why a trace fired. A single resolution emits at most one event; if both the
/// app asked and one or more `[[trace]]` policies matched, all reasons appear
/// here, so app- and package-driven tracing never double-emit.
#[derive(Clone, Debug)]
pub struct TraceProvenance {
    /// The resolve call passed `ResolveOptions { trace: true }`.
    pub app_requested: bool,
    /// Indices of the `[[trace]]` policies whose `when` matched.
    pub policies: Vec<usize>,
}

impl TraceProvenance {
    fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "appRequested": self.app_requested,
            "policies": self.policies,
        })
    }
}

/// A resolution trace event: the full execution detail plus the request context
/// and the package version it ran against. Redaction of the context is the
/// consumer's responsibility before logging.
#[derive(Clone, Debug)]
pub struct TraceEvent {
    pub event_id: Uuid,
    pub target: TraceTarget,
    pub context: JsonValue,
    pub detail: TraceDetail,
    pub provenance: TraceProvenance,
    pub identity: PackageIdentity,
    pub at: SystemTime,
    pub sdk: SdkIdentity,
}

impl TraceEvent {
    pub(crate) fn new(
        target: TraceTarget,
        context: JsonValue,
        detail: TraceDetail,
        provenance: TraceProvenance,
        identity: PackageIdentity,
        at: SystemTime,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            target,
            context,
            detail,
            provenance,
            identity,
            at,
            sdk: SdkIdentity::rust(),
        }
    }

    pub fn to_json(&self) -> JsonValue {
        serde_json::json!({
            "schemaVersion": 1,
            "eventId": self.event_id.to_string(),
            "targetKind": self.target.kind(),
            "targetId": self.target.id(),
            "context": self.context,
            "detail": self.detail.to_json(),
            "provenance": self.provenance.to_json(),
            "identity": self.identity.to_json(),
            "at": system_time_to_unix_seconds(Some(self.at)),
            "sdk": self.sdk.to_json(),
        })
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
    trace_capacity: usize,
    refresh_capacity: usize,
    fallback_source: Option<String>,
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

    /// Buffer depth for the trace-event channel. A lagging trace consumer drops
    /// the oldest events past this depth and observes a
    /// [`TraceStreamItem::Dropped`] count.
    pub fn trace_capacity(&self) -> usize {
        self.trace_capacity
    }

    /// Buffer depth for the refresh-event channel.
    pub fn refresh_capacity(&self) -> usize {
        self.refresh_capacity
    }

    pub fn with_lint(mut self, lint: LintMode) -> Self {
        self.lint = lint;
        self
    }

    pub fn with_source_auth(mut self, auth: SourceAuth) -> Self {
        self.source = self.source.with_auth(auth);
        self
    }

    pub fn with_trace_capacity(mut self, capacity: usize) -> Self {
        self.trace_capacity = capacity.max(1);
        self
    }

    pub fn with_refresh_capacity(mut self, capacity: usize) -> Self {
        self.refresh_capacity = capacity.max(1);
        self
    }

    /// The fallback package source, if one is configured.
    pub fn fallback_source(&self) -> Option<&str> {
        self.fallback_source.as_deref()
    }

    /// Name a fallback package source for degraded starts. When the primary
    /// source fails to load for any reason (fetch, auth, staging, parse, or a
    /// lint failure under [`LintMode::Deny`]), the SDK loads this source
    /// through the identical pipeline instead of failing the start. Typically
    /// a local path to a bundled, app-tested copy of the package. A healthy
    /// primary always wins; if both fail, the load error names both attempts.
    /// A [`RefreshingPackage`] started on the fallback keeps refreshing from
    /// the primary; the fallback itself is never refreshed from.
    pub fn with_fallback_source(mut self, source: impl Into<String>) -> Self {
        self.fallback_source = Some(source.into());
        self
    }
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            lint: LintMode::Deny,
            source: SourceOptions::default(),
            trace_capacity: DEFAULT_TRACE_EVENT_CAPACITY,
            refresh_capacity: DEFAULT_REFRESH_EVENT_CAPACITY,
            fallback_source: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintMode {
    Deny,
    Skip,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveOptions {
    pub validate_context: bool,
    /// Emit a full trace of this resolution to the trace stream, regardless of
    /// any `[[trace]]` policy. Distinct from `trace_variable_resolution`, which
    /// returns the trace inline; this routes it to subscribers.
    pub trace: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            validate_context: true,
            trace: false,
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
        assert_eq!(RefreshEventType::FallbackLoaded.as_str(), "fallback_loaded");
        assert_eq!(RefreshEventType::RefreshStarted.as_str(), "refresh_started");
        assert_eq!(RefreshEventType::Refreshed.as_str(), "refreshed");
        assert_eq!(RefreshEventType::Failed.as_str(), "failed");
        assert_eq!(RefreshEventType::Immutable.as_str(), "immutable");
        assert_eq!(RefreshEventType::Shutdown.as_str(), "shutdown");
    }
}

/// A reference to a catalog entry (optionally a path inside it), the
/// app-side twin of `x-rototo-ref`. Obtained from
/// [`Package::references_in`], parsed from an address, or built from the
/// raw reference forms package files use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueRef {
    /// Candidate catalogs, in pin order. A parsed address has exactly one;
    /// a multi-catalog `x-rototo-ref` pin carries them all, and resolution
    /// picks the first that contains the entry (hydration's rule).
    catalogs: Vec<String>,
    entry: String,
    pointer: Option<String>,
}

impl ValueRef {
    /// From an address: `catalog=email_template:entry=welcome#/body`.
    pub fn parse(address: &str) -> Result<Self> {
        let parsed = crate::address::Address::parse(address)?;
        let steps = parsed.steps();
        let [catalog_step, entry_step] = steps else {
            return Err(invalid_value_ref(address));
        };
        let (
            crate::address::EntityClass::Catalog,
            crate::address::StepId::Entity(catalog),
            crate::address::EntityClass::Entry,
            crate::address::StepId::Entity(entry),
        ) = (
            catalog_step.class,
            &catalog_step.id,
            entry_step.class,
            &entry_step.id,
        )
        else {
            return Err(invalid_value_ref(address));
        };
        Ok(Self {
            catalogs: vec![catalog.clone()],
            entry: entry.clone(),
            pointer: parsed.pointer().map(str::to_owned),
        })
    }

    /// From a raw entry-reference string (`welcome#/body`) plus the pinned
    /// target catalogs, mirroring `x-rototo-ref` semantics.
    pub fn from_entry_ref(value: &str, pins: &[&str]) -> Result<Self> {
        if pins.is_empty() {
            return Err(RototoError::new(
                "an entry reference needs at least one pinned catalog",
            ));
        }
        let (entry, pointer) = crate::resolve::hydrate::split_catalog_entry_ref(value)
            .ok_or_else(|| RototoError::new(format!("value is not an entry reference: {value}")))?;
        Ok(Self {
            catalogs: pins.iter().map(|pin| (*pin).to_owned()).collect(),
            entry: entry.to_owned(),
            pointer: pointer.map(str::to_owned),
        })
    }

    /// From a dynamic reference object: `{ catalog, entry, pointer? }`.
    pub fn from_dynamic(value: &JsonValue) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| RototoError::new("a dynamic reference is an object"))?;
        let catalog = object
            .get("catalog")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("a dynamic reference names its catalog"))?;
        let entry = object
            .get("entry")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("a dynamic reference names its entry"))?;
        Ok(Self {
            catalogs: vec![catalog.to_owned()],
            entry: entry.to_owned(),
            pointer: object
                .get("pointer")
                .and_then(JsonValue::as_str)
                .map(str::to_owned),
        })
    }

    /// The canonical address. A multi-catalog reference renders its first
    /// candidate; errors from resolution name the full candidate list.
    pub fn address(&self) -> String {
        let catalog = self.catalogs.first().map(String::as_str).unwrap_or("");
        let mut address = format!("catalog={catalog}:entry={}", self.entry);
        if let Some(pointer) = &self.pointer {
            address.push('#');
            address.push_str(pointer);
        }
        address
    }
}

fn invalid_value_ref(address: &str) -> RototoError {
    RototoError::new(format!(
        "a value reference address is catalog=<id>:entry=<key>[#<json-pointer>], got `{address}`"
    ))
}
