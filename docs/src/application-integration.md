# Application Integration

Once the workspace model is clear, the next question is how the application
should use it. This is where rototo either becomes a clean runtime boundary or
turns into another config format that application code quietly reimplements.

The application should not parse workspace files. It should not duplicate
qualifier logic. It should not know how values are arranged on disk. The app is
deployed with a [workspace source](reference-workspace-sources.html) URI,
[loads that source through the SDK](reference-sdk-loading.html), builds context
from facts it owns, and
[resolves named variables](reference-sdk-resolution.html) at the boundary where
runtime behavior is selected.

That keeps the control plane in one place. It also gives the service a clear
answer when someone asks which value was selected, and from which workspace
version.

## Load A Workspace Source

Application configuration should point at a workspace source:

```text
ROTOTO_WORKSPACE_SOURCE=git+https://github.com/acme/runtime-config.git#main:workspaces/prod
```

The app should load that source through the SDK:

:::sdk-snippet application-load-workspace
```rust
use rototo::Workspace;

let source = std::env::var("ROTOTO_WORKSPACE_SOURCE")?;
let workspace = Workspace::load(source).await?;
```

```python
import os
import rototo

source = os.environ["ROTOTO_WORKSPACE_SOURCE"]
workspace = await rototo.Workspace.load(source)
```

```typescript
import { Workspace } from "rototo";

const source = process.env.ROTOTO_WORKSPACE_SOURCE;
if (!source) {
  throw new Error("ROTOTO_WORKSPACE_SOURCE is required");
}

const workspace = await Workspace.load(source);
```

```java
import dev.rototo.Workspace;

String source = System.getenv("ROTOTO_WORKSPACE_SOURCE");
Workspace workspace = Workspace.load(source).get();
```

```go
import (
    "context"
    "os"

    rototo "github.com/manasgarg/rototo/sdks/go"
)

source := os.Getenv("ROTOTO_WORKSPACE_SOURCE")
workspace, err := rototo.Load(context.Background(), source, nil)
if err != nil {
    return err
}
defer workspace.Close()
```
:::

`Workspace::load` stages the source, inspects the workspace, runs lint, and
compiles the runtime model. If lint fails, load fails. That is the behavior I
want at application startup: a service should not quietly start from a broken
control plane.

For tools that need to inspect broken workspaces, use `Workspace::inspect`.
For application runtime paths, prefer
[`Workspace::load`](reference-sdk-loading.html) or
[`RefreshingWorkspace::load`](reference-sdk-refresh.html).

## Resolve At The Behavior Boundary

Resolve variables where the application crosses from request facts into
behavior selection.

For an HTTP service, that is often near the handler, use-case, or policy
boundary:

:::sdk-snippet application-resolve-boundary
```rust
let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "id": account.id,
        "plan": account.plan,
        "seats": account.seats
    },
    "request": {
        "country": request.country
    }
}))?;

let resolution = workspace
    .resolve_variable("account-limit-profile", &context)
    .await?;
```

```python
context = {
    "account": {
        "id": account.id,
        "plan": account.plan,
        "seats": account.seats,
    },
    "request": {
        "country": request.country,
    },
}

resolution = await workspace.resolve_variable(
    "account-limit-profile",
    context,
)
```

```typescript
const context = {
  account: {
    id: account.id,
    plan: account.plan,
    seats: account.seats,
  },
  request: {
    country: request.country,
  },
};

const resolution = await workspace.resolveVariable(
  "account-limit-profile",
  context,
);
```

```java
Map<String, Object> context = Map.of(
    "account", Map.of(
        "id", account.id(),
        "plan", account.plan(),
        "seats", account.seats()
    ),
    "request", Map.of(
        "country", request.country()
    )
);

VariableResolution resolution = workspace
    .resolveVariable("account-limit-profile", context)
    .get();
```

```go
resolveContext := map[string]any{
    "account": map[string]any{
        "id":    account.ID,
        "plan":  account.Plan,
        "seats": account.Seats,
    },
    "request": map[string]any{
        "country": request.Country,
    },
}

resolution, err := workspace.ResolveVariable(
    ctx,
    "account-limit-profile",
    resolveContext,
    nil,
)
if err != nil {
    return err
}
```
:::

