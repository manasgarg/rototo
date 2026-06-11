# Testing Runtime Configuration

After the application can load a workspace and resolve variables, the next
question is whether a workspace change is safe to release. Lint answers part of
that question, but not all of it.

[`rototo lint`](reference-lint-overview.html) can prove that the workspace is
well-formed: schemas parse, qualifiers reference known fields, variable rules
point at known values, and selected values match their declared shape. You
still need that. It does not prove the application still behaves correctly when
those values are selected.

I think about the tests in layers:

- workspace lint protects the control-plane files;
- generated fixtures protect expected resolution behavior;
- app tests protect the contract between selected JSON and application code;
- refresh tests protect the long-running service behavior.

Each layer catches a different failure. Keeping those layers distinct makes the
test suite easier to maintain and easier to trust during review.

## Start With The Release Question

For any runtime configuration change, ask the same question a reviewer will ask
under pressure:

> If this workspace commit reaches a running service, what behavior can change?

That question usually points at one variable, one set of context facts, and one
application boundary. For an account limit policy, the important cases may be:

```text
standard account -> standard limit profile
enterprise account -> enterprise limit profile
test account -> preview limit profile
```

Those cases should be visible in tests. A reviewer should not need to inspect a
chain of qualifiers and values by hand to understand whether the important
runtime paths still hold.

## Keep Lint In The Workspace

Run lint before any behavior test:

```sh
rototo lint account-config
```

Lint is the first gate because it catches invalid control-plane state before
the app even enters the picture. It should stay close to the workspace
repository and run in pre-commit and CI.

Use built-in lint for rototo's own contracts:

- workspace layout;
- qualifier and variable references;
- predicate operators;
- primitive and schema-backed values;
- context schema compatibility;
- custom Lua lint registration.

Use [custom Lua lint](reference-custom-lua-lint.html) for local policy that
belongs with the workspace. For example, if account limits must stay below an
operational ceiling, that rule belongs in the workspace because it constrains
the values reviewers are approving.

Do not use app tests for that kind of file-level policy. App tests are slower,
farther away from the policy author, and usually worse at explaining which
workspace field is wrong.

## Commit Context Fixtures

Runtime configuration tests need real context objects. I prefer to commit them
as small JSON files instead of rebuilding every context inline in test code:

```text
account-app/tests/contexts/
  standard-account.json
  enterprise-account.json
  test-account.json
```

For example:

```json
{
  "account": {
    "id": "acct_enterprise",
    "plan": "enterprise",
    "seats": 120
  },
  "service": {
    "lane": "prod"
  }
}
```

These files are part of the app-workspace
[context](reference-context.html) contract. When the application starts sending
a new context field, the fixture changes. When the workspace starts depending
on a new field, the fixture proves the app knows how to provide it.

They also help in review because the CLI can resolve with the same input:

```sh
rototo resolve account-config \
  --variable account-limit-profile \
  --context @account-app/tests/contexts/enterprise-account.json
```

That command gives the reviewer a direct way to inspect the selected value key
and [resolution trace](reference-resolution-output.html) before reading
application code.

## Generate Resolution Fixtures

Rototo can generate readable TOML
[fixtures](reference-cli-commands.html) for variables and qualifiers:

```sh
rototo fixtures account-config \
  --variable account-limit-profile \
  --qualifier enterprise-account \
  --out account-app/tests/rototo-fixtures
```

The generated files record contexts and expected outcomes. Commit them with the
app tests:

```text
account-app/tests/rototo-fixtures/
  rototo-fixtures.toml
  variable-account-limit-profile.toml
  qualifier-enterprise-account.toml
```

Then assert them from the app test suite:

```rust
use std::error::Error;
use rototo::Workspace;

#[tokio::test]
async fn rototo_resolution_fixtures_still_hold() -> Result<(), Box<dyn Error>> {
    let source = std::env::var("ROTOTO_WORKSPACE_SOURCE")
        .unwrap_or_else(|_| "account-config".to_owned());
    let workspace = Workspace::load(source).await?;

    rototo::testing::assert_fixtures(&workspace, "tests/rototo-fixtures").await?;

    Ok(())
}
```

Generated fixtures are not a replacement for judgment. They are a reviewable
record of expected runtime behavior. When a policy change intentionally changes
selection, regenerate the fixture, review the diff, and make sure the app
tests still explain why the new behavior is acceptable.

Bucket predicates are a good example. A bucket change can look small in TOML
but move a stable account from one value key to another. The fixture diff makes
that visible.

## Test The App Contract

