# Production Workflow

The local loop from getting started is useful, but it is not enough for
production. Production configuration needs names for runtime conditions, a
contract for the context the app sends, policy checks for values, and tests that
fail when the app and workspace drift apart.

The useful part is that none of this changes the core shape. We keep the same
`account-config` workspace and the same `account-app`; we just add the pieces I
would want before trusting this path in a service.

## Add A Runtime Condition

The next thing I would add is a named condition. Premium accounts should be
able to keep more active projects, but I do not want that rule hidden in app
code. The app should supply facts about the request, and the workspace should
own the policy for turning those facts into a value.

That starts with a qualifier: a named predicate over the context the app will
send at runtime.

Create `account-config/qualifiers/premium-account.toml`:

```toml
schema_version = 1
description = "Requests from premium accounts"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "premium"
```

That name is important. Once the condition is named, variables can refer to
`premium-account` instead of copying the `account.plan` predicate into
every place that needs it.

Update `account-config/variables/max-active-projects.toml`:

```toml
schema_version = 1

description = "Maximum active projects for an account"
type = "int"

[values]
standard = 3
premium = 25

[resolve]
default = "standard"

[[resolve.rule]]
qualifier = "premium-account"
value = "premium"
```

Rules are evaluated in order. The first matching qualifier selects its value;
otherwise the default value is used.

Now the workspace has a predicate that reads `account.plan`. This is the
right time to add the context schema. I like generating the first skeleton from
the workspace because it avoids a copy-paste trap: the qualifier can read one
path while a hand-written schema validates another.

Generate the context schema:

```sh
rototo init account-config --context
```

On this workspace, that writes `account-config/schemas/context.schema.json` from
the context paths referenced by qualifiers:

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

The generated schema is a starting contract, not a file to ignore. Review it
with the same care as the qualifier, because it defines the request facts the
app must provide when it asks rototo to resolve a value.

Lint the workspace:

```sh
rototo lint account-config
```

Then resolve both paths so the behavior is visible before the app relies on it.

Non-premium requests use the default:

```sh
rototo resolve account-config \
  --variable max-active-projects \
  --context account.plan=standard
```

Premium requests select the larger value:

```sh
rototo resolve account-config \
  --variable max-active-projects \
  --context account.plan=premium
```

At this point the app also needs to provide that context when it resolves the
variable. In `account-app`, replace the empty context with request or account
facts from your application boundary. For this guide, an environment variable
is enough:

```rust
let account_plan =
    std::env::var("ACCOUNT_PLAN").unwrap_or_else(|_| "standard".to_owned());
let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "plan": account_plan
    }
}))?;
```

Run the app as a premium account:

```sh
ACCOUNT_PLAN=premium cargo run -- ../account-config
```

Now the app owns the facts it knows at runtime, and the workspace owns the
decision about which configured value those facts select.

## Publish The Workspace

A local directory is a good place to learn the loop. A production service needs
a workspace source it can fetch. Since git is the source of truth, publish
`account-config` as its own repository and pass the app a git workspace URI.

The following commands create a private GitHub repository using `gh`. They use
SSH for runtime access, so the production environment needs an SSH key or deploy
key that can read the repository.

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

The `#main` ref means refreshes follow the current `main` branch. Pinning the
source to a full 40-character commit SHA is useful when you need exact
reproducibility, but it is immutable: the SDK will not discover newer commits
from that source.

## Add Workspace Lint

Built-in lint proves the workspace is structurally valid. Local policy is where
the team's judgment enters. For this variable, the policy is straightforward:
account project limits should be positive, stay under an agreed ceiling, and
keep the standard plan from accidentally getting more than premium.

Lua lint is useful here because the rule belongs with the workspace. A reviewer
can see the value change and the policy that guards it in the same repository.

Create `account-config/lint/max-active-projects.lua`:

```lua
function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = {
      id = "operations/max-active-projects-policy",
      title = "Account project limit violates operations policy",
      help = "Keep max-active-projects values between 1 and 100 and keep standard <= premium.",
    },
    handler = "check_max_active_projects",
  })
end

function check_max_active_projects(ctx)
  if ctx.target.id ~= "max-active-projects" then
    return {}
  end

  local values = ctx.target.toml.values or {}
  local diagnostics = {}

  for name, value in pairs(values) do
    if type(value) ~= "number" or value < 1 or value > 100 then
      table.insert(diagnostics, {
        message = "max-active-projects." .. name .. " must be between 1 and 100"
      })
    end
  end

  if type(values.standard) == "number"
      and type(values.premium) == "number"
      and values.standard > values.premium then
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

Custom rules use their own authority. Here the rule is
`operations/max-active-projects-policy`; built-in rototo rules stay under the
`rototo/` authority.

## Protect Changes Before Merge

The workspace repository should reject bad edits before they reach `main`.
Pre-commit gives fast local feedback, while CI protects the shared branch.

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

Now a configuration change follows the same shape as a code change: edit the
workspace, run lint locally, open a PR, run CI, review the diff, and merge.

## Test The App Contract

Workspace lint protects the workspace. App tests protect the contract between
the workspace and the code that consumes it.

I like generating fixtures at this point because they turn runtime behavior
into reviewable files. The app test does not need to know how every qualifier
works; it asserts that the contexts the app depends on still select values the
app can deserialize and use.

Generate rototo fixtures from the workspace:

```sh
rototo fixtures account-config \
  --variable max-active-projects \
  --qualifier premium-account \
  --out account-app/tests/rototo-fixtures
```

Commit the generated `tests/rototo-fixtures` directory in the app repo. The
fixtures are readable TOML cases, so review can see which contexts and selected
values the app is asserting.

Add an app test, for example `account-app/tests/rototo_contract.rs`:

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

Run the app tests against the local workspace:

```sh
cd /path/to/account-app
cargo test
```

In CI, set `ROTOTO_WORKSPACE_SOURCE` to the same git source URI the service uses
when the app repository should test against the hosted workspace.

## What This Workflow Gives You

The app is still deployed with a workspace source URI, not embedded
configuration values. At startup, the SDK loads and lints that source. During
runtime, the app supplies request context and resolves named variables.

For long-running services, successful refreshes affect future resolutions.
Failed refreshes keep the last successfully loaded workspace active. That is
the operational shape I want: reviewed configuration can move without an app
redeploy, and a bad refresh does not take away the last known-good workspace.

The control plane remains reviewable git state: workspace files, custom lint,
fixtures, and tests all move through the release process before the app observes
new configuration.
