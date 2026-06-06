# Testing Runtime Configuration

After the application can load a workspace and resolve variables, the next
question is whether a workspace change is safe to release. Lint answers part of
that question, but not all of it.

`rototo lint` can prove that the workspace is well-formed: schemas parse,
qualifiers reference known fields, variable rules point at known values, and
selected values match their declared shape. That is necessary. It is not the
same as proving the application still behaves correctly when those values are
selected.

The useful testing model is layered:

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

Use custom Lua lint for local policy that belongs with the workspace. For
example, if account limits must stay below an operational ceiling, that rule
belongs in the workspace because it constrains the values reviewers are
approving.

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

These files are part of the app-workspace contract. When the application starts
sending a new context field, the fixture changes. When the workspace starts
depending on a new field, the fixture proves the app knows how to provide it.

They are also useful in review because the CLI can resolve with the same input:

```sh
rototo resolve account-config \
  --variable account-limit-profile \
  --context @account-app/tests/contexts/enterprise-account.json
```

That command gives the reviewer a direct way to inspect the selected value key
and resolution trace before reading application code.

## Generate Resolution Fixtures

Rototo can generate readable TOML fixtures for variables and qualifiers:

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

This is especially useful for bucket predicates. A bucket change can look small
in TOML but move a stable account from one value key to another. The fixture
diff makes that visible.

## Test The App Contract

The application should still have tests that deserialize the selected value and
exercise the behavior boundary that uses it.

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

Refresh is part of rototo's runtime model, so long-running services need tests
for refresh behavior too.

At minimum, cover these cases:

- initial load succeeds before the service starts serving requests;
- a successful refresh affects future resolutions;
- a failed refresh keeps the last successfully loaded workspace active;
- refresh status is exposed to logs, health checks, or metrics.

The core assertion looks like this:

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

And the failure path should prove last-known-good behavior:

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

The helper names in those snippets are app test helpers, not rototo APIs. The
important part is the contract: a bad workspace commit must not replace the
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

That sequence is what makes the production workflow credible. The workspace can
move independently from the application binary, but it still moves through a
release path that proves the app and control plane agree.
