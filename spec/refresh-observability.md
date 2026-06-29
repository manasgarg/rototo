# Refresh Observability

## Status

Proposed.

## Summary

Applications that use `RefreshingPackage` need a reliable way to report which
package version each running instance has accepted. Operations teams need that
data to know when a configuration rollout has completed. Some applications also
need a customer-facing view that can answer which configuration version and
selected values are active for a request, account, or tenant.

Rototo should solve the SDK side of this problem by exposing package identity,
refresh events, and current refresh state in a structured form. Rototo should
not own a telemetry backend, a fleet inventory system, or an external database.
The application sends Rototo's events and state into its existing observability
system.

The design has three layers:

- Package identity: a stable description of the package currently active inside
  a process.
- Refresh events: structured SDK events emitted when loading or refreshing
  changes state.
- Application projection: app-owned logging, metrics, traces, health endpoints,
  audit tables, and customer-facing views built from package identity and
  resolution provenance.

## Problem

`RefreshingPackage` already keeps last-known-good state and exposes
`RefreshStatus`. It also emits tracing logs for refresh success and failure.
That is enough for local debugging, but it is not enough to answer operational
questions at production scale:

- Which package release is active on each app instance?
- Did every active instance receive `sha256:...`?
- Which instances are stale, pinned, failing refresh, or still on the previous
  package?
- When did each instance accept the new package?
- Can a support or customer-facing view show which configuration version
  influenced a request?

The SDK cannot answer those questions alone because it does not know fleet
membership, service identity, deployment identity, or customer identity. The
application and its observability system know those things. Rototo should expose
the package and refresh facts that those systems need.

## Goals

- Give each loaded package a stable, serializable identity.
- Emit structured refresh events for initial load, successful refresh, unchanged
  probes, immutable sources, and failed refresh attempts.
- Make events suitable for logs, OpenTelemetry, metrics, app audit tables, and
  customer-facing views.
- Preserve last-known-good behavior: emit `refreshed` only after the new package
  has been loaded, linted, compiled into the runtime model, and made current.
- Support Git sources, HTTPS archive sources, local sources, and layered
  packages.
- Keep package refresh observability independent from variable resolution
  semantics.
- Keep application-specific data, such as service name, region, instance id,
  account id, and user id, outside the Rototo SDK.
- Provide a rollout completion pattern that operations teams can implement with
  their existing telemetry backend.

## Non-Goals

- Rototo does not collect telemetry from applications.
- Rototo does not define fleet membership or decide which instances count
  toward rollout completion.
- Rototo does not store customer-facing audit history.
- Rototo does not log raw evaluation context by default.
- Rototo does not expose secrets or bearer tokens in source strings.
- Rototo does not require a specific observability provider.
- Rototo does not make refresh events part of the package format.

## Current State

The Rust SDK currently exposes:

- `Package::source_fingerprint() -> Option<&SourceFingerprint>`
- `Package::immutable_source() -> bool`
- `Package::source_layers() -> &[SourceLayer]`
- `RefreshingPackage::status() -> RefreshStatus`
- `RefreshingPackage::refresh_now() -> Result<RefreshOutcome>`

`RefreshStatus` includes:

- `current_fingerprint`
- `last_success`
- `last_attempt`
- `consecutive_failures`
- `last_error`
- `refreshing`
- `immutable`

`SourceFingerprint` can represent:

- `GitCommit(String)`
- `HttpValidator(String)`
- `ContentHash(String)`
- `PackageLayers(Vec<SourceFingerprint>)`

The language SDKs already expose refresh status in JSON-shaped forms.

This is close to what operators need, but the current surface has two gaps:

- There is no durable event stream. Apps can poll status, but they cannot hook
  directly into refresh state transitions.
- There is no named package identity type that combines fingerprint, source
  redaction, loaded time, immutability, and layered package details into one
  stable object.

## Design Principles

### The SDK Reports, the App Publishes

Rototo should report package facts. The app should publish them. That keeps
Rototo independent from telemetry vendors and lets each application attach the
labels its operations team already uses.

For example, Rototo can emit:

```json
{
  "eventType": "refreshed",
  "current": {
    "releaseId": "sha256:4d1c...",
    "fingerprint": { "kind": "content_hash", "value": "sha256:4d1c..." }
  }
}
```

The application adds:

```json
{
  "service": "checkout-api",
  "environment": "prod",
  "region": "us-east-1",
  "instanceId": "checkout-api-6f7d9c8c9f-g82qp",
  "deploymentId": "2026-06-29.4"
}
```

