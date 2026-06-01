# Production Workflow

This tutorial configures a SaaS product's incident summary agent. Enterprise
accounts in production should receive a larger LLM configuration. Other
accounts should receive the standard configuration.

rototo models this use case with four core pieces:

- The workspace is the Git-versioned configuration boundary. It contains the
  environments, schemas, qualifiers, variables, and tests.
- Runtime context is the JSON facts the application sends with a config request.
  In this example, the context says the account plan is `enterprise` and the
  account has `250` seats.
- A qualifier turns runtime context into a named condition. The
  `enterprise-accounts` qualifier means "account plan is enterprise and seats
  are at least 100."
- A variable is the config value the application asks for. The
  `llm-agent-config` variable returns the standard LLM config by default, and
  returns the enterprise LLM config when `enterprise-accounts` matches.

In this tutorial, you will:

1. Create a Git-backed workspace with schemas for input context and output
   config.
2. Model the rollout decision with a qualifier and a variable.
3. Validate the workspace and automate the same checks in tests and hooks.
4. Load the workspace from an application by Git URI and verify the integration.
5. Define the evaluation record shape needed to observe production decisions.

## Create a workspace repository

Create a separate repository for runtime configuration:

```sh
mkdir runtime-config
cd runtime-config
git init
mkdir -p config/qualifiers config/variables config/resources config/schemas config/tests
```

The workspace lives under `config/`:

```text
runtime-config/
  config/
    rototo-workspace.toml
    qualifiers/
      enterprise-accounts.toml
    variables/
      llm-agent-config.toml
    resources/
      llm-agent-config.toml
      llm-agent-config-objects/
        local.toml
        standard.toml
        enterprise.toml
    schemas/
      context.schema.json
      llm-config.schema.json
    tests/
      prod-enterprise.json
      prod-enterprise.expected.json
```

Keeping the workspace in its own repository lets configuration follow a
GitOps-style lifecycle: review changes, run automated checks, publish from Git,
and let applications load the exact reviewed source.

At this point you have an empty workspace for runtime configuration. The next
two steps define the contract for that workspace: which environments it
supports, and which runtime facts an application must provide when it asks for
config.

## Add the workspace manifest

Create `config/rototo-workspace.toml`:

```toml
schema_version = 1

[environments]
values = ["dev", "stage", "prod"]

[context]
schema = "schemas/context.schema.json"
```

The manifest declares the environments and context schema. rototo discovers
qualifiers, variables, resources, schemas, and Lua lint files from the
conventional workspace directories.

The context schema is the input contract between the application and the
workspace. It defines the JSON attributes the application promises to send at
resolution time, so config authors can write rules against known fields instead
of guessing.

## Define runtime context

Create `config/schemas/context.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["account"],
  "additionalProperties": false,
  "properties": {
    "account": {
      "type": "object",
      "required": ["plan", "seats"],
      "additionalProperties": true,
      "properties": {
        "plan": { "type": "string" },
        "seats": { "type": "integer" }
      }
    }
  }
}
```

If the application later stops sending `account.seats`, or changes it from an
integer to a string, context validation catches the mismatch before rules are
evaluated. Predicate evaluation also fails if a qualifier reads a context path
that is missing, so the workspace does not choose the default LLM config when it
no longer receives the facts its qualifier depends on.

Now the workspace knows what information it can trust from the application. The
next step turns a business condition in that context into a reusable name.

## Define a qualifier

The product decision is not really about a raw JSON path. It is about an account
class: enterprise accounts with enough seats should receive a larger LLM config.
A qualifier gives that condition a stable name so variables, tests, diagnostics,
and future config can refer to `enterprise-accounts` instead of repeating the
same predicate everywhere.

Create `config/qualifiers/enterprise-accounts.toml`:

```toml
schema_version = 1

description = "Accounts on the enterprise plan with at least 100 seats"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"

[[predicate]]
attribute = "account.seats"
op = "gte"
value = 100
```

`enterprise-accounts` is the qualifier id because the file is named
`enterprise-accounts.toml`. It resolves to `true` only when both predicates
match the runtime context.