The application should still have tests that deserialize the selected value and
exercise the behavior boundary that uses it.

:::sdk-snippet testing-app-contract
```rust
use std::error::Error;
use serde::Deserialize;
use rototo::{ResolveContext, Workspace};

#[derive(Debug, Deserialize)]
struct AccountLimitProfile {
    max_projects: u64,
    audit_retention_days: u64,
}

#[tokio::test]
async fn enterprise_account_receives_enterprise_limits() -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::load("account-config").await?;
    let context = ResolveContext::from_json(serde_json::json!({
        "account": {
            "id": "acct_enterprise",
            "plan": "enterprise",
            "seats": 120
        }
    }))?;

    let resolution = workspace
        .resolve_variable("account-limit-profile", &context)
        .await?;
    assert_eq!(resolution.value_key, "enterprise");

    let profile: AccountLimitProfile = serde_json::from_value(resolution.value)?;
    assert_eq!(profile.max_projects, 100);
    assert_eq!(profile.audit_retention_days, 365);

    Ok(())
}
```

```python
from dataclasses import dataclass
import rototo


@dataclass
class AccountLimitProfile:
    max_projects: int
    audit_retention_days: int


async def test_enterprise_account_receives_enterprise_limits():
    workspace = await rototo.Workspace.load("account-config")
    resolution = await workspace.resolve_variable(
        "account-limit-profile",
        {
            "account": {
                "id": "acct_enterprise",
                "plan": "enterprise",
                "seats": 120,
            },
        },
    )

    assert resolution.value_key == "enterprise"
    profile = AccountLimitProfile(**resolution.value)
    assert profile.max_projects == 100
    assert profile.audit_retention_days == 365
```

```typescript
import assert from "node:assert/strict";
import test from "node:test";
import { Workspace } from "rototo";

type AccountLimitProfile = {
  max_projects: number;
  audit_retention_days: number;
};

test("enterprise account receives enterprise limits", async () => {
  const workspace = await Workspace.load("account-config");
  const resolution = await workspace.resolveVariable(
    "account-limit-profile",
    {
      account: {
        id: "acct_enterprise",
        plan: "enterprise",
        seats: 120,
      },
    },
  );

  assert.equal(resolution.valueKey, "enterprise");
  const profile = resolution.value as AccountLimitProfile;
  assert.equal(profile.max_projects, 100);
  assert.equal(profile.audit_retention_days, 365);
});
```

```java
@Test
void enterpriseAccountReceivesEnterpriseLimits() throws Exception {
    try (Workspace workspace = Workspace.load("account-config").get()) {
        VariableResolution resolution = workspace
            .resolveVariable(
                "account-limit-profile",
                Map.of("account", Map.of(
                    "id", "acct_enterprise",
                    "plan", "enterprise",
                    "seats", 120
                ))
            )
            .get();

        @SuppressWarnings("unchecked")
        Map<String, Object> profile =
            (Map<String, Object>) resolution.value();

        assertEquals("enterprise", resolution.valueKey());
        assertEquals(100L, ((Number) profile.get("max_projects")).longValue());
        assertEquals(
            365L,
            ((Number) profile.get("audit_retention_days")).longValue()
        );
    }
}
```

```go
func TestEnterpriseAccountReceivesEnterpriseLimits(t *testing.T) {
    ctx := context.Background()
    workspace, err := rototo.Load(ctx, "account-config", nil)
    if err != nil {
        t.Fatal(err)
    }
    defer workspace.Close()

    resolution, err := workspace.ResolveVariable(
        ctx,
        "account-limit-profile",
        map[string]any{
            "account": map[string]any{
                "id":    "acct_enterprise",
                "plan":  "enterprise",
                "seats": 120,
            },
        },
        nil,
    )
    if err != nil {
        t.Fatal(err)
    }

    payload, err := json.Marshal(resolution.Value)
    if err != nil {
        t.Fatal(err)
    }
    var profile struct {
        MaxProjects        uint64 `json:"max_projects"`
        AuditRetentionDays uint64 `json:"audit_retention_days"`
    }
    if err := json.Unmarshal(payload, &profile); err != nil {
        t.Fatal(err)
    }

    if resolution.ValueKey != "enterprise" {
        t.Fatalf("unexpected value key: %s", resolution.ValueKey)
    }
    if profile.MaxProjects != 100 || profile.AuditRetentionDays != 365 {
        t.Fatalf("unexpected profile: %#v", profile)
    }
}
```
:::

This test catches a different class of failure from lint. The workspace may
select a schema-valid value, but the application may no longer be able to
deserialize or use it. That is an app contract failure, and it should fail in
the app test suite before the service observes the workspace change.