That placement matters. If resolution is scattered through low-level helpers,
it becomes hard to see which runtime decisions a request can make. If the app
resolves too early and passes selected values everywhere, it can become hard to
log and debug why the decision happened.

Keep the boundary narrow: build context, resolve the variable, convert the
selected JSON value into an app type, and pass the typed policy inward.

## Build Context From App-Owned Facts

The application owns the runtime facts. It should build context from request,
account, environment, and service state it already trusts:

:::sdk-snippet application-build-context
```rust
let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "id": account.id,
        "plan": account.plan,
        "seats": account.seats
    },
    "service": {
        "lane": deployment.lane
    }
}))?;
```

```python
context = {
    "account": {
        "id": account.id,
        "plan": account.plan,
        "seats": account.seats,
    },
    "service": {
        "lane": deployment.lane,
    },
}
```

```typescript
const context = {
  account: {
    id: account.id,
    plan: account.plan,
    seats: account.seats,
  },
  service: {
    lane: deployment.lane,
  },
};
```

```java
Map<String, Object> context = Map.of(
    "account", Map.of(
        "id", account.id(),
        "plan", account.plan(),
        "seats", account.seats()
    ),
    "service", Map.of(
        "lane", deployment.lane()
    )
);
```

```go
resolveContext := map[string]any{
    "account": map[string]any{
        "id":    account.ID,
        "plan":  account.Plan,
        "seats": account.Seats,
    },
    "service": map[string]any{
        "lane": deployment.Lane,
    },
}
```
:::

Do not precompute rototo policy in the application context:

```json
{
  "use_enterprise_limits": true
}
```

That hides the condition rototo is supposed to explain. The app should provide
facts. The workspace should decide what those facts mean.

[`schemas/context.schema.json`](reference-context.html) is the contract between
the app and workspace. When the schema exists, SDK resolution validates context
by default. If the app forgets a required fact or sends the wrong type, the
failure happens before predicate evaluation.

## Prefer RefreshingWorkspace For Services

Configuration is deployed separately from the application binary. Long-running
services usually need to pick up reviewed workspace changes without a restart.

Use [`RefreshingWorkspace`](reference-sdk-refresh.html) for that path:

:::sdk-snippet application-refreshing-workspace
```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingWorkspace};

let source = std::env::var("ROTOTO_WORKSPACE_SOURCE")?;
let refresh = RefreshOptions::new().with_period(Duration::from_secs(30));

let workspace = RefreshingWorkspace::load(source, refresh).await?;
```

```python
import os
import rototo

source = os.environ["ROTOTO_WORKSPACE_SOURCE"]
workspace = await rototo.RefreshingWorkspace.load(
    source,
    period_seconds=30,
)
```

```typescript
import { RefreshingWorkspace } from "rototo";

const source = process.env.ROTOTO_WORKSPACE_SOURCE;
if (!source) {
  throw new Error("ROTOTO_WORKSPACE_SOURCE is required");
}

const workspace = await RefreshingWorkspace.load(source, {
  periodSeconds: 30,
});
```

```java
RefreshingWorkspaceOptions options = RefreshingWorkspaceOptions.builder()
    .periodSeconds(30.0)
    .build();

String source = System.getenv("ROTOTO_WORKSPACE_SOURCE");
RefreshingWorkspace workspace = RefreshingWorkspace
    .load(source, options)
    .get();
```

```go
periodSeconds := 30.0
source := os.Getenv("ROTOTO_WORKSPACE_SOURCE")

workspace, err := rototo.LoadRefreshing(
    ctx,
    source,
    &rototo.RefreshingWorkspaceOptions{
        PeriodSeconds: &periodSeconds,
    },
)
if err != nil {
    return err
}
defer workspace.Close(ctx)
```
:::

Initial load must succeed. After that, successful refreshes affect future
resolutions. Failed refreshes keep the last successfully loaded workspace
active.

