# Application Integration

Once the workspace model is clear, the next question is how the application
should use it. This is where rototo either becomes a clean runtime boundary or
turns into another config format that application code quietly reimplements.

The application should not parse workspace files. It should not duplicate
qualifier logic. It should not know how values are arranged on disk. The app is
deployed with a workspace source URI, loads that source through the SDK, builds
context from facts it owns, and resolves named variables at the boundary where
runtime behavior is selected.

That shape keeps the control plane in one place and gives production systems a
way to explain which value was selected from which workspace version.

## Load A Workspace Source

Application configuration should point at a workspace source:

```text
ROTOTO_WORKSPACE_SOURCE=git+https://github.com/acme/runtime-config.git#main:workspaces/prod
```

The app should load that source through the SDK:

```rust
use rototo::Workspace;

let source = std::env::var("ROTOTO_WORKSPACE_SOURCE")?;
let workspace = Workspace::load(source).await?;
```

`Workspace::load` stages the source, inspects the workspace, runs lint, and
compiles the runtime model. If lint fails, load fails. That is the behavior I
want at application startup: a service should not quietly start from a broken
control plane.

For tools that need to inspect broken workspaces, use `Workspace::inspect`.
For application runtime paths, prefer `Workspace::load` or
`RefreshingWorkspace::load`.

## Resolve At The Behavior Boundary

Resolve variables where the application crosses from request facts into
behavior selection.

For an HTTP service, that is often near the handler, use-case, or policy
boundary:

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

That placement matters. If resolution is scattered through low-level helpers,
it becomes hard to see which runtime decisions a request can make. If the app
resolves too early and passes selected values everywhere, it can become hard to
log and debug why the decision happened.

Keep the boundary narrow: build context, resolve the variable, convert the
selected JSON value into an app type, and pass the typed policy inward.

## Build Context From App-Owned Facts

The application owns the runtime facts. It should build context from request,
account, environment, and service state it already trusts:

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

Do not precompute rototo policy in the application context:

```json
{
  "use_enterprise_limits": true
}
```

That hides the condition rototo is supposed to explain. The app should provide
facts. The workspace should decide what those facts mean.

`schemas/context.schema.json` is the contract between the app and workspace.
When the schema exists, SDK resolution validates context by default. If the app
forgets a required fact or sends the wrong type, the failure happens before
predicate evaluation.

## Prefer RefreshingWorkspace For Services

Configuration is deployed separately from the application binary. Long-running
services usually need to pick up reviewed workspace changes without a restart.

Use `RefreshingWorkspace` for that path:

```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingWorkspace};

let source = std::env::var("ROTOTO_WORKSPACE_SOURCE")?;
let refresh = RefreshOptions::new()
    .with_period(Duration::from_secs(30))
    .with_failure_backoff(Duration::from_secs(5), Duration::from_secs(300));

let workspace = RefreshingWorkspace::load(source, refresh).await?;
```

Initial load must succeed. After that, successful refreshes affect future
resolutions. Failed refreshes keep the last successfully loaded workspace
active.

That last-known-good behavior is the operational bargain. A bad workspace
commit should not automatically take down a running service that already has a
valid workspace. It should show up as a refresh failure, keep serving the
previous workspace, and give operators a clear signal to fix or revert the
workspace change.

Pinned commit sources are different. If the source is pinned to a full commit
SHA, refresh is reproducible but it will not discover later commits. Use pinned
sources for jobs and reproducible deploys. Use branch or tag sources when the
service should receive reviewed configuration updates from the same source
URI.

## Convert To App Types At One Edge

Rototo returns JSON values because the workspace is language-neutral. The app
should convert those values into app-native types at a narrow edge:

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

Keep that conversion close to the resolution call. It gives tests one place to
assert the app's expectations, and it keeps the rest of the codebase working
with ordinary domain types instead of raw JSON.

If conversion fails, treat it as a contract failure between the app and
workspace. In most services that should be logged with enough context to
identify the variable id, value key, and workspace fingerprint.

## Log The Selection, Not The Whole Payload

For most production debugging, the important fields are:

- variable id;
- selected value key;
- workspace fingerprint;
- relevant request or account identifier;
- refresh status when investigating freshness.

For example:

```rust
tracing::info!(
    variable = "account-limit-profile",
    value_key = %resolution.value_key,
    workspace_fingerprint = ?workspace.current().await.source_fingerprint(),
    account_id = %account.id,
    "resolved runtime configuration"
);
```

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

For refresh failures, keep serving last-known-good and expose status:

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

## Keep Policy Out Of Low-Level Helpers

It is tempting to hide resolution behind helpers like:

```rust
async fn max_projects(account: &Account) -> u64
```

That can be fine if it is the application boundary for account limits. It is a
problem if dozens of helpers each resolve their own variables and reassemble a
policy the workspace could have selected as one object.

Prefer an integration shape that makes runtime decisions visible:

```rust
let profile = account_limit_policy.resolve(&workspace, &account).await?;
project_service.create_project(account, profile).await?;
```

The service gets a typed policy. The rototo-facing boundary remains small,
testable, and observable.

## What Not To Do

Avoid these patterns:

- parsing `variables/*.toml` or `resources/*.toml` from application code;
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