The most valuable app tests usually assert three things:

- the selected `value_key`;
- the app type produced from the selected JSON;
- the application behavior that depends on that type.

The third assertion is what keeps the test from becoming a duplicate of rototo
resolution. For example, if the selected profile controls project creation,
the app test should show that the account can or cannot create a project under
that profile.

## Test Defaults And Failure Paths

Every important variable should have tests for its default path. Defaults are
not just fallback syntax. They are production behavior for any request that
does not match a rule.

:::sdk-snippet testing-default-path
```rust
let standard = ResolveContext::from_json(serde_json::json!({
    "account": {
        "id": "acct_standard",
        "plan": "standard"
    }
}))?;

let resolution = workspace
    .resolve_variable("account-limit-profile", &standard)
    .await?;
assert_eq!(resolution.value_key, "standard");
```

```python
standard = {
    "account": {
        "id": "acct_standard",
        "plan": "standard",
    },
}

resolution = await workspace.resolve_variable(
    "account-limit-profile",
    standard,
)
assert resolution.value_key == "standard"
```

```typescript
const standard = {
  account: {
    id: "acct_standard",
    plan: "standard",
  },
};

const resolution = await workspace.resolveVariable(
  "account-limit-profile",
  standard,
);
assert.equal(resolution.valueKey, "standard");
```

```java
Map<String, Object> standard = Map.of(
    "account",
    Map.of("id", "acct_standard", "plan", "standard")
);

VariableResolution resolution = workspace
    .resolveVariable("account-limit-profile", standard)
    .get();
assertEquals("standard", resolution.valueKey());
```

```go
standard := map[string]any{
    "account": map[string]any{
        "id":   "acct_standard",
        "plan": "standard",
    },
}

resolution, err := workspace.ResolveVariable(
    ctx,
    "account-limit-profile",
    standard,
    nil,
)
if err != nil {
    t.Fatal(err)
}
if resolution.ValueKey != "standard" {
    t.Fatalf("unexpected value key: %s", resolution.ValueKey)
}
```
:::

Also test the failures you expect the app to handle deliberately:

- missing required context;
- selected JSON that cannot become the app type;
- unknown variable ids at the integration boundary;
- app-side degradation when runtime policy cannot be resolved safely.

Do not quietly replace a failed rototo resolution with hardcoded values in the
test. That teaches the app the wrong recovery path. If the service can degrade,
make the degraded behavior explicit and observable.

## Test The Final Layered Source

If the application loads a layered workspace, test the same source URI the
service will use:

```text
ROTOTO_WORKSPACE_SOURCE=git+https://github.com/acme/customer-config.git#main:customers/acme-support
```

Testing only the base workspace can miss the failure that matters in
production: the product default is valid, but the customer or team layer
overrides a value in a way the app cannot consume.

The app should have at least one test path that loads the final assembled
workspace source, resolves the variables it depends on, and deserializes the
selected values into the app types.

That keeps workspace layering in its proper role. Layers are administrative
boundaries. The application still consumes one runtime control plane.

## Test Refresh Behavior

[Refresh](reference-sdk-refresh.html) is part of rototo's runtime model, so
long-running services need tests for refresh behavior too.

At minimum, cover these cases:

- initial load succeeds before the service starts serving requests;
- a successful refresh affects future resolutions;
- a failed refresh keeps the last successfully loaded workspace active;
- refresh status is exposed to logs, health checks, or metrics.

The core assertion looks like this:

:::sdk-snippet testing-refresh-success
```rust
let workspace = RefreshingWorkspace::load(source, RefreshOptions::new()).await?;
let context = ResolveContext::from_json(serde_json::json!({}))?;

let before = workspace
    .resolve_variable("support-banner", &context)
    .await?;
assert_eq!(before.value_key, "off");

publish_workspace_change_that_turns_banner_on().await?;
workspace.refresh_now().await?;

let after = workspace
    .resolve_variable("support-banner", &context)
    .await?;
assert_eq!(after.value_key, "on");
```

```python
workspace = await rototo.RefreshingWorkspace.load(source)
context = {}

before = await workspace.resolve_variable("support-banner", context)
assert before.value_key == "off"

await publish_workspace_change_that_turns_banner_on()
await workspace.refresh_now()

after = await workspace.resolve_variable("support-banner", context)
assert after.value_key == "on"
```

