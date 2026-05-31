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

Every variable file uses `schema_version = 1`, a type source, named values, and
environment selection blocks:

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

The `_` environment is required. It is the fallback used when a variable does
not define a block for the requested environment.

## `schema_version`

Required. Must be:

```toml
schema_version = 1
```

Files without a supported schema version fail lint.

## `description`

Optional but recommended. Use it to explain what application-facing behavior the
variable controls.

```toml
description = "LLM settings for the incident summary agent"
type = "string"
```

## `type`

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

## `schema`

Declares a JSON Schema file for structured values. A variable must declare
exactly one of `type` or `schema`.

```toml
description = "LLM settings for the incident summary agent"
schema = "../schemas/llm-config.schema.json"
```

The schema path is resolved relative to the variable file. Each configured value
is validated against the schema during lint.

## Custom Lint

Variables do not point at custom lint scripts. Custom lint is workspace-scoped:
the manifest declares rule metadata with `[[lint.rule]]`, and rototo
auto-discovers Lua files under `lint/*.lua`.

A Lua file registers handlers with `register(lint)`:

```lua
function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value.max_output_tokens",
    rule = "platform/max-output-token-budget",
    handler = "check_token_budget",
  })
end

function check_token_budget(ctx)
  if ctx.target.value.max_output_tokens > 5000 then
    return {
      {
        message = "value " .. ctx.target.name .. " exceeds the token budget",
      }
    }
  end
  return {}
end
```

Handlers return diagnostics with `message`. The registration owns the rule id,
and the manifest declaration owns the diagnostic title and help text. A
diagnostic can also return `field` to point at a narrower target field using the
same field grammar as the registration target. If the returned field is not
valid for that target, rototo keeps the diagnostic on the registered target.

## Values

A variable must define at least one value. Values can be inline in the variable
file, external in a sibling `*-values/` directory, or both.

Value keys are names used by environment mappings and rules. They are also
returned in resolution output as `value_key`.

### Inline Primitive Values

Use `[values]` for primitive values:

```toml
[values]
small = 500
standard = 1000
large = 2000
```

### Inline Object Values

Use nested tables for object values:

```toml
[values.standard]
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = 2400
temperature = 0.3

[values.enterprise]
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

External values are merged into `[values]` before lint, custom lint,
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

Variable environment blocks live under `[env]`.

The fallback block is required:

```toml
[env._]
value = "standard"
```

Named environment blocks are optional:

```toml
[env.dev]
value = "small"

[env.prod]
value = "large"
```

Environment names other than `_` must be declared in the workspace manifest
under `[environments].values`.

When resolving a variable, rototo first looks for a block matching the requested
environment. If there is no matching block, it uses `[env._]`.

Each environment block must contain:

```toml
value = "<value-key>"
```

The value key must exist in the expanded values table.

## Rules

Rules let an environment block select a value by qualifier.

```toml
[env.prod]
value = "standard"

[[env.prod.rule]]
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
- Exactly one of `type` or `schema` is declared.
- Values exist after external value files are loaded.
- Primitive values match `type`.
- Schema-backed values match the referenced JSON Schema.
- `[env._]` exists.
- Environment blocks are tables with `value`.
- Named environments are declared in the workspace manifest.
- Environment `value` references point at known value keys.
- Rule `qualifier` references point at known qualifier ids.
- Rule `value` references point at known value keys.
- Registered custom lint returns no diagnostics.

Context schema validation happens during resolution, before qualifiers and rules
are evaluated. Value type and value schema checks happen during lint, before the
workspace is used.

## Complete Examples

### Primitive Inline Variable

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

### Schema-Backed Variable With External Values

```toml
schema_version = 1

description = "LLM settings for the incident summary agent"
schema = "../schemas/llm-config.schema.json"

[env._]
value = "standard"

[env.prod]
value = "standard"

[[env.prod.rule]]
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