That last-known-good behavior matters in production. A bad workspace commit
should not take down a running service that already has a valid workspace. It
should show up as a refresh failure, keep serving the previous workspace, and
give operators a clear signal to fix or revert the workspace change.

Pinned commit sources are different. If the source is pinned to a full commit
SHA, refresh is reproducible but it will not discover later commits. Use pinned
sources for jobs and reproducible deploys. Use branch or tag sources when the
service should receive reviewed configuration updates from the same source
URI.

## Convert To App Types At One Edge

Rototo returns JSON values because the workspace is language-neutral. The app
should convert those values into app-native types at a narrow edge:

:::sdk-snippet application-convert-value
```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AccountLimitProfile {
    enabled_features: Vec<String>,
    limits: AccountLimits,
}

#[derive(Debug, Deserialize)]
struct AccountLimits {
    projects: u64,
    members: u64,
    monthly_requests: u64,
}

let profile: AccountLimitProfile =
    serde_json::from_value(resolution.value.clone())?;
```

```python
from dataclasses import dataclass

@dataclass
class AccountLimits:
    projects: int
    members: int
    monthly_requests: int

@dataclass
class AccountLimitProfile:
    enabled_features: list[str]
    limits: AccountLimits

payload = resolution.value
profile = AccountLimitProfile(
    enabled_features=list(payload["enabled_features"]),
    limits=AccountLimits(**payload["limits"]),
)
```

```typescript
type AccountLimitProfile = {
  enabled_features: string[];
  limits: {
    projects: number;
    members: number;
    monthly_requests: number;
  };
};

const profile = resolution.value as AccountLimitProfile;
```

```java
record AccountLimits(
    long projects,
    long members,
    long monthlyRequests
) {}

record AccountLimitProfile(
    List<String> enabledFeatures,
    AccountLimits limits
) {}

@SuppressWarnings("unchecked")
Map<String, Object> payload = (Map<String, Object>) resolution.value();
Map<String, Object> limits = (Map<String, Object>) payload.get("limits");

AccountLimitProfile profile = new AccountLimitProfile(
    (List<String>) payload.get("enabled_features"),
    new AccountLimits(
        ((Number) limits.get("projects")).longValue(),
        ((Number) limits.get("members")).longValue(),
        ((Number) limits.get("monthly_requests")).longValue()
    )
);
```

```go
type AccountLimitProfile struct {
    EnabledFeatures []string      `json:"enabled_features"`
    Limits          AccountLimits `json:"limits"`
}

type AccountLimits struct {
    Projects        uint64 `json:"projects"`
    Members         uint64 `json:"members"`
    MonthlyRequests uint64 `json:"monthly_requests"`
}

payload, err := json.Marshal(resolution.Value)
if err != nil {
    return err
}

var profile AccountLimitProfile
if err := json.Unmarshal(payload, &profile); err != nil {
    return err
}
```
:::

Keep that conversion close to the resolution call. It gives tests one place to
assert the app's expectations, and it keeps the rest of the codebase working
with ordinary domain types instead of raw JSON.

If conversion fails, treat it as a contract failure between the app and
workspace. In most services that should be logged with enough context to
identify the variable id, value key, and
[workspace fingerprint](reference-workspace-sources.html).

## Log The Selection, Not The Whole Payload

For most production debugging, the important fields are:

- variable id;
- selected value key;
- workspace fingerprint;
- relevant request or account identifier;
- refresh status when investigating freshness.

For example:

:::sdk-snippet application-log-selection
```rust
tracing::info!(
    variable = "account-limit-profile",
    value_key = %resolution.value_key,
    workspace_fingerprint = ?workspace.current().await.source_fingerprint(),
    account_id = %account.id,
    "resolved runtime configuration"
);
```

```python
logger.info(
    "resolved runtime configuration",
    extra={
        "variable": "account-limit-profile",
        "value_key": resolution.value_key,
        "account_id": account.id,
    },
)
```

```typescript
logger.info("resolved runtime configuration", {
  variable: "account-limit-profile",
  valueKey: resolution.valueKey,
  accountId: account.id,
});
```

