# rototo

Runtime configuration changes production behavior. rototo helps you treat it
with the same rigor as code while keeping it deployable separately from the
application binary.

Most software has two familiar lifecycles.

Code is reviewed, tested, versioned, released, observed, and rolled back when it
misbehaves. Data is created and changed through the application's runtime data
model.

Runtime configuration looks like data because it is made of values: token
limits, model names, queue names, support messages, rollout buckets, checkout
settings, tenant limits, and operational overrides. But those values often act
like code. They decide which behavior runs in production.

That is the boundary rototo is built for.

## The Problem

A runtime configuration change can alter what users see, which backend a request
uses, how much budget an operation consumes, which account receives a different
path, or whether an incident override is active.

Those changes should not be invisible operational data. They need the controls
teams already rely on for production code:

- reviewable diffs
- validation before release
- tests for representative cases
- clear ownership
- version history
- rollback
- diagnostics when a change is invalid
- observability when a request receives a surprising value

At the same time, configuration should not require an application rebuild every
time a reviewed value changes. The application binary can stay fixed while the
configuration workspace moves through its own release process.

## What rototo Is

rototo is a Git-backed runtime configuration control plane.

A rototo workspace is a directory of configuration files in Git. It declares
environments, qualifiers, variables, resources, schemas, values, and optional
custom lint policy. The CLI and SDK load the same workspace, so local debugging,
CI checks, agents, and running services use the same source of truth.

Applications do not read arbitrary files or loose keys. They ask rototo for a
named variable in an environment with runtime context:

```text
workspace version + variable id + environment + runtime context
  -> validate context
  -> evaluate qualifiers
  -> select value key
  -> validate selected value
  -> return value and selection metadata
```

The result is runtime configuration that can change independently from the
application binary without becoming detached from the software delivery
process.

## The Lifecycle

The workspace is the control-plane boundary. It can live in the same repository
as an application during early development, but the production shape is usually
a separate Git-backed configuration source.

```text
workspace files
    |
    v
Git review
    |
    v
automated checks: inspect, lint, resolve representative cases
    |
    v
release workspace from local path, Git source, or archive
    |
    v
application loads workspace through SDK
    |
    v
runtime resolution from environment + context
    |
    v
diagnostics + application telemetry
```

Long-running services can refresh from the same workspace source. A successful
refresh affects future resolutions. A failed refresh keeps the last
successfully loaded workspace active. Immutable commit refs are reproducible,
but they do not produce new refresh results because the source does not move.

## What rototo Controls

rototo is for runtime decisions that are too important to scatter across
constants, deployment variables, dashboards, database rows, and runbooks:

- Use one token budget in `dev`, another in `stage`, and a stricter one in
  `prod`.
- Route enterprise accounts to a different fraud review queue when cart value
  is high.
- Select an LLM agent configuration by environment, account class, and rollout
  bucket.
- Show a support banner only for requests affected by an incident.
- Validate that a structured recommendation config still matches the schema the
  application expects.
- Route premium accounts in Germany to a priority payment review queue while
  keeping the standard queue for everyone else.

The point is not to move every value out of application code. The point is to
give production behavior changes a reviewed, typed, testable place to live when
they need to move independently from application deployments.

## The Model

rototo uses a small set of concepts.

A workspace is the versioned source of truth. It contains the manifest and the
files that define runtime behavior.

An environment is the deployment or runtime lane, such as `dev`, `stage`, or
`prod`. The same variable can resolve differently in each environment while the
application-facing name stays stable.

Runtime context is the JSON object the application supplies for a request,
tenant, account, user, or operation. Context keeps request-time facts visible
instead of hiding them in unrelated systems.

A qualifier is a named condition over runtime context, such as
`enterprise-accounts` or `eu-users`. Variables can use qualifiers to select
different values.

A variable is what application code asks for. It declares the logic for
selecting one value in an environment. Small primitive values can live inline in
the variable file.

A resource is a structured value catalog that variables can select from. A
resource-backed variable points at a resource, and its value keys come from
resource object files. This keeps selection logic in `variables/` while larger
JSON-shaped values live under `resources/` with their own schema contract.

Schemas validate the contract between the workspace and the application:
context schemas validate the facts supplied at resolution time, and value
schemas validate resource objects before application code consumes them.

