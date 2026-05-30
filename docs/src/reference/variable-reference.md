# Variable File Reference

A variable is the application-facing configuration contract. Application code
resolves a variable id in an environment with runtime context, and rototo
returns the selected value key and value.

This page specifies the variable file format and the related external value
file format.

## Location and ID

Variable files live under the workspace `variables/` directory:

```text
variables/<variable-id>.toml
```

The file stem is the variable id. For example:

```text
variables/llm-agent-config.toml -> llm-agent-config
variables/max-output-tokens.toml -> max-output-tokens
```

Variable ids are the names applications resolve. Keep them stable once
application code depends on them.

## Minimal Shape

Every variable file uses `schema_version = 1` and a `[variable]` table:

```toml
schema_version = 1

[variable]
description = "Maximum number of tokens the summarizer can emit"
type = "int"

[variable.values]
small = 500
standard = 1000
large = 2000

[variable.env._]
value = "standard"

[variable.env.dev]
value = "small"

[variable.env.prod]
value = "large"
```

The `_` environment is required. It is the fallback used when a variable does
not define a block for the requested environment.

## `schema_version`

Required. Must be:

```toml
schema_version = 1
```

Files without a supported schema version fail lint.

## `[variable]`

Required. Contains the variable metadata and contract.

### `description`

Optional but recommended. Use it to explain what application-facing behavior the
variable controls.

```toml
[variable]
description = "LLM settings for the incident summary agent"
type = "string"
```

### `type`

Declares a primitive value type. A variable must declare exactly one of `type`
or `schema`.

Supported primitive types:

```text
bool
int
number
string
list
```

Primitive values are checked during lint. A variable with `type = "int"` fails
lint if any configured value is not an integer.

### `schema`

Declares a JSON Schema file for structured values. A variable must declare
exactly one of `type` or `schema`.

```toml
[variable]
description = "LLM settings for the incident summary agent"
schema = "../schemas/llm-config.schema.json"
```

The schema path is resolved relative to the variable file. Each configured value
is validated against the schema during lint.

### `[variable.lint]`

Optional. Declares variable-scoped custom Lua lint.

```toml
[variable.lint]
path = "../lint/llm-agent-config.lua"
```

The path is resolved relative to the variable file. The Lua script can define
`lint(variable)`, `lint_value(value)`, or both.

`lint(variable)` receives the expanded variable, including inline and external
values:

```lua
function lint(variable)
  return {}
end
```

`lint_value(value)` runs once for each expanded value:

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

Each function must return a list of diagnostics. A diagnostic must contain
`message` and may contain `help`.

Custom lint is declared on variables today. Workspace-level and qualifier-level
custom lint are not separate extension points.

## Values

A variable must define at least one value. Values can be inline in the variable
file, external in a sibling `*-values/` directory, or both.

Value keys are names used by environment mappings and rules. They are also
returned in resolution output as `value_key`.

### Inline Primitive Values

Use `[variable.values]` for primitive values:

```toml
[variable.values]
small = 500
standard = 1000
large = 2000
```

### Inline Object Values

Use nested tables for object values:

```toml
[variable.values.standard]
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = 2400
temperature = 0.3

[variable.values.enterprise]
model = "gpt-5"
gateway = "openai"
max_output_tokens = 5000
temperature = 0.2
```

## External Value Files

External value files keep large or structured values separate from the variable
selection logic.

For this variable:

```text
variables/llm-agent-config.toml
```

rototo also loads TOML files from:

```text
variables/llm-agent-config-values/*.toml
```

Each external value file stem is the value key:

```text
variables/
  llm-agent-config.toml
  llm-agent-config-values/
    standard.toml     -> standard
    enterprise.toml   -> enterprise
```

External values are merged into `[variable.values]` before lint, custom lint,
and resolution. If the same value key is declared inline and externally, loading
fails.

### Scalar External Values

If an external value file contains a single top-level `value` key, rototo uses
the contents of that key as the value:

```toml
value = "Welcome back, premium member."
```

The selected value is the string:

```json
"Welcome back, premium member."
```

### Object External Values

Use `[value]` for object values:

```toml
[value]
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

The selected value is the object under `value`.

If an external value TOML file does not consist of a single top-level `value`
key, rototo uses the whole TOML document as the value.

## Environment Mappings

Variable environment blocks live under `[variable.env]`.

The fallback block is required:

```toml
[variable.env._]
value = "standard"
```

Named environment blocks are optional:

```toml
[variable.env.dev]
value = "small"

[variable.env.prod]
value = "large"
```

Environment names other than `_` must be declared in the workspace manifest
under `[environments].values`.

When resolving a variable, rototo first looks for a block matching the requested
environment. If there is no matching block, it uses `[variable.env._]`.

Each environment block must contain:

```toml
value = "<value-key>"
```

The value key must exist in the expanded values table.

## Rules

Rules let an environment block select a value by qualifier.

```toml
[variable.env.prod]
value = "standard"

[[variable.env.prod.rule]]
description = "Enterprise accounts get the larger agent configuration"
qualifier = "enterprise-accounts"
value = "enterprise"
```

Each rule contains:

- `description`: optional but recommended.
- `qualifier`: required. Must reference an existing qualifier id.
- `value`: required. Must reference an existing value key.

Rules are evaluated in the order they appear. The first matching rule selects
its value. If no rule matches, rototo uses the environment block's `value`.

Rules are only evaluated for the selected environment block. If the requested
environment has no block and rototo falls back to `_`, only rules in `_` are
considered.

## Resolution Output

Variable resolution returns:

```json
{
  "id": "llm-agent-config",
  "environment": "prod",
  "value_key": "enterprise",
  "value": {
    "model": "gpt-5",
    "gateway": "openai",
    "max_output_tokens": 5000,
    "temperature": 0.2
  }
}
```

The `environment` field is the environment requested by the caller. The
`value_key` field is the selected branch. The `value` field is the configured
value after inline and external value files have been expanded.

CLI JSON output also includes the workspace source/path.

## Validation

Variable lint checks:

- `schema_version = 1` exists.
- `[variable]` exists.
- Exactly one of `type` or `schema` is declared.
- `[variable.lint]`, when present, is a table with `path`.
- Values exist after external value files are loaded.
- Primitive values match `type`.
- Schema-backed values match the referenced JSON Schema.
- `[variable.env._]` exists.
- Environment blocks are tables with `value`.
- Named environments are declared in the workspace manifest.
- Environment `value` references point at known value keys.
- Rule `qualifier` references point at known qualifier ids.
- Rule `value` references point at known value keys.
- Custom lint returns no diagnostics.

Context schema validation happens during resolution, before qualifiers and rules
are evaluated. Value type and value schema checks happen during lint, before the
workspace is used.

## Complete Examples

### Primitive Inline Variable

```toml
schema_version = 1

[variable]
description = "Maximum number of tokens the summarizer can emit"
type = "int"

[variable.values]
small = 500
standard = 1000
large = 2000

[variable.env._]
value = "standard"

[variable.env.dev]
value = "small"

[variable.env.prod]
value = "large"
```

### Schema-Backed Variable With External Values

```toml
schema_version = 1

[variable]
description = "LLM settings for the incident summary agent"
schema = "../schemas/llm-config.schema.json"

[variable.lint]
path = "../lint/llm-agent-config.lua"

[variable.env._]
value = "standard"

[variable.env.prod]
value = "standard"

[[variable.env.prod.rule]]
description = "Enterprise accounts get the larger agent configuration"
qualifier = "enterprise-accounts"
value = "enterprise"
```

With value files:

```text
variables/
  llm-agent-config.toml
  llm-agent-config-values/
    standard.toml
    enterprise.toml
```

`variables/llm-agent-config-values/enterprise.toml`:

```toml
[value]
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```
