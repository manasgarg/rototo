# Production Workflow

The Adopt pages before this one define how I would run rototo in production:
[model the runtime decision](modeling-runtime-configuration.html),
[integrate through the SDK](application-integration.html),
[test the app-workspace contract](testing-runtime-configuration.html), and
[treat workspace changes as releases](operating-runtime-configuration.html).

Here is that approach as one concrete path. We continue the `account-config`
workspace and `account-app` from getting started, then add the pieces I would
want before trusting it in a service: a named condition, a context contract, a
hosted workspace source, workspace policy lint, merge gates, app contract
tests, and runtime observability.

The core split does not change. The application is still deployed with a
workspace source URI. The app still supplies runtime facts. The workspace still
owns the policy for selecting the value.

## Add The Runtime Condition

The first production gap is that account limits should vary by account facts.
Premium accounts should receive a larger `max-active-projects` value, but that
[condition](reference-qualifiers.html) should not be hidden in app code.

Create `account-config/qualifiers/premium-account.toml`:

```toml
schema_version = 1
description = "Requests from premium accounts"

when = 'context.account.plan == "premium"'
```

Update `account-config/variables/max-active-projects.toml`:

```toml
schema_version = 1

description = "Maximum active projects for an account"
type = "int"

[resolve]
default = 3

[[resolve.rule]]
when = 'qualifier["premium-account"]'
value = 25
```

The qualifier gives the condition a name. The variable rule can now say
`premium-account -> 25`, and the app does not need to know how premium is
defined.

## Add The Context Contract

The workspace now depends on `account.plan`. That path is part of the
[context contract](reference-context.html) between the app and workspace, so it
should be validated.

Generate a request context schema skeleton:

```sh
rototo init account-config --context
```

For this workspace, that writes `account-config/request-contexts/request.schema.json`
with the context path used by the qualifier:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": true,
  "properties": {
    "account": {
      "type": "object",
      "additionalProperties": true,
      "properties": {
        "plan": { "type": "string" }
      }
    }
  }
}
```

Review this file. The generator gives you a starting point; it does not know
which fields should be required, which values are allowed, or which app boundary
owns them.

Run lint:

```sh
rototo lint account-config
```

Then resolve both paths with the same facts the app will send:

```sh
rototo resolve account-config \
  --variable max-active-projects \
  --context account.plan=standard

rototo resolve account-config \
  --variable max-active-projects \
  --context account.plan=premium
```

The app should now build context from account facts instead of sending an empty
object:

:::sdk-snippet production-context-facts
```rust
let account_plan =
    std::env::var("ACCOUNT_PLAN").unwrap_or_else(|_| "standard".to_owned());
let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "plan": account_plan
    }
}))?;
```

```python
account_plan = os.environ.get("ACCOUNT_PLAN", "standard")
context = {
    "account": {
        "plan": account_plan,
    },
}
```

```typescript
const accountPlan = process.env.ACCOUNT_PLAN ?? "standard";
const context = {
  account: {
    plan: accountPlan,
  },
};
```

```java
String accountPlan = System.getenv().getOrDefault(
    "ACCOUNT_PLAN",
    "standard"
);
Map<String, Object> context = Map.of(
    "account",
    Map.of("plan", accountPlan)
);
```

```go
accountPlan := os.Getenv("ACCOUNT_PLAN")
if accountPlan == "" {
    accountPlan = "standard"
}
resolveContext := map[string]any{
    "account": map[string]any{
        "plan": accountPlan,
    },
}
```
:::

Run the app as a premium account:

```sh
ACCOUNT_PLAN=premium cargo run -- ../account-config
```

At this point the split is visible: the app supplies facts, and the workspace
decides which configured value those facts select.

## Publish The Workspace Source

A production service should load a
[source](reference-workspace-sources.html) it can fetch from its runtime
environment. Since git is the source of truth, publish `account-config` as a
private repository and pass the app a git workspace URI.

The following commands use the GitHub CLI and SSH. The runtime environment needs
an SSH key or deploy key that can read the repository.

```sh
cd /path/to/account-config

git init .
git add .
git commit -m "Initialize account config workspace"
git branch -M main