### Package Identity Is the Rollout Unit

Operators need one value to compare across the fleet. For release-time archive
distribution, that value should be the artifact digest, for example
`sha256:4d1c...`.

Git commit ids are good package identities for Git sources. Content hashes are
good package identities for immutable archives. HTTP validators are acceptable
for change detection, but they are weaker as customer-facing release ids unless
the release pipeline controls them and makes them digest-like.

### Events Must Follow State Changes

The SDK should emit a successful refresh event only after the current package
handle has been swapped to the newly loaded package. If lint or loading fails,
the SDK should emit a failure event and keep reporting the previous package as
current.

### Customer-Facing Data Must Be Deliberate

A customer-facing view should not expose raw package source URLs, bearer tokens,
full evaluation context, or internal rule expressions unless the application
chooses to reveal them. Rototo should provide enough structured data for the app
to build a safe view.

## Package Identity

Add a public `PackageIdentity` type to the Rust SDK:

```rust
pub struct PackageIdentity {
    pub source: RedactedPackageSource,
    pub fingerprint: Option<SourceFingerprint>,
    pub release_id: Option<String>,
    pub loaded_at: SystemTime,
    pub immutable: bool,
    pub layers: Vec<PackageLayerIdentity>,
}

pub struct PackageLayerIdentity {
    pub source: RedactedPackageSource,
    pub fingerprint: Option<SourceFingerprint>,
    pub release_id: Option<String>,
    pub immutable: bool,
}

pub struct RedactedPackageSource(String);
```

`Package::identity()` should return the identity of the currently loaded
package. `RefreshingPackage::identity()` should return
`self.current().identity()`.

`loaded_at` is the time at which this package instance was accepted by the SDK.
For an initial load, it is the successful load time. For refresh, it is the time
the new package becomes current.

`release_id` is a best-effort stable label derived from the fingerprint:

- `SourceFingerprint::GitCommit(commit)` -> `git:<commit>`
- `SourceFingerprint::ContentHash("sha256:...")` -> `sha256:...`
- `SourceFingerprint::HttpValidator("etag:\"sha256:...\"")` ->
  `sha256:...` when it clearly contains a digest
- `SourceFingerprint::HttpValidator(value)` -> `http:<stable-hash(value)>`
  if a digest cannot be derived
- `SourceFingerprint::PackageLayers(layers)` -> stable hash of layer release ids

The exact derivation should be deterministic and specified. The release
pipeline for issue 49 should set the archive `ETag` to the artifact digest, or
serve immutable digest URLs that cause the SDK to use a content hash. That gives
operators and customer-facing views the same package release id.

### Source Redaction

Package sources can contain credentials in URLs or identify private repository
paths. The SDK should expose a redacted source string:

- strip userinfo from URLs;
- never include bearer tokens;
- preserve scheme, host, path, ref, and subdir when safe;
- allow applications to opt into further redaction before publishing.

Example:

```text
git+https://github.com/acme/runtime-config.git#main:packages/checkout
https://config.acme.com/rototo/checkout/prod/current.tar.gz
```

## Refresh Events

Add a public `RefreshEvent` type:

```rust
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

pub enum RefreshEventType {
    Loaded,
    RefreshStarted,
    Unchanged,
    Refreshed,
    Failed,
    Immutable,
    Shutdown,
}

pub struct SdkIdentity {
    pub name: &'static str,
    pub version: &'static str,
    pub language: &'static str,
}
```

`Loaded` is emitted after initial successful load. `RefreshStarted` is optional
for applications that want in-progress visibility. `Unchanged` is emitted after
a successful probe that confirms the source has not changed. `Refreshed` is
emitted after the package becomes current. `Failed` is emitted after a failed
refresh attempt. `Immutable` is emitted when refresh discovers an immutable
source and disables further refresh. `Shutdown` is emitted when a background
refresh loop shuts down cleanly.

Applications usually care about `Loaded`, `Refreshed`, and `Failed`.
Operations teams can use `Unchanged` as heartbeat-like evidence that refresh is
still probing successfully, but it should not replace the app's own liveness
signal.

### Event JSON Shape

Language SDKs should expose events in a consistent JSON shape:

```json
{
  "schemaVersion": 1,
  "eventId": "018f1a5f-6c0e-7f6a-b20d-8f3b1a1f66b7",
  "eventType": "refreshed",
  "source": "https://config.acme.com/rototo/checkout/prod/current.tar.gz",
  "previous": {
    "releaseId": "sha256:1111",
    "fingerprint": { "kind": "content_hash", "value": "sha256:1111" },
    "loadedAt": 1782699600.120,
    "immutable": false,
    "layers": []
  },
  "current": {
    "releaseId": "sha256:2222",
    "fingerprint": { "kind": "content_hash", "value": "sha256:2222" },
    "loadedAt": 1782699662.481,
    "immutable": false,
    "layers": []
  },
  "attemptedAt": 1782699662.120,
  "completedAt": 1782699662.481,
  "durationMs": 361,
  "outcome": "refreshed",
  "consecutiveFailures": 0,
  "error": null,
  "sdk": {
    "name": "rototo",
    "version": "0.1.0-alpha.5",
    "language": "rust"
  }
}
```

For `Failed`, `current` remains the last-known-good package and `previous`
should usually be omitted or equal to `current`; the event's `error` carries the
failed attempt. The failed package must not be reported as current.

## Event Delivery API

Rust should support two usage patterns:

- a non-blocking observer callback for direct integration with tracing or
  metrics;
- an event subscription for applications that want to forward events from an
  async task.

### Observer Callback

```rust
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
```

`RefreshOptions` can accept an observer:

```rust
let package = RefreshingPackage::load(
    source,
    RefreshOptions::new()
        .with_period(Duration::from_secs(30))
        .with_observer(|event| {
            tracing::info!(
                event_type = ?event.event_type,
                release_id = event.current.as_ref().and_then(|id| id.release_id.as_deref()),
                "rototo package refresh event"
            );
        }),
).await?;
```

The SDK must never let observer failure break refresh. The observer should be
best-effort. If the observer panics, Rust should catch unwind only if doing so
does not complicate the SDK substantially; otherwise the SDK guide should
require observers not to panic. Observers must not perform blocking I/O.

### Event Subscription

Expose a bounded channel backed by `tokio::sync::broadcast` or `watch`:

```rust
let package = RefreshingPackage::load(source, refresh_options).await?;
let mut events = package.subscribe_refresh_events();

tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        app_observability.record_rototo_refresh(event);
    }
});
```

A broadcast channel fits multiple consumers and does not block refresh. If the
consumer lags and events are dropped, the app can recover by reading
`package.status()` and `package.identity()`.

### Default Tracing

The SDK should keep emitting `tracing` logs for success and failure, but those
logs should include structured fields derived from the same event data:

```text
event_type=refreshed
release_id=sha256:2222
previous_release_id=sha256:1111
source=https://config.acme.com/rototo/checkout/prod/current.tar.gz
duration_ms=361
```

This gives existing users immediate value even before they adopt callbacks or
subscriptions.

## Status API

Extend `RefreshStatus` or add `RefreshSnapshot`:

```rust
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
```

The current `RefreshStatus` should remain available. `RefreshSnapshot` is the
better surface for operational export because it joins status with package
identity.

Applications should publish this snapshot periodically as a heartbeat:

```json
{
  "service": "checkout-api",
  "environment": "prod",
  "region": "us-east-1",
  "instanceId": "checkout-api-6f7d9c8c9f-g82qp",
  "rototo": {
    "releaseId": "sha256:2222",
    "lastSuccess": 1782699662.481,
    "lastAttempt": 1782699690.002,
    "consecutiveFailures": 0,
    "immutable": false
  }
}
```

Events tell operations what changed. Snapshots tell operations what is true now.
Rollout completion should use snapshots, not logs alone.

## Rollout Completion

Rototo should describe a rollout completion query, but it should not implement a
fleet coordinator.

The operations system needs:

- target release id, for example `sha256:2222`;
- active instance set, from service discovery, orchestration, or telemetry;
- recent `RefreshSnapshot` for each active instance;
- timeout and freshness policy.

The rollout is complete when:

```text
for every active instance:
  snapshot.rototo.releaseId == target_release_id
  snapshot.reportedAt >= now - freshness_window
  snapshot.rototo.consecutiveFailures == 0 or failures are accepted
```

Instances that are terminating, draining, or outside the active fleet should be
excluded by the app platform, not Rototo.

For channel-based archive sources, the release system should know the target
digest before moving the channel pointer. It can store:

```json
{
  "channel": "prod/current",
  "targetReleaseId": "sha256:2222",
  "publishedAt": 1782699600.000
}
```