```typescript
const workspace = await RefreshingWorkspace.load(source);
const context = {};

const before = await workspace.resolveVariable("support-banner", context);
assert.equal(before.valueKey, "off");

await publishWorkspaceChangeThatTurnsBannerOn();
await workspace.refreshNow();

const after = await workspace.resolveVariable("support-banner", context);
assert.equal(after.valueKey, "on");
```

```java
RefreshingWorkspace workspace = RefreshingWorkspace
    .load(source)
    .get();
Map<String, Object> context = Map.of();

VariableResolution before = workspace
    .resolveVariable("support-banner", context)
    .get();
assertEquals("off", before.valueKey());

publishWorkspaceChangeThatTurnsBannerOn();
workspace.refreshNow().get();

VariableResolution after = workspace
    .resolveVariable("support-banner", context)
    .get();
assertEquals("on", after.valueKey());
```

```go
workspace, err := rototo.LoadRefreshing(ctx, source, nil)
if err != nil {
    t.Fatal(err)
}
defer workspace.Close(ctx)

before, err := workspace.ResolveVariable(
    ctx,
    "support-banner",
    map[string]any{},
    nil,
)
if err != nil {
    t.Fatal(err)
}
if before.ValueKey != "off" {
    t.Fatalf("unexpected value key before refresh: %s", before.ValueKey)
}

publishWorkspaceChangeThatTurnsBannerOn(t)
if _, err := workspace.RefreshNow(ctx); err != nil {
    t.Fatal(err)
}

after, err := workspace.ResolveVariable(
    ctx,
    "support-banner",
    map[string]any{},
    nil,
)
if err != nil {
    t.Fatal(err)
}
if after.ValueKey != "on" {
    t.Fatalf("unexpected value key after refresh: %s", after.ValueKey)
}
```
:::

And the failure path should prove last-known-good behavior:

:::sdk-snippet testing-refresh-failure
```rust
publish_broken_workspace_change().await?;
assert!(workspace.refresh_now().await.is_err());

let still_valid = workspace
    .resolve_variable("support-banner", &context)
    .await?;
assert_eq!(still_valid.value_key, "on");

let status = workspace.status().await;
assert!(status.last_error.is_some());
assert_eq!(status.consecutive_failures, 1);
```

```python
await publish_broken_workspace_change()
try:
    await workspace.refresh_now()
except rototo.RototoError:
    pass

still_valid = await workspace.resolve_variable("support-banner", context)
assert still_valid.value_key == "on"

status = await workspace.status()
assert status.last_error is not None
assert status.consecutive_failures == 1
```

```typescript
await publishBrokenWorkspaceChange();
await assert.rejects(() => workspace.refreshNow());

const stillValid = await workspace.resolveVariable("support-banner", context);
assert.equal(stillValid.valueKey, "on");

const status = await workspace.status();
assert.ok(status.lastError);
assert.equal(status.consecutiveFailures, 1);
```

```java
publishBrokenWorkspaceChange();
assertThrows(ExecutionException.class, () -> workspace.refreshNow().get());

VariableResolution stillValid = workspace
    .resolveVariable("support-banner", context)
    .get();
assertEquals("on", stillValid.valueKey());

RefreshStatus status = workspace.status().get();
assertNotNull(status.lastError());
assertEquals(1, status.consecutiveFailures());
```

```go
publishBrokenWorkspaceChange(t)
if _, err := workspace.RefreshNow(ctx); err == nil {
    t.Fatal("expected refresh failure")
}

stillValid, err := workspace.ResolveVariable(
    ctx,
    "support-banner",
    map[string]any{},
    nil,
)
if err != nil {
    t.Fatal(err)
}
if stillValid.ValueKey != "on" {
    t.Fatalf("unexpected value key after failed refresh: %s", stillValid.ValueKey)
}

status, err := workspace.Status(ctx)
if err != nil {
    t.Fatal(err)
}
if status.LastError == nil || status.ConsecutiveFailures != 1 {
    t.Fatalf("unexpected refresh status: %#v", status)
}
```
:::

The helper names in those snippets are app test helpers, not rototo APIs. The
contract is what matters: a bad workspace commit must not replace the
last-known-good workspace in a running service.

## Put The Gates In Order

I would wire CI in this order:

```sh
rototo lint account-config
cargo test -p account-app rototo_resolution_fixtures_still_hold
cargo test -p account-app
```

The exact commands will vary by repository layout, but the order matters. First
prove the workspace is valid. Then prove expected resolution behavior. Then
prove the application can consume and apply the selected policy.

That order is why the [production workflow](production-workflow.html) holds
together. The workspace can move independently from the application binary, but
it still moves through a release path that proves the app and control plane
agree.
