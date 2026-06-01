# rototo

`rototo` is a control plane for runtime configuration in application code.

Configuration decides what code path is active, what limits apply, which
customers receive a rollout, how an AI model is configured, and what operational
message a user sees. rototo treats those decisions like software: versioned in
Git, reviewed, tested, released, loaded by services, resolved at runtime, and
observable in production.

## The configuration lifecycle

The unit of configuration is a workspace. A workspace is a directory in Git with
a `rototo-workspace.toml` manifest and the files that define runtime behavior.
It can contain runtime behavior gates, per-environment values, JSON schemas,
and custom lint policy.

```text
workspace files
    |
    v
Git review
    |
    v
automated tests: inspect, lint, resolve representative cases
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

The result is configuration that can move quickly without becoming invisible,
untyped, or detached from the software delivery process.

## What rototo controls

rototo is for runtime decisions that should not be hardcoded but still need
strong ownership and validation:

- Set `max-output-tokens` to one value in `dev`, another in `stage`, and a
  stricter value in `prod`.
- Route enterprise accounts to a different fraud review queue when cart value is
  high.
- Select an LLM agent configuration by environment, tenant tier, and rollout
  state.
- Serve a support banner only to users affected by an incident.
- Validate that a structured recommendation config still matches the schema the
  application expects.
- Route premium accounts in Germany to a priority payment review queue while
  keeping the standard queue for everyone else.

rototo keeps these decisions outside application binaries while keeping them
testable, explicit, and owned in source control.

## A concrete example

Suppose a SaaS product has an LLM agent that helps users summarize operational
incidents.

The team wants this behavior:

- In `dev`, engineers should use a local model so development does not spend
  production model budget.
- In `prod`, most accounts should use the standard hosted model and the normal
  incident-summary prompt.
- Enterprise accounts should use the larger model, a more specific prompt, and
  a higher output token limit.
- The returned model, gateway, prompt, token, and temperature settings must
  match the JSON shape expected by the application.

Without rototo, this logic usually spreads across application code, deployment
environment variables, feature flag rules, and undocumented operational
exceptions. With rototo, the decision is represented as reviewed workspace
files:

- `rototo-workspace.toml` declares environments such as `dev`, `stage`, and
  `prod`.
- A qualifier such as `enterprise-accounts` answers whether the current context
  matches an account condition.
- A variable such as `llm-agent-config` defines named model, gateway, prompt,
  token, and temperature configurations.
- A JSON Schema validates the structure of the LLM config.
- Optional Lua lint can enforce local policy, such as required fields or naming
  conventions.

The workspace can also define related variables, such as `max-output-tokens`,
when an application wants to control token budget independently from model and
prompt selection.

At runtime, the application supplies context:

```json
{
  "account": {
    "tier": "enterprise",
    "plan": "enterprise",
    "seats": 250
  },
  "request": {
    "country": "DE"
  }
}
```

rototo evaluates the qualifier rules against that context, selects the matching
variable value for `prod`, validates the selected value, and returns the JSON
configuration the agent should use.

For the enterprise context above, `llm-agent-config` can resolve to a value like
this:

```json
{
  "model": "gpt-5",
  "gateway": "openai",
  "prompt": "You are a precise development assistant for enterprise workflows.",
  "max_output_tokens": 5000,
  "temperature": 0.2
}
```

## Data model

```text
workspace
  |
  +-- environments
  |     +-- dev
  |     +-- stage
  |     +-- prod
  |
  +-- qualifiers
  |     +-- enterprise-accounts
  |     +-- eu-users
  |
  +-- variables
  |     +-- llm-agent-config
  |     |     +-- values: local, standard, enterprise
  |     |     +-- env rules: dev, stage, prod
  |     |
  |     +-- max-output-tokens
  |           +-- values: low, standard, high
  |           +-- env rules: dev, stage, prod
  |
  +-- schemas
  |     +-- context schema
  |     +-- variable value schemas
  |
  +-- custom lint
        +-- workspace-specific policy

runtime input
  |
  +-- environment: prod
  +-- context: account plan, account seats, request country
        |
        v
resolution
  |
  +-- validate context
  +-- evaluate qualifiers
  +-- select variable value
  +-- validate selected value
        |
        v
JSON value returned to application
```

A workspace is the versioned source of truth. Qualifiers describe reusable
conditions. Variables use environments and qualifiers to select values. Schemas
validate the contract between configuration and application code. Diagnostics
explain invalid workspace files with stable rule ids.

The same model is available through the CLI and the Rust SDK.

## Where to go next

If you are evaluating rototo, start with `why-rototo`. It explains the runtime
configuration problem rototo is designed for and when the model is a good fit.

If you want the vocabulary before running commands, read `model`. It explains
the core objects and how resolution turns workspace, environment, and context
into a selected value.

If you want to run something, continue with `quickstart`. It is the shortest
guided path: create a small workspace, lint it, and resolve one variable.

After that, read `production-workflow` for a production workflow with a separate
Git repository, context schema, qualifier, resource schema, tests, Git URI
loading, application integration, and observability.

After that, use the docs by intent:

- Concepts explain the model: what rototo is, why it exists, and how resolution
  works.
- Tutorials walk through complete first-time workflows.
- How-to guides explain operational tasks, such as adding a config value,
  selecting a value for a runtime condition, testing a config change, loading a Git
  workspace from an app, refreshing config, and investigating a resolution.
  They are grouped by the work you are doing: authoring config, validating
  changes, integrating an application, and operating production config.
- Reference pages specify exact CLI, SDK, file format, source URI, and
  diagnostic behavior.
- Examples show complete production patterns you can adapt: deployment-lane
  limits, reviewed account conditions, structured LLM config, tenant exceptions,
  operational overrides, and stable percentage rollouts.

## Bundled documentation

```text
Concepts
  why-rototo
  model

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
  cli
  sdk
  diagnostics
  json-output-reference
```