The operations dashboard then joins that target with instance snapshots.

## Customer-Facing Views

Some applications need to show users which configuration version is active. The
SDK should provide safe primitives, not a complete UI.

A basic customer-facing status can expose:

```json
{
  "configuration": {
    "releaseId": "sha256:2222",
    "channel": "prod/current",
    "loadedAt": 1782699662.481,
    "status": "current"
  }
}
```

For a support view that explains a selected value, the application can combine:

- `PackageIdentity.release_id`;
- variable id;
- selected `VariableResolution.source`;
- selected value, if safe to show;
- qualifier outcomes, if safe to show;
- a redacted summary of the evaluation context.

Example:

```json
{
  "configurationReleaseId": "sha256:2222",
  "variable": "checkout-redesign",
  "selectedSource": {
    "kind": "catalog",
    "catalog": "checkout-redesign",
    "value": "premium"
  },
  "qualifiers": [
    { "id": "premium-users", "value": true }
  ]
}
```

The application decides what is customer-visible. Rototo should not assume that
rule text, catalog ids, or qualifier ids are safe to reveal to every user.

## Privacy and Safety

Refresh events and package identity must not include:

- bearer tokens;
- raw request context;
- user identifiers;
- account identifiers;
- secrets embedded in package source URLs;
- full package file contents.

Resolution provenance may include selected values. Those values can be sensitive
depending on the application. The SDK should expose provenance to the app, but
the app must choose what to publish.

Errors can leak source paths or hostnames. SDK error strings in events should
use the same redacted source behavior as normal logs.

## Interaction With Release-Time Artifact Distribution

Issue 49 proposes projecting a reviewed package into an immutable,
content-addressed archive and serving it through object storage plus a CDN.
Refresh observability should use that release model directly.

Release should produce:

- immutable archive: `sha256:<digest>.tar.gz`;
- channel pointer: `prod/current.tar.gz` or equivalent;
- expected release id: `sha256:<digest>`;
- HTTP validator that reflects the digest where possible.

Runtime instances report the digest after they accept the package. Operations
compare instance reports to the channel target digest. Rollback moves the
channel pointer back to a previous digest, and rollout completion uses the same
mechanism.

If the CDN only provides opaque ETags, the release process should publish a
sidecar channel metadata object:

```json
{
  "channel": "prod/current",
  "releaseId": "sha256:2222",
  "archive": "sha256:2222.tar.gz",
  "publishedAt": 1782699600.000
}
```

The SDK does not need to fetch this metadata in v1. It is enough for the release
system and observability dashboard to know the target release id. A later SDK
feature can read metadata headers or sidecars if needed.

## Language SDK Mapping

All language SDKs should expose the same concepts, adapted to local idioms.

Rust:

```rust
let snapshot = package.snapshot();
let mut events = package.subscribe_refresh_events();
```

Python:

```python
async for event in package.refresh_events():
    await observability.record_rototo_refresh(event)
```

TypeScript:

```ts
for await (const event of pkg.refreshEvents()) {
  observability.recordRototoRefresh(event)
}
```

Go:

```go
events := pkg.RefreshEvents(ctx)
for event := range events {
    observability.RecordRototoRefresh(event)
}
```

Java:

```java
pkg.addRefreshListener(event -> observability.recordRototoRefresh(event));
```

The JSON representation of `PackageIdentity`, `RefreshEvent`, and
`RefreshSnapshot` should be shared across language bindings to keep dashboards
and contract tests portable.

## Implementation Plan

### Phase 1: Identity and Snapshot

- Add `PackageIdentity`, `PackageLayerIdentity`, and `RedactedPackageSource`.
- Store `loaded_at` in `Package`.
- Add `Package::identity()` and `RefreshingPackage::identity()`.
- Add `RefreshSnapshot` or extend status JSON to include identity.
- Update language SDKs to expose identity and snapshot.
- Add tests for Git, HTTPS archive, content hash, HTTP validator, and layered
  package identity.

This phase lets apps poll and publish current state, which is enough for basic
rollout completion.

### Phase 2: Refresh Events

- Add `RefreshEvent` and `RefreshEventType`.
- Emit `Loaded` after initial load.
- Emit `Unchanged`, `Refreshed`, `Failed`, and `Immutable` from refresh paths.
- Add non-blocking observer support.
- Add subscription support where runtime constraints allow it.
- Update structured `tracing` logs to include event fields.
- Add tests that successful refresh emits a new current identity and failed
  refresh leaves current identity unchanged.

