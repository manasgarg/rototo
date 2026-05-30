# The rototo Model

When an application asks for configuration at runtime, what exactly is rototo
evaluating?

That question matters because the application is not reading a constant from
its own code. It is asking a reviewed configuration workspace for an answer that
can depend on the environment and the current request.

The complete lifecycle has two moving parts: the configuration workspace and
the application that consumes it. They are developed, tested, and deployed
separately. The workspace is released to a source such as a Git ref, and the
application is deployed with a URI for that source.

```text
Phase 1: release the configuration workspace

config author             workspace source
     |                           |
     | create workspace          |
     | define variables,         |
     | qualifiers, schemas       |
     |                           |
     | pre-push validation       |
     | lint and tests            |
     |                           |
     | review and merge          |
     |-------------------------->|
     |                           | controlled Git ref
     |                           | exposes workspace URI

Phase 2: build and deploy the application

app developer             workspace source             app CI              deployed app
     |                           |                      |                      |
     | use workspace URI         |                      |                      |
     | in SDK tests              |                      |                      |
     |-------------------------->|                      |                      |
     |                           | returns workspace    |                      |
     |<--------------------------|                      |                      |
     |                           |                      |                      |
     | pre-push validation       |                      |                      |
     | app tests with workspace  |                      |                      |
     |                           |                      |                      |
     | push app change           |                      |                      |
     |------------------------------------------------->|                      |
     |                           |                      | load workspace       |
     |                           |<---------------------| for integration test |
     |                           | returns workspace    |                      |
     |                           |--------------------->|                      |
     |                           |                      | app CI passes        |
     |                           |                      |--------------------->|
     |                           |                      |                      | deployed with
     |                           |                      |                      | workspace URI

Phase 3: resolve configuration at runtime

deployed app              workspace source
     |                           |
     | load workspace at startup |
     |-------------------------->|
     |                           | returns workspace version
     |<--------------------------|
     |
     | request arrives with runtime context
     | resolve variable for environment + context
     | use selected value

Phase 4: refresh after a config release

config author             workspace source             deployed app
     |                           |                           |
     | update workspace          |                           |
     | pre-push validation       |                           |
     | lint, test, review        |                           |
     |-------------------------->| updated Git ref           |
     |                           |                           |
     |                           | refresh workspace         |
     |                           |<--------------------------|
     |                           | returns new workspace     |
     |                           |-------------------------->|
     |                           |                           | future resolutions
     |                           |                           | use refreshed config
```

Inside the deployed application, each resolution is smaller than that whole
lifecycle. The application asks for one variable in one environment with one
runtime context. rototo evaluates that request against the currently loaded
workspace version:

```text
workspace version + variable id + environment + runtime context
  -> validate context
  -> evaluate qualifiers
  -> select value key
  -> validate selected value
  -> return value and selection metadata
```

The rest of this page defines the pieces in those two flows and how they fit
together.

## Workspace

A workspace is the control-plane boundary for rototo configuration. It is a
directory tree with a `rototo-workspace.toml` manifest and the files that define
runtime decisions.

The workspace is the unit that gets reviewed, linted, tested, loaded by the
CLI, loaded by the SDK, and shipped through Git. Keeping that boundary explicit
prevents configuration from becoming a mix of application constants, deployment
variables, dashboard rules, and undocumented overrides.

The common shape is:

```text
config/
  rototo-workspace.toml
  qualifiers/
    enterprise-accounts.toml
  variables/
    llm-agent-config.toml
  schemas/
    context.schema.json
    llm-config.schema.json
```

The manifest declares workspace-level facts such as the valid environments and,
when needed, the context schema. Qualifier and variable files are discovered
from conventional directories. The file stem is the id: `llm-agent-config.toml`
defines the variable id `llm-agent-config`.

## Workspace Source

An application or tool does not have to load a workspace only from a local
directory. A workspace source can point at a local path, a `file://` URI, a Git
repository, or an HTTPS archive source.

Git sources make the deployment model explicit:

```text
git+https://github.com/acme/runtime-config.git#prod:config
```

That source means: load the workspace from the `config` directory at ref `prod`.
The application can be deployed with this source URI instead of embedding the
configuration files into the application binary.

rototo does not require a specific release convention. A small team might use
`main` as the reviewed production source. A team that wants explicit promotion
can use a mutable production branch such as `prod` or `release/prod`, and move
that ref only after workspace checks pass. Tags and commit refs are useful for
reproducible tests or pinned deployments, but immutable refs do not produce new
refresh results because the source does not move.

## Environment