The workspace can now recognize the target account class. The next step defines
what the application actually asks for: the LLM configuration value.

## Define a variable

A variable is the application-facing contract. Application code should not know
which qualifiers exist or which rollout rules are active; it should ask for a
named value, `llm-agent-config`, in an environment with runtime context. rototo
then returns the selected value and the key that produced it.

Before creating the variable file, define the shape of the value it is allowed
to return. This keeps the workspace honest: every configured LLM value must have
the fields the application expects.

Create `config/schemas/llm-config.schema.json`:

```json
{
  "type": "object",
  "required": ["model", "gateway", "prompt", "max_output_tokens", "temperature"],
  "properties": {
    "model": { "type": "string" },
    "gateway": { "type": "string" },
    "prompt": { "type": "string" },
    "max_output_tokens": { "type": "integer", "minimum": 1, "maximum": 5000 },
    "temperature": { "type": "number", "minimum": 0, "maximum": 2 }
  },
  "additionalProperties": false
}
```

This schema is the output contract between the workspace and the application. It
validates every configured LLM value before the workspace is used.

The LLM config values are structured and have long prompt text, so model them
as a resource. The variable file owns the application-facing id and selection
rules:

Create `config/variables/llm-agent-config.toml`:

```toml
schema_version = 1

description = "LLM settings for the incident summary agent"
type = "resource:llm-agent-config"

[env._]
value = "standard"

[env.dev]
value = "local"

[env.prod]
value = "standard"

[[env.prod.rule]]
description = "Enterprise accounts get the larger agent configuration"
qualifier = "enterprise-accounts"
value = "enterprise"
```

Create `config/resources/llm-agent-config.toml`:

```toml
schema_version = 1
schema = "../schemas/llm-config.schema.json"
```

Create `config/resources/llm-agent-config-objects/local.toml`:

```toml
model = "local-small"
gateway = "ollama"
prompt = "Summarize the incident briefly."
max_output_tokens = 1200
temperature = 0.2
```

Create `config/resources/llm-agent-config-objects/standard.toml`:

```toml
model = "gpt-5-mini"
gateway = "openai"
prompt = "Summarize the incident, customer impact, and next steps."
max_output_tokens = 2400
temperature = 0.3
```

Create `config/resources/llm-agent-config-objects/enterprise.toml`:

```toml
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow. Preserve customer impact, operational risk, and next actions."
max_output_tokens = 5000
temperature = 0.2
```

`llm-agent-config` is the variable id because the file is named
`llm-agent-config.toml`. The `_` environment is the fallback. `dev` uses
`local`. `prod` uses `standard`, except when `enterprise-accounts` matches.
The value keys come from the files in `llm-agent-config-objects/`.

Built-in validation checks the rototo model and validates each resource object
against the JSON Schema. The schema captures both application shape and the
local token ceiling for this resource.

The workspace now has the whole decision model: context describes the account,
the qualifier recognizes the enterprise account condition, and the variable
returns the right LLM config for the environment.

## Add a representative test context

Before publishing this repo, capture the behavior that must not regress. The
test fixture below represents the important production case from the use case:
an enterprise account with 250 seats should resolve to the enterprise value.

Create `config/tests/prod-enterprise.json`:

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 250
  }
}
```

Create `config/tests/prod-enterprise.expected.json`:

```json
{
  "variables": [
    {
      "id": "llm-agent-config",
      "environment": "prod",
      "value_key": "enterprise",
      "value": {
        "gateway": "openai",
        "max_output_tokens": 5000,
        "model": "gpt-5",
        "prompt": "Summarize the incident for an enterprise support workflow. Preserve customer impact, operational risk, and next actions.",
        "temperature": 0.2
      }
    }
  ],
  "qualifiers": []
}
```

This test fixture documents the expected production behavior for an enterprise
account.

## Validate locally

With the model and fixture in place, validate the workspace before thinking
about Git, CI, or application code.

From the repository root:

```sh
rototo inspect ./config
rototo lint ./config
```

Resolve the qualifier:

```sh
rototo resolve ./config --qualifier enterprise-accounts \
  --context @config/tests/prod-enterprise.json