Diagnostics explain invalid workspace files with stable rule ids. Built-in
lint catches malformed manifests, unknown environments, missing values, invalid
qualifier references, type mismatches, schema failures, and other release-time
problems. Custom lint can add workspace-specific policy.

## A Concrete Example

Suppose a SaaS product has an LLM agent that summarizes operational incidents.
The application should not hardcode every model, prompt, token limit, and
gateway setting. Those choices change as budget, product behavior, and account
contracts change.

The team wants this behavior:

- In `dev`, engineers use a local model so development does not spend
  production model budget.
- In `prod`, most accounts use the standard hosted model and the normal
  incident-summary prompt.
- Enterprise accounts use the larger model, a more specific prompt, and a
  higher output token limit.
- The returned model, gateway, prompt, token, and temperature settings match the
  JSON shape expected by the application.

With rototo, that decision lives in reviewed workspace files:

- `rototo-workspace.toml` declares environments and, when needed, the context
  schema.
- `qualifiers/enterprise-accounts.toml` names the account condition.
- `variables/llm-agent-config.toml` defines the application-facing variable and
  its environment rules.
- `resources/llm-agent-config.toml` points at the schema for agent
  configuration objects.
- `resources/llm-agent-config-objects/enterprise.toml` stores the larger
  enterprise object that the variable can select.
- `schemas/llm-config.schema.json` validates each resource object.
- Optional Lua lint enforces local policy that belongs to this workspace.

At runtime, the application supplies context:

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 250
  },
  "request": {
    "country": "DE"
  }
}
```

rototo evaluates the qualifier rules against that context, selects the matching
value for `prod`, validates the selected value, and returns the JSON
configuration the agent should use. Logs and JSON output can include both the
selected value key and the returned value, which gives operators a way to
explain production behavior from the workspace version that made the decision.

## Where to Go Next

If you are evaluating rototo, start with `why-rototo`. It explains the runtime
configuration problem rototo is designed for and when the model is a good fit.

If you want the vocabulary before running commands, read `rototo-model`. It
explains how workspaces, environments, context, qualifiers, variables,
resources, schemas, and values compose during resolution.

If you want to run something, continue with `quickstart`. It creates a small
local workspace, lints it, and resolves one variable.

After that, read `production-workflow` for a Git-backed workflow with schemas,
qualifiers, variables, tests, app loading, refresh, and observability.

Use the rest of the docs by intent:

- Concepts explain the model and the runtime architecture.
- Tutorials walk through complete first-time workflows.
- How-to guides cover authoring configuration, validating changes, integrating
  applications, keeping config fresh, and debugging production selections.
- Reference pages specify exact file formats, commands, SDK APIs, diagnostics,
  source URI behavior, and JSON output.
- Examples show complete production patterns you can adapt: deployment-lane
  limits, reviewed account conditions, structured LLM config, tenant
  exceptions, operational overrides, and stable percentage rollouts.

## Bundled documentation

```text
Concepts
  why-rototo
  rototo-model

Tutorials
  quickstart
  production-workflow

How-to guides: authoring configuration
  how-to-add-a-new-runtime-config-value
  how-to-change-a-config-value-for-one-environment
  how-to-add-a-new-context-field
  how-to-select-a-value-for-a-runtime-condition
  how-to-move-large-values-out-of-toml

How-to guides: validation and policy
  how-to-test-a-config-change-before-merge
  how-to-enforce-a-config-policy

How-to guides: application integration
  how-to-load-config-from-a-git-repo-in-an-app
  how-to-keep-config-fresh-in-a-running-app

How-to guides: operations and debugging
  how-to-investigate-why-a-value-was-selected
  how-to-diagnose-a-failing-workspace

Examples
  example-environment-specific-limits
  example-reviewed-account-class
  example-llm-agent-configuration
  example-tenant-specific-runtime-config
  example-incident-banner
  example-bucketed-rollout

Reference
  workspace-manifest-reference
  qualifier-reference
  variable-reference
  resource-reference
  predicate-reference
  context-reference
  environment-reference
  value-types-reference
  source-uri-reference

API
  cli-reference
  rust-sdk-reference
  diagnostic-reference
  json-output-reference
```
