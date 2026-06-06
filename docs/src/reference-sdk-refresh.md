# SDK Refresh Reference

Configuration is deployed separately from the application binary. A
long-running service should be able to pick up a newly reviewed workspace
version without restarting, while continuing to serve the last known good
workspace when refresh fails.

`RefreshingWorkspace` is the SDK type for that model.

## Load With Refresh

```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingWorkspace};

let refresh = RefreshOptions::new()
    .with_period(Duration::from_secs(30));

let workspace = RefreshingWorkspace::load(source, refresh).await?;
```

Initial load stages the source, lints it, compiles the runtime model, and makes
that workspace current. If initial load fails, the service has no active
workspace and the call returns an error.

## Resolution

`RefreshingWorkspace` exposes the same runtime resolution methods:

```rust
let resolution = workspace
    .resolve_variable("account-limits", &context)
    .await?;
```

Each call resolves against the current successfully loaded workspace. A
successful refresh affects future resolutions. It does not mutate a resolution
already returned to application code.

## Manual Refresh

```rust
let outcome = workspace.refresh_now().await?;
```

`RefreshOutcome` values:

| Outcome | Meaning |
| --- | --- |
| `Unchanged` | Source fingerprint did not change. |
| `Refreshed` | A changed source loaded, linted, and replaced the current workspace. |
| `Immutable` | Source is pinned to an immutable commit. |

## Periodic Refresh

`RefreshOptions::with_period` starts a background refresh loop for mutable
sources.

```rust
let refresh = RefreshOptions::new()
    .with_period(Duration::from_secs(60))
    .with_failure_backoff(Duration::from_secs(5), Duration::from_secs(300));
```

On refresh failure, rototo keeps the current workspace active and backs off
before the next attempt. The default backoff starts at 5 seconds and caps at
300 seconds.

## Immutable Sources

A git source pinned to a full 40-character commit SHA is immutable. Periodic
refresh is disabled for immutable sources, because there is no later workspace
version to discover from that source string.

Commit-pinned sources are good for reproducible jobs and tests. Branch or tag
sources are the usual fit for long-running services that should receive
reviewed configuration updates.

## Status

```rust
let status = workspace.status().await;
```

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

`RefreshStatus::stale(max_staleness)` returns true when the last successful
load is older than the supplied duration.

## Shutdown

```rust
workspace.shutdown().await;
```

`shutdown` stops the background refresh loop and waits for it. Dropping
`RefreshingWorkspace` also signals shutdown and aborts the task if needed, but
explicit shutdown gives services a cleaner stop path.

## Observability

A service should expose the active fingerprint, refresh failures, and staleness
state. Those are the fields that tell an operator whether the service is using
the latest reviewed workspace or deliberately serving the last known good
version.
