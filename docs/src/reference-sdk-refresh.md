# SDK Refresh Reference

Configuration is deployed separately from the application binary. A
long-running service should be able to pick up a newly reviewed package
version without restarting, while continuing to serve the last known good
package when refresh fails.

`RefreshingPackage` is the SDK type for that model.

## Load With Refresh

:::sdk-snippet load-refreshing-package
```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingPackage};

let refresh = RefreshOptions::new()
    .with_period(Duration::from_secs(30));

let pkg = RefreshingPackage::load(source, refresh).await?;
```

```python
import rototo

pkg = await rototo.RefreshingPackage.load(
    source,
    period_seconds=30,
)
```

```typescript
import { RefreshingPackage } from "rototo";

const pkg = await RefreshingPackage.load(source, {
  periodSeconds: 30,
});
```

```java
RefreshingPackageOptions options = RefreshingPackageOptions.builder()
    .periodSeconds(30.0)
    .build();

RefreshingPackage pkg = RefreshingPackage
    .load(source, options)
    .get();
```

```go
periodSeconds := 30.0

pkg, err := rototo.LoadRefreshing(
    ctx,
    source,
    &rototo.RefreshingPackageOptions{
        PeriodSeconds: &periodSeconds,
    },
)
if err != nil {
    return err
}
defer pkg.Close(ctx)
```
:::

Initial load [stages the source](reference-sdk-loading.html), lints it,
compiles the runtime model, and makes that package current. If initial load
fails, the service has no active package and the call returns an error.

## Resolution

`RefreshingPackage` exposes the same runtime resolution methods:

:::sdk-snippet refreshing-resolution
```rust
let resolution = pkg
    .resolve_variable("account-limits", &context)?;
```

```python
resolution = pkg.resolve_variable(
    "account-limits",
    context,
)
```

```typescript
const resolution = pkg.resolveVariable(
  "account-limits",
  context,
);
```

```java
VariableResolution resolution = pkg
    .resolveVariable("account-limits", context)
    .get();
```

```go
resolution, err := pkg.ResolveVariable(
    ctx,
    "account-limits",
    resolveContext,
    nil,
)
```
:::

Each call [resolves](reference-sdk-resolution.html) against the current
successfully loaded package. A successful refresh affects future resolutions.
It does not mutate a resolution already returned to application code.

## Manual Refresh

:::sdk-snippet manual-refresh
```rust
let outcome = pkg.refresh_now().await?;
```

```python
outcome = await pkg.refresh_now()
```

```typescript
const outcome = await pkg.refreshNow();
```

```java
String outcome = pkg.refreshNow().get();
```

```go
outcome, err := pkg.RefreshNow(ctx)
```
:::

Refresh outcomes:

| Outcome | Meaning |
| --- | --- |
| `unchanged` | Source fingerprint did not change. |
| `refreshed` | A changed source loaded, linted, and replaced the current package. |
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
pkg = await rototo.RefreshingPackage.load(
    source,
    period_seconds=60,
)
```

```typescript
const pkg = await RefreshingPackage.load(source, {
  periodSeconds: 60,
});
```

```java
RefreshingPackageOptions options = RefreshingPackageOptions.builder()
    .periodSeconds(60.0)
    .build();

RefreshingPackage pkg = RefreshingPackage
    .load(source, options)
    .get();
```

```go
periodSeconds := 60.0

pkg, err := rototo.LoadRefreshing(
    ctx,
    source,
    &rototo.RefreshingPackageOptions{
        PeriodSeconds: &periodSeconds,
    },
)
```
:::

On refresh failure, rototo keeps the current package active and backs off
before the next attempt. The default Rust backoff starts at 5 seconds and caps
at 300 seconds. The first Python and Go SDK releases use the Rust defaults.

## Immutable Sources

A [git source](reference-package-sources.html) pinned to a full 40-character
commit SHA is immutable. Periodic refresh is disabled for immutable sources,
because there is no later package version to discover from that source
string.

Commit-pinned sources are good for reproducible jobs and tests. Branch or tag
sources are the usual fit for long-running services that should receive
reviewed configuration updates.

## Status

:::sdk-snippet refresh-status
```rust
let status = pkg.status();
```

```python
status = await pkg.status()
```

```typescript
const status = await pkg.status();
```

```java
RefreshStatus status = pkg.status().get();
```

```go
status, err := pkg.Status(ctx)
```
:::

`RefreshStatus` contains:

| Field | Meaning |
| --- | --- |
| `current_fingerprint` | Fingerprint of the active package source. |
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
pkg.shutdown().await;
```

```python
await pkg.shutdown()
```

```typescript
await pkg.shutdown();
```

```java
pkg.shutdown().get();
```

```go
err := pkg.Shutdown(ctx)
```
:::

`shutdown` stops the background refresh loop and waits for it. Dropping the
refreshing package also signals shutdown and aborts the task if needed, but
explicit shutdown gives services a cleaner stop path.

## Observability

A service should expose the active fingerprint, refresh failures, and staleness
state. Those are the fields that tell an operator whether the service is using
the latest reviewed package or deliberately serving the last known good
version.
