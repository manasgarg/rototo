# SDK Refresh Reference

Configuration is deployed separately from the application binary. A
long-running service should be able to pick up a newly reviewed workspace
version without restarting, while continuing to serve the last known good
workspace when refresh fails.

`RefreshingWorkspace` is the SDK type for that model.

## Load With Refresh

:::sdk-snippet load-refreshing-workspace
```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingWorkspace};

let refresh = RefreshOptions::new()
    .with_period(Duration::from_secs(30));

let workspace = RefreshingWorkspace::load(source, refresh).await?;
```

```python
import rototo

workspace = await rototo.RefreshingWorkspace.load(
    source,
    period_seconds=30,
)
```
:::

Initial load stages the source, lints it, compiles the runtime model, and makes
that workspace current. If initial load fails, the service has no active
workspace and the call returns an error.

## Resolution

`RefreshingWorkspace` exposes the same runtime resolution methods:

:::sdk-snippet refreshing-resolution
```rust
let resolution = workspace
    .resolve_variable("account-limits", &context)
    .await?;
```

```python
resolution = await workspace.resolve_variable(
    "account-limits",
    context,
)
```
:::

Each call resolves against the current successfully loaded workspace. A
successful refresh affects future resolutions. It does not mutate a resolution
already returned to application code.

## Manual Refresh

:::sdk-snippet manual-refresh
```rust
let outcome = workspace.refresh_now().await?;
```

```python
outcome = await workspace.refresh_now()
```
:::

Refresh outcomes:

| Outcome | Meaning |
| --- | --- |
| `unchanged` | Source fingerprint did not change. |
| `refreshed` | A changed source loaded, linted, and replaced the current workspace. |
| `immutable` | Source is pinned to an immutable commit. |

## Periodic Refresh

Periodic refresh starts a background refresh loop for mutable sources.

:::sdk-snippet periodic-refresh
```rust
let refresh = RefreshOptions::new()
    .with_period(Duration::from_secs(60))
    .with_failure_backoff(Duration::from_secs(5), Duration::from_secs(300));
```

```python
workspace = await rototo.RefreshingWorkspace.load(
    source,
    period_seconds=60,
)
```
:::

On refresh failure, rototo keeps the current workspace active and backs off
before the next attempt. The default Rust backoff starts at 5 seconds and caps
at 300 seconds. The first Python SDK release uses the Rust defaults.

## Immutable Sources

A git source pinned to a full 40-character commit SHA is immutable. Periodic
refresh is disabled for immutable sources, because there is no later workspace
version to discover from that source string.

Commit-pinned sources are good for reproducible jobs and tests. Branch or tag
sources are the usual fit for long-running services that should receive
reviewed configuration updates.

## Status

:::sdk-snippet refresh-status
```rust
let status = workspace.status().await;
```

```python
status = await workspace.status()
```
:::

`RefreshStatus` contains:

| Field | Meaning |
| --- | --- |
| `current_fingerprint` | Fingerprint of the active workspace source. |
| `last_success` | Last successful initial load or refresh time. |
| `last_attempt` | Last refresh attempt time. |
| `consecutive_failures` | Count of refresh failures since the last success. |
| `last_error` | Last refresh error message, if any. |
| `refreshing` | Whether a refresh attempt is currently running. |
| `immutable` | Whether the current source is immutable. |

Rust also exposes `RefreshStatus::stale(max_staleness)`.

## Shutdown

:::sdk-snippet refresh-shutdown
```rust
workspace.shutdown().await;
```

```python
await workspace.shutdown()
```
:::

`shutdown` stops the background refresh loop and waits for it. Dropping the
refreshing workspace also signals shutdown and aborts the task if needed, but
explicit shutdown gives services a cleaner stop path.

## Observability

A service should expose the active fingerprint, refresh failures, and staleness
state. Those are the fields that tell an operator whether the service is using
the latest reviewed workspace or deliberately serving the last known good
version.