```java
logger.info(
    "resolved runtime configuration variable={} valueKey={} accountId={}",
    "account-limit-profile",
    resolution.valueKey(),
    account.id()
);
```

```go
slog.Info(
    "resolved runtime configuration",
    "variable", "account-limit-profile",
    "value_key", resolution.ValueKey,
    "account_id", account.ID,
)
```
:::

Do not log full selected payloads by default. Some configuration is sensitive,
and even non-sensitive payloads make logs noisy. The value key and fingerprint
usually tell you which reviewed workspace content was used. Use `rototo show`,
`rototo inspect`, or repository history when you need to read the full value.

## Handle Failures Deliberately

Startup load failure usually means the process should fail to start. The app
does not have a valid control plane.

Runtime resolution failure needs a product-specific decision. Missing context,
schema validation failure, unknown variable ids, and failed app-type conversion
are usually programmer or release errors. For high-risk behavior, failing
closed is often better than inventing an app-side fallback that bypasses
reviewed policy.

If a feature can degrade safely, make that degradation explicit in app code and
observe it. Do not silently replace rototo with hardcoded defaults in many
call sites. That makes recovery harder because nobody can tell which boundary
made the decision.

For refresh failures, keep serving last-known-good and
[expose status](reference-sdk-refresh.html):

:::sdk-snippet application-refresh-status
```rust
let status = workspace.status().await;
if status.stale(Duration::from_secs(300)) {
    tracing::warn!(
        consecutive_failures = status.consecutive_failures,
        last_error = ?status.last_error,
        "workspace refresh is stale"
    );
}
```

```python
status = await workspace.status()
if status.consecutive_failures > 0:
    logger.warning(
        "workspace refresh is stale",
        extra={
            "consecutive_failures": status.consecutive_failures,
            "last_error": status.last_error,
        },
    )
```

```typescript
const status = await workspace.status();
if (status.consecutiveFailures > 0) {
  logger.warn("workspace refresh is stale", {
    consecutiveFailures: status.consecutiveFailures,
    lastError: status.lastError,
  });
}
```

```java
RefreshStatus status = workspace.status().get();
if (status.consecutiveFailures() > 0) {
    logger.warn(
        "workspace refresh is stale consecutiveFailures={} lastError={}",
        status.consecutiveFailures(),
        status.lastError()
    );
}
```

```go
status, err := workspace.Status(ctx)
if err != nil {
    return err
}
if status.ConsecutiveFailures > 0 {
    slog.Warn(
        "workspace refresh is stale",
        "consecutive_failures", status.ConsecutiveFailures,
        "last_error", status.LastError,
    )
}
```
:::

## Keep Policy Out Of Low-Level Helpers

It is tempting to hide resolution behind helpers like:

```text
max_projects(account) -> number
```

That can be fine if it is the application boundary for account limits. It is a
problem if dozens of helpers each resolve their own variables and reassemble a
policy the workspace could have selected as one object.

Prefer integration code that makes runtime decisions visible:

```text
profile = account_limit_policy.resolve(workspace, account)
project_service.create_project(account, profile)
```

The service gets a typed policy. The rototo-facing boundary remains small,
testable, and observable.

## What Not To Do

Avoid these patterns:

- parsing `variables/*.toml` or `catalogs/*.toml` from application code;
- duplicating qualifier predicates in app conditionals;
- putting policy decisions into context booleans;
- caching selected values forever when refresh is part of the runtime model;
- logging full selected payloads as the normal observability path;
- spreading resolution calls so widely that one request's runtime decisions
  are hard to enumerate.

Those patterns usually work at first. They fail later, when a workspace change
needs to be reviewed, tested, explained, or rolled back under pressure.

## What The App Should Own

An idiomatic integration gives the app clear responsibilities:

- configure the workspace source URI;
- load and refresh the workspace through the SDK;
- build context from facts the app owns;
- resolve named variables at behavior boundaries;
- convert selected JSON values into app types;
- log the selected value key and workspace fingerprint;
- expose refresh status.

The workspace owns the policy. The app owns applying the selected policy to
runtime behavior.