GITHUB_OWNER="$(gh api user --jq .login)"
gh repo create "$GITHUB_OWNER/account-config" \
  --private \
  --source . \
  --remote origin \
  --push

export WORKSPACE_URI="git+ssh://git@github.com/${GITHUB_OWNER}/account-config.git#main"
```

Run the app with the hosted source:

```sh
cd /path/to/account-app
ACCOUNT_PLAN=premium cargo run -- "$WORKSPACE_URI"
```

The `#main` ref means refreshes can discover later reviewed commits on `main`.
Use a full commit SHA when a job or deployment needs exact reproducibility; that
source will not discover newer commits through refresh.

## Add Workspace Policy Lint

[Built-in lint](reference-lint-overview.html) protects rototo's structural
contracts. The workspace also needs local policy: account project limits should
be positive, stay under an operational ceiling, and keep the standard plan from
accidentally exceeding the premium plan.

That policy belongs with the workspace because reviewers need to see the values
and the guardrail together.

Create `account-config/lint/max-active-projects.lua`:

```lua
function register(lint)
  lint:rule({
    id = "operations/max-active-projects-policy",
    title = "Account project limit violates operations policy",
    help = "Keep max-active-projects values between 1 and 100 and keep standard <= premium.",
    target = "/variables/max-active-projects",
    handler = "check_max_active_projects",
  })
end

function check_max_active_projects(workspace, variable)
  local values = variable.values or {}
  local diagnostics = {}

  for name, value in pairs(values) do
    if type(value.value) ~= "number" or value.value < 1 or value.value > 100 then
      table.insert(diagnostics, {
        message = "max-active-projects." .. name .. " must be between 1 and 100"
      })
    end
  end

  if values.standard ~= nil
      and values.premium ~= nil
      and type(values.standard.value) == "number"
      and type(values.premium.value) == "number"
      and values.standard.value > values.premium.value then
    table.insert(diagnostics, {
      message = "max-active-projects.standard must not exceed max-active-projects.premium"
    })
  end

  return diagnostics
end
```

Run lint again:

```sh
rototo lint account-config
```

The custom rule uses the `operations/` authority. Built-in rototo rules stay
under the `rototo/` authority, which keeps diagnostic ownership clear.

## Put Gates Before Merge

The workspace repository should reject bad edits before they reach the branch
that services refresh from. Use a local hook for fast feedback and CI for the
shared gate.

Add `.pre-commit-config.yaml` to `account-config`:

```yaml
repos:
  - repo: local
    hooks:
      - id: rototo-lint
        name: rototo lint
        entry: rototo lint .
        language: system
        pass_filenames: false
```

Install the hook:

```sh
pre-commit install
```

Add `.github/workflows/rototo.yml`:

```yaml
name: Rototo

on:
  pull_request:
  push:
    branches:
      - main

permissions:
  contents: read

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install rototo --locked
      - run: rototo lint .
```

Now the workspace has the same release discipline as code: edit, lint locally,
open a PR, run CI, review the runtime behavior delta, and merge.

## Test The App Contract

Workspace lint proves the workspace is valid.
[App tests](testing-runtime-configuration.html) prove the application can still
consume the selected values and apply the policy.

Generate readable behavior fixtures:

```sh
rototo fixtures account-config \
  --variable max-active-projects \
  --qualifier premium-account \
  --out account-app/tests/rototo-fixtures
```

Commit the generated `tests/rototo-fixtures` directory with the app tests. The
fixture diff should be part of review when runtime behavior intentionally
changes.

Add an app contract test in the app's test framework:

:::sdk-snippet production-app-contract-test
```rust
use std::error::Error;

use rototo::{ResolveContext, Workspace};

#[tokio::test]
async fn rototo_workspace_fixtures_still_hold() -> Result<(), Box<dyn Error>> {
    let source = std::env::var("ROTOTO_WORKSPACE_SOURCE")
        .unwrap_or_else(|_| "../account-config".to_owned());
    let workspace = Workspace::load(source).await?;

    let report =
        rototo::testing::assert_fixtures(&workspace, "tests/rototo-fixtures").await?;
    assert!(report.cases > 0);

    Ok(())
}

#[tokio::test]
async fn max_active_projects_deserializes_for_app_contexts() -> Result<(), Box<dyn Error>> {
    let source = std::env::var("ROTOTO_WORKSPACE_SOURCE")
        .unwrap_or_else(|_| "../account-config".to_owned());
    let workspace = Workspace::load(source).await?;

    let standard = ResolveContext::from_json(serde_json::json!({
        "account": { "plan": "standard" }
    }))?;
    let premium = ResolveContext::from_json(serde_json::json!({
        "account": { "plan": "premium" }
    }))?;

    let standard = workspace
        .resolve_variable("max-active-projects", &standard)
        .await?;
    let premium = workspace
        .resolve_variable("max-active-projects", &premium)
        .await?;

    let standard: i64 = serde_json::from_value(standard.value)?;
    let premium: i64 = serde_json::from_value(premium.value)?;

    assert_eq!(standard, 3);
    assert_eq!(premium, 25);

    Ok(())
}
```

```python
import os
import rototo


async def test_max_active_projects_deserializes_for_app_contexts():
    source = os.environ.get("ROTOTO_WORKSPACE_SOURCE", "../account-config")
    workspace = await rototo.Workspace.load(source)

    standard = await workspace.resolve_variable(
        "max-active-projects",
        {"account": {"plan": "standard"}},
    )
    premium = await workspace.resolve_variable(
        "max-active-projects",
        {"account": {"plan": "premium"}},
    )

    assert standard.value == 3
    assert premium.value == 25
```

```typescript
import assert from "node:assert/strict";
import test from "node:test";
import { Workspace } from "rototo";

test("max-active-projects deserializes for app contexts", async () => {
  const source = process.env.ROTOTO_WORKSPACE_SOURCE ?? "../account-config";
  const workspace = await Workspace.load(source);

  const standard = await workspace.resolveVariable(
    "max-active-projects",
    { account: { plan: "standard" } },
  );
  const premium = await workspace.resolveVariable(
    "max-active-projects",
    { account: { plan: "premium" } },
  );

  assert.equal(standard.value, 3);
  assert.equal(premium.value, 25);
});
```

```java
@Test
void maxActiveProjectsDeserializesForAppContexts() throws Exception {
    String source = System.getenv().getOrDefault(
        "ROTOTO_WORKSPACE_SOURCE",
        "../account-config"
    );

    try (Workspace workspace = Workspace.load(source).get()) {
        VariableResolution standard = workspace
            .resolveVariable(
                "max-active-projects",
                Map.of("account", Map.of("plan", "standard"))
            )
            .get();
        VariableResolution premium = workspace
            .resolveVariable(
                "max-active-projects",
                Map.of("account", Map.of("plan", "premium"))
            )
            .get();

        assertEquals(3L, ((Number) standard.value()).longValue());
        assertEquals(25L, ((Number) premium.value()).longValue());
    }
}
```

```go
func TestMaxActiveProjectsDeserializesForAppContexts(t *testing.T) {
    source := os.Getenv("ROTOTO_WORKSPACE_SOURCE")
    if source == "" {
        source = "../account-config"
    }

    ctx := context.Background()
    workspace, err := rototo.Load(ctx, source, nil)
    if err != nil {
        t.Fatal(err)
    }
    defer workspace.Close()

    standard, err := workspace.ResolveVariable(
        ctx,
        "max-active-projects",
        map[string]any{"account": map[string]any{"plan": "standard"}},
        nil,
    )
    if err != nil {
        t.Fatal(err)
    }
    premium, err := workspace.ResolveVariable(
        ctx,
        "max-active-projects",
        map[string]any{"account": map[string]any{"plan": "premium"}},
        nil,
    )
    if err != nil {
        t.Fatal(err)
    }

    if standard.Value != float64(3) || premium.Value != float64(25) {
        t.Fatalf("unexpected values: %v, %v", standard.Value, premium.Value)
    }
}
```
:::

Run the app tests against the local workspace:

```sh
cd /path/to/account-app
cargo test
```