```

Expected output:

```text
enterprise-accounts=true
```

Resolve the variable:

```sh
rototo resolve ./config --variable llm-agent-config \
  --env prod \
  --context @config/tests/prod-enterprise.json
```

Expected output:

```text
llm-agent-config={"gateway":"openai","max_output_tokens":5000,"model":"gpt-5","prompt":"Summarize the incident for an enterprise support workflow. Preserve customer impact, operational risk, and next actions.","temperature":0.2} (enterprise)
```

## Automate workspace tests

For CI, use JSON output and compare it with the committed expected output:

```sh
rototo lint ./config

rototo resolve ./config --variable llm-agent-config \
  --env prod \
  --context @config/tests/prod-enterprise.json \
  --json > /tmp/llm-agent-config.actual.json

jq 'del(.workspace)' /tmp/llm-agent-config.actual.json > /tmp/llm-agent-config.resolution.json
diff -u config/tests/prod-enterprise.expected.json /tmp/llm-agent-config.resolution.json
```

That test proves the workspace still validates, that custom lint passes, and
that the enterprise case continues to select the expected value.

## Add a pre-push check

CI is the release gate, but a local hook catches mistakes before they leave the
developer machine. Create `.git/hooks/pre-push`:

```sh
#!/usr/bin/env sh
set -eu

rototo lint ./config
rototo resolve ./config --variable llm-agent-config \
  --env prod \
  --context @config/tests/prod-enterprise.json \
  --json > /tmp/llm-agent-config.actual.json
jq 'del(.workspace)' /tmp/llm-agent-config.actual.json > /tmp/llm-agent-config.resolution.json
diff -u config/tests/prod-enterprise.expected.json /tmp/llm-agent-config.resolution.json
```

Make it executable:

```sh
chmod +x .git/hooks/pre-push
```

At this point the workspace can be reviewed like source code: it has a local
contract, a representative test, and an automated guard before changes are
pushed.

## Publish the workspace from Git

Commit and push the workspace repository:

```sh
git add config
git commit -m "Add LLM agent runtime config"
git remote add origin https://github.com/acme/runtime-config.git
git push -u origin main
git branch prod
git push origin prod
```

Applications and automation can now load the workspace by Git URI:

```text
git+https://github.com/acme/runtime-config.git#prod:config
```

The fragment means: use ref `prod` and workspace subdirectory `config`.
rototo does not require this convention; `main` can be the production source for
a small team. A separate `prod` or `release/prod` ref is useful when you want an
explicit promotion step after workspace checks pass.

## Resolve from the Git workspace source

After the workspace is pushed, consumers should load the same reviewed source
instead of copying files into each application repo. The CLI can resolve
directly from the Git source, which is useful for release verification and
debugging.

```sh
rototo resolve 'git+https://github.com/acme/runtime-config.git#prod:config' --variable llm-agent-config \
  --env prod \
  --context @config/tests/prod-enterprise.json
```

For private HTTPS archive sources, use `ROTOTO_WORKSPACE_TOKEN` or
`--workspace-token`. For private Git repositories, use the authentication
available to `git` on the host running rototo.

## Load from an application

The application uses the same URI. That keeps local validation, CI, debugging,
and production code pointed at one workspace source.

For tests, jobs, and short-lived tools, load the workspace once:

```rust
use rototo::{Environment, ResolveContext, Workspace};

let workspace = Workspace::load(
    "git+https://github.com/acme/runtime-config.git#prod:config"
).await?;

let env = Environment::new("prod");
let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "plan": "enterprise",
        "seats": 250
    }
}))?;

let config = workspace
    .resolve_variable("llm-agent-config", &env, &context)
    .await?;
```

Long-running services usually need a different shape. Deploy the application
with the Git workspace URI as runtime configuration. The application loads that
workspace on startup, then refreshes it in the background so reviewed config
changes can take effect without rebuilding or redeploying the application.

Use `RefreshingWorkspace` for that model:

```rust
use std::time::Duration;

use rototo::{Environment, RefreshOptions, RefreshingWorkspace, ResolveContext};