An environment is the runtime or deployment lane in which a variable is
resolved. Common environments are `dev`, `stage`, and `prod`.

The environment is not just a string tag. It is part of the selection input.
The same application-facing variable can resolve differently in different
environments while application code keeps asking for the same id.

```text
max-output-tokens in dev   -> small
max-output-tokens in stage -> standard
max-output-tokens in prod  -> large
```

Declaring environments in the workspace gives rototo a validation boundary. A
request for `prod` can resolve. A misspelled environment such as `prd` is an
error instead of an accidental fallback.

## Runtime Context

Runtime context is the JSON object the application passes when it asks for
configuration. It contains request-time facts that are not known when the
workspace is authored: account plan, tenant region, user attributes, request
country, operation type, rollout bucket, or other application facts.

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

Context keeps request-specific inputs visible. Instead of hiding those facts in
application branches, deployment state, or a feature flag service, the
application supplies them directly to resolution and the workspace declares how
they are used.

## Context Schema

A context schema is the input contract between the application and the
workspace. It is a JSON Schema that describes the runtime context shape the
workspace expects.

The schema exists because runtime decisions often fail at the boundary between
application code and configuration. If config authors write predicates against
`account.seats`, but the application stops sending that field or sends it as a
string, the rule may no longer mean what the author intended.

With a context schema, rototo can reject invalid context before evaluating
qualifiers or variables. That turns a hidden mismatch into a validation failure
with a diagnostic, instead of silently falling through to a default branch or
selecting a value for the wrong reason.

## Qualifier

A qualifier is a named condition over runtime context.

The name is the important part. Production decisions are usually about a
business or operational condition, not about a raw JSON path. A qualifier such
as `enterprise-accounts` can mean "the account is on the enterprise plan and
has at least 100 seats." Variables, tests, diagnostics, and future config can
refer to that name instead of repeating the predicate everywhere.

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

Qualifiers are reusable. A workspace can resolve a qualifier directly for
debugging, or use it inside variable rules to select different values.

## Variable

A variable is the application-facing configuration contract. Application code
asks for a variable id such as `max-output-tokens` or `llm-agent-config`; it
does not need to know which qualifiers, branches, or rules are active.

A variable defines possible values and the logic for selecting one of them in
an environment. It also declares either a primitive type or a JSON Schema for
the returned value.

```toml
schema_version = 1

description = "Maximum number of tokens the summarizer can emit"
type = "int"

[values]
small = 500
standard = 1000
large = 2000

[env._]
value = "standard"

[env.dev]
value = "small"

[env.prod]
value = "large"
```

The variable id is stable from the application's point of view. The workspace
can change which value key is selected in each environment without requiring
application code to change.

## External Value Files

Values can live inline in the variable file, or in a sibling directory when the
values are large enough to deserve their own files.

Inline values keep small decisions close to the variable:

```toml
[values]
small = 500
standard = 1000
large = 2000
```

External value files keep richer values readable and reviewable. For a variable
file named `variables/llm-agent-config.toml`, rototo also loads value files
from `variables/llm-agent-config-values/*.toml`. Each file stem becomes the
value key.

```text
variables/
  llm-agent-config.toml
  llm-agent-config-values/
    standard.toml
    enterprise.toml
```

An external value file can hold a scalar:

```toml
value = "Welcome back, premium member."
```

Or it can hold an object under `[value]`:

```toml
[value]
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

Resolution does not care whether the selected value came from the variable file
or a separate value file. The workspace loader expands both forms into the same
variable model before linting and resolution.

## Value Key and Value

A value key is the name of a configured branch inside a variable. The value is
the actual JSON-compatible data returned to the application.

For `max-output-tokens`, the value key might be `large` and the value might be
`2000`. For `llm-agent-config`, the value key might be `enterprise` and the
value might be an object containing the model, gateway, prompt, token limit, and
temperature.

Keeping both pieces matters. The application needs the value. Humans, tests,
logs, and agents often need the value key because it explains which configured
branch produced the value.

In CLI and JSON output, the selected branch name appears as `value_key`.

## Rules

Rules let a variable choose a value based on qualifiers.

The environment block provides the default value for that environment. Rules can
override that default when their qualifier matches the runtime context.

```toml
[env.prod]
value = "standard"