In CI, set `ROTOTO_WORKSPACE_SOURCE` to the same git source URI the service
uses when the app repository should test against the hosted workspace.

## Release And Observe

Before merging a workspace change, the pull request should say what behavior
can change and how to recover:

```text
Change max-active-projects:
- add premium-account rule
- standard accounts keep value 3
- premium accounts select value 25
- rototo lint and account-app contract tests passed
- rollback: revert this workspace commit
```

After merge, services following the branch source can
[refresh](reference-sdk-refresh.html) to the new workspace. The application
binary does not redeploy, but future resolutions can change.

The service should log the selected source and workspace fingerprint near the
behavior boundary:

:::sdk-snippet production-log-selection
```rust
let resolution = workspace
    .resolve_variable("max-active-projects", &context)
    .await?;

tracing::info!(
    variable = "max-active-projects",
    source = %resolution.source,
    workspace_fingerprint = ?workspace.current().await.source_fingerprint(),
    account_plan = %account_plan,
    "resolved runtime configuration"
);
```

```python
resolution = await workspace.resolve_variable(
    "max-active-projects",
    context,
)
logger.info(
    "resolved runtime configuration",
    extra={
        "variable": "max-active-projects",
        "source": resolution.source,
        "account_plan": account_plan,
    },
)
```

```typescript
const resolution = await workspace.resolveVariable(
  "max-active-projects",
  context,
);
logger.info("resolved runtime configuration", {
  variable: "max-active-projects",
  source: resolution.source,
  accountPlan,
});
```

```java
VariableResolution resolution = workspace
    .resolveVariable("max-active-projects", context)
    .get();
logger.info(
    "resolved runtime configuration variable={} source={} accountPlan={}",
    "max-active-projects",
    resolution.source(),
    accountPlan
);
```

```go
resolution, err := workspace.ResolveVariable(
    ctx,
    "max-active-projects",
    resolveContext,
    nil,
)
if err != nil {
    return err
}
slog.Info(
    "resolved runtime configuration",
    "variable", "max-active-projects",
    "source", resolution.Source,
    "account_plan", accountPlan,
)
```
:::

It should also expose refresh status:

:::sdk-snippet production-refresh-status
```rust
let status = workspace.status().await;
if status.consecutive_failures > 0 {
    tracing::warn!(
        consecutive_failures = status.consecutive_failures,
        last_error = ?status.last_error,
        "workspace refresh is failing; serving last-known-good configuration"
    );
}
```

```python
status = await workspace.status()
if status.consecutive_failures > 0:
    logger.warning(
        "workspace refresh is failing; serving last-known-good configuration",
        extra={
            "consecutive_failures": status.consecutive_failures,
            "last_error": status.last_error,
        },
    )
```

```typescript
const status = await workspace.status();
if (status.consecutiveFailures > 0) {
  logger.warn(
    "workspace refresh is failing; serving last-known-good configuration",
    {
      consecutiveFailures: status.consecutiveFailures,
      lastError: status.lastError,
    },
  );
}
```

```java
RefreshStatus status = workspace.status().get();
if (status.consecutiveFailures() > 0) {
    logger.warn(
        "workspace refresh is failing; serving last-known-good configuration " +
            "consecutiveFailures={} lastError={}",
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
        "workspace refresh is failing; serving last-known-good configuration",
        "consecutive_failures", status.ConsecutiveFailures,
        "last_error", status.LastError,
    )
}
```
:::

If the policy is wrong, revert the workspace commit. If the app sent the wrong
context or cannot consume the selected value, fix the app-workspace contract and
redeploy the app.

## What This Workflow Gives You

The final system has one clear path:

1. The app is deployed with a workspace source URI.
2. The SDK loads and lints that source at startup.
3. The app supplies runtime facts as context.
4. The workspace evaluates named conditions and variables.
5. Tests prove the workspace and app still agree.
6. Refresh lets reviewed workspace commits affect future resolutions.
7. Last-known-good state protects running services from failed refreshes.
8. Logs and refresh status explain what value was selected and from which
   workspace version.

That is the production goal: runtime configuration can move independently from
the application binary, while still going through review, validation, tests,
observability, and git-backed recovery.