let workspace = RefreshingWorkspace::load(
    "git+https://github.com/acme/runtime-config.git#prod:config",
    RefreshOptions::new().with_period(Duration::from_secs(60)),
)
.await?;

let env = Environment::new("prod");
let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "plan": "enterprise",
        "seats": 250
    }
}))?;

let config = workspace
    .resolve_variable("llm-agent-config", &env, &context)
    .await?;
```

Each refresh checks the same source. A successful refresh replaces the active
workspace for future resolutions. If a refresh fails because Git is temporarily
unavailable, authentication breaks, or the new workspace is invalid, the
application continues resolving from the last successfully loaded workspace.

Use a mutable ref, such as a controlled production branch, when you want
periodic refresh. Use an immutable commit ref for reproducible tests or pinned
deployments; immutable commit refs load normally, but there is nothing new to
refresh.

## Test application integration

Workspace tests prove the configuration resolves as intended. Application tests
prove the service loads the workspace and uses the returned value correctly.

A Rust application test can load the same Git workspace and assert the selected
LLM config:

```rust
use rototo::{Environment, ResolveContext, Workspace};

#[tokio::test]
async fn enterprise_accounts_use_enterprise_llm_config() {
    let workspace = Workspace::load(
        "git+https://github.com/acme/runtime-config.git#prod:config"
    )
    .await
    .unwrap();

    let env = Environment::new("prod");
    let context = ResolveContext::from_json(serde_json::json!({
        "account": {
            "plan": "enterprise",
            "seats": 250
        }
    }))
    .unwrap();

    let config = workspace
        .resolve_variable("llm-agent-config", &env, &context)
        .await
        .unwrap();

    assert_eq!(config.value_key, "enterprise");
    assert_eq!(config.value["model"], "gpt-5");
    assert_eq!(config.value["max_output_tokens"], 5000);
}
```

In production services, prefer a pinned ref or a controlled branch policy for
tests so application CI does not depend on unrelated workspace changes.

## Observe runtime resolution

The system now makes production decisions outside application code, so operators
need visibility into those decisions. Observability should answer not just which
value was returned, but why it was returned and which workspace version made the
decision. A rototo evaluation record should capture what was evaluated, the
runtime context, the matched rules, the returned value, and the current request
trace.

A record for the enterprise LLM config resolution can look like this:

```json
{
  "schema_version": 1,
  "timestamp": "2026-05-29T12:00:00Z",
  "workspace": {
    "source": "git+https://github.com/acme/runtime-config.git#prod:config",
    "fingerprint": "git:abc123",
    "environment": "prod"
  },
  "subject": {
    "kind": "variable",
    "id": "llm-agent-config"
  },
  "result": {
    "value_key": "enterprise",
    "value_type": "object",
    "reason": "matched_rule"
  },
  "evaluation": {
    "matched_rules": ["prod.enterprise-accounts"],
    "matched_qualifiers": ["enterprise-accounts"],
    "duration_ms": 2
  },
  "context": {
    "attributes": {
      "account.plan": "enterprise",
      "account.seats": 250
    },
    "redacted": []
  },
  "sdk": {
    "name": "rototo-rust",
    "version": "0.1.0-alpha.1"
  },
  "trace": {
    "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
    "span_id": "00f067aa0ba902b7"
  }
}
```

The intended SDK observability surface should be able to deliver this record to
sinks. During development, a stdout or file sink is enough. In production, an
OTLP sink can emit the record as a span event or log record so operators can
answer questions such as:

- Which config value did this request receive?
- Which workspace version produced it?
- Which qualifier or rule selected it?
- Did a release change evaluation distribution?
- Are unexpected contexts reaching production?

## What you built

This tutorial covered the full rototo loop:

```text
workspace repo
  -> define qualifier and variable
  -> store structured values in resources
  -> validate resource objects with JSON Schema
  -> validate locally
  -> test in CI
  -> publish through Git
  -> load via git+https URI
  -> test application integration
  -> resolve in application code
  -> emit an evaluation record
```

Next, read the reference pages for exact CLI, SDK, and diagnostic behavior.