[[env.prod.rule]]
description = "Enterprise accounts get the larger agent configuration"
qualifier = "enterprise-accounts"
value = "enterprise"
```

This means `prod` uses `standard` unless `enterprise-accounts` matches. When
the qualifier matches, rototo selects the `enterprise` value key instead.

Rules keep application code out of the decision. The application still asks for
`llm-agent-config` with environment and context; the workspace owns the
selection logic.

## Resolution

Resolution is the process that turns a variable id, environment, context, and
workspace version into a selected value.

For a variable resolution, rototo follows this shape:

```text
1. Load the workspace source.
2. Validate the workspace structure.
3. Validate the requested environment.
4. Validate runtime context against the context schema, if one is declared.
5. Find the requested variable.
6. Evaluate qualifiers referenced by matching rules.
7. Select the value key for the environment.
8. Validate the selected value against the variable type or schema.
9. Return the selected value key and value.
```

The exact result is not just "the value for this key." It is the answer to a
more precise question:

```text
Given this workspace version, environment, and runtime context,
which reviewed branch applies, and what value should the application receive?
```

That distinction is the core of rototo. It is a runtime configuration resolver,
not only a storage format.

## Refreshing Workspaces

Configuration does not have to be fixed at application deployment time.

In the production model, the configuration workspace is deployed separately
from the application binary. The application is deployed with a workspace source
URI. At startup, the SDK loads the workspace from that source. A long-running
service can then refresh the workspace periodically from the same source.

```text
configuration repo
  |
  | review, lint, test, merge
  v
controlled Git ref
  |
  | workspace source URI
  v
application instances
  |
  +-- load workspace at startup
  +-- periodically refresh workspace
  +-- resolve variables from current workspace
```

A successful refresh replaces the active workspace for future resolutions. If a
refresh fails because Git is unavailable, authentication breaks, or the new
workspace is invalid, the application continues resolving from the last
successfully loaded workspace.

That behavior gives teams a way to release reviewed configuration changes
without rebuilding the application while avoiding a hard dependency on every
refresh attempt succeeding.

## Custom Lint

Built-in validation checks the rototo model: manifests, environments,
qualifier references, value keys, schemas, and value types. Some teams also
need workspace-specific policy that rototo cannot know in advance.

Custom lint is declared on a variable. The variable points at a Lua file owned
by the workspace:

```toml
[lint]
path = "../lint/llm-agent-config.lua"
```

The Lua file can define hooks at two levels:

- `lint(variable)` validates the expanded variable as a whole.
- `lint_value(value)` validates each value after inline and external value files
  have been loaded.

```lua
function lint_value(value)
  if value.value.max_output_tokens > 5000 then
    return {
      {
        message = "value " .. value.name .. " exceeds the token budget",
        help = "Use 5000 or fewer output tokens."
      }
    }
  end
  return {}
end
```

Custom lint is for local policy: naming conventions, budget limits, required
metadata, allowed model families, rollout rules, or organization-specific
constraints. It runs with workspace lint, so policy failures can block local
pre-push checks, CI, and release promotion before an application loads the
workspace.

The current custom lint declaration is variable-scoped. Workspace-level and
qualifier-level custom lint are not separate extension points today.

## Validation and Diagnostics

rototo validates configuration at multiple points because different mistakes
appear at different times.

Workspace lint checks the files before release: parse errors, invalid
manifests, unknown environments, missing values, unknown qualifiers, invalid
schemas, external value files, and custom lint failures. Context validation
checks the runtime input the application supplies. Value validation checks the
selected output before the application consumes it.

Diagnostics use stable rule ids so humans and agents can recognize failure classes
and link them to reference documentation. That makes validation usable in local
development, CI, release automation, and application startup.

## Observability

Once runtime decisions move out of application code, production systems need a
record of what rototo decided.

An evaluation record should capture:

- the workspace source and version that made the decision;
- the variable id and environment;
- the selected value key;
- the matched qualifier or rule;
- the relevant runtime context attributes, with redaction where needed;
- request trace information from the application.

That record lets operators answer the questions that matter during debugging:
which value did this request receive, which configuration version produced it,
and why did that branch match?

## Boundaries

rototo is for runtime configuration decisions that need review, validation,
testing, release discipline, and explainability.

It is not intended to replace every configuration mechanism. Process-level
settings such as listening ports, database connection strings, and secrets may
still belong in deployment or secret-management systems. Application code still
owns business logic. rototo owns the reviewed runtime decision: which configured
value should this application receive for this environment and context?

That boundary keeps the model small enough to reason about while still covering
the decisions that become risky when they are scattered across code, deployment
state, dashboards, and operational memory.

## What to Read Next

Read `quickstart` if you want to create a small local workspace and resolve one
variable from the CLI.

Read `production-workflow` if you want to see the same model with a separate Git
workspace repository, context schema, qualifier rules, tests, SDK loading,
refresh, and observability.