### Phase 3: Cross-Language Event APIs

- Expose event streams or listeners in Python, TypeScript, Go, and Java.
- Add shared JSON contract fixtures for identity, snapshot, and events.
- Add package smoke tests that install each SDK and verify event delivery.

### Phase 4: Docs

- Extend production workflow docs with rollout observability.
- Add SDK reference docs for identity, status, snapshot, and refresh events.
- Add examples for logs, OpenTelemetry, and a customer-facing status endpoint.

## Testing Strategy

Rust tests:

- initial load produces `Loaded` event and identity;
- manual refresh with changed Git source produces `Refreshed` with previous and
  current identities;
- failed refresh emits `Failed` and keeps old identity current;
- unchanged probe emits `Unchanged` without changing identity;
- immutable pinned source emits `Immutable`;
- layered package identity changes when a parent layer changes;
- redacted source never includes bearer tokens or URL userinfo.

Cross-language contract tests:

- identity JSON shape is stable;
- refresh snapshot JSON shape is stable;
- refresh event JSON shape is stable;
- event names use the same `snake_case` values across SDKs.

Operational example tests:

- example app records a refresh event into a fake observability sink;
- example app exposes a status endpoint built from `RefreshSnapshot`;
- example rollout query marks completion only when all active instances report
  the target release id.

## Resolved Decisions

These were open questions during design. They are now settled for v1. The
guiding principle is that every "should we also build X" lands on defer, so the
first cut stays a minimal slice, while the wire contract reserves room so later
additions stay non-breaking.

### `RefreshStarted` is opt-in, off by default

v1 emits terminal events only. The `RefreshStarted` variant stays in the
`RefreshEventType` enum so adding emission later is not a breaking change, but
the refresh loop does not emit it unless an application opts in through
`RefreshOptions`. "A refresh is in flight right now" is already answered more
cheaply by `RefreshSnapshot.refreshing` (poll) and, after the fact, by
`durationMs` on the terminal event. Emitting a start event every probe would
mostly add noise paired with `Unchanged`.

### Observer callbacks are synchronous and best-effort

The SDK does not accept async observer callbacks. The observer runs synchronous,
non-blocking work only (increment a metric, emit a structured `tracing` line).
Anything that performs I/O bridges through `subscribe_refresh_events()` and runs
on the application's own task. An async observer would force the SDK to await
application futures inside the refresh critical section, which fights the
invariant that observer failure must never break refresh. Higher-level language
SDKs expose only the async event stream and omit the sync callback, removing the
"do not await in here" footgun.

### `release_id` is derived from `SourceFingerprint` in v1

There is no dedicated release-metadata header or sidecar fetch in v1. The
derivation table is the whole contract, it is deterministic, and `release_id`
stays `Option<String>` so a local source with no fingerprint reports no release
id rather than a fabricated one. When the issue 49 release pipeline lands and
sets the archive `ETag` to the artifact digest, the same `HttpValidator` path
yields `sha256:...` with no SDK change. A later feature may read sidecar
metadata if opaque-ETag CDNs make derivation insufficient.

### Customer-facing provenance gets no dedicated redaction helpers in v1

The SDK exposes structured resolution provenance (`VariableResolution.source`,
qualifier outcomes) and `PackageIdentity.release_id`; the application chooses
what is customer-visible. A shared redactor would have to encode a policy about
which catalog ids, rule text, or qualifier ids are safe to reveal, and that
policy is application-specific. Revisit only when a concrete app needs a shared
redactor.

### Event subscription lives on `RefreshingPackage`, not `Package`

`Package::identity()` exists on both a plainly loaded package and a refreshing
one, because any loaded package has a reportable release id. But
`subscribe_refresh_events()` and `snapshot()` live only on `RefreshingPackage`.
A `Package` from `Package::load` never refreshes; its only lifecycle event is
`Loaded`, which the caller already holds as the return value of `load`. Manual
refresh is a `RefreshingPackage` with no period where the app calls
`refresh_now()` itself, and that already yields the full event stream.

## Recommendation

Implement identity and snapshot first. That gives operations teams a polling
surface for rollout completion with minimal SDK complexity. Then add refresh
events as a non-blocking stream so applications can record exact transition
times and feed their existing observability pipelines.

The release pipeline should make `sha256:<digest>` the operational package
version. The SDK should surface that version consistently. The application
should attach service, instance, environment, and customer context only at the
observability boundary.
