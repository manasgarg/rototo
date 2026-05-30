# Example: Control Structured LLM Agent Config Safely

This example models a structured LLM configuration that application code can
load at runtime while the workspace owns validation, selection rules, external
value files, and local policy.

Use this pattern when the value is not a scalar knob. LLM configuration usually
has a model, gateway, prompt, token limit, temperature, and local policy rules.
That should be schema-validated and reviewable as configuration.

## Production problem

An incident summary agent has three configurations:

- `local` for development;
- `standard` for most production accounts;
- `enterprise` for enterprise accounts.

The application resolves:

```text
llm-agent-config
```

and receives one structured object.

## Workspace shape

```text
config/
  rototo-workspace.toml
  schemas/
    context.schema.json
    llm-config.schema.json
  qualifiers/
    enterprise-accounts.toml
  variables/
    llm-agent-config.toml
    llm-agent-config-values/
      local.toml
      standard.toml
      enterprise.toml
  lint/
    llm-agent-config.lua
```

## Value schema

Create `schemas/llm-config.schema.json`:

```json
{
  "type": "object",
  "required": ["model", "gateway", "prompt", "max_output_tokens", "temperature"],
  "properties": {
    "model": { "type": "string" },
    "gateway": { "type": "string" },
    "prompt": { "type": "string" },
    "max_output_tokens": { "type": "integer", "minimum": 1 },
    "temperature": { "type": "number", "minimum": 0, "maximum": 2 }
  },
  "additionalProperties": false
}
```

The schema is the contract application code can rely on.

## Variable

Create `variables/llm-agent-config.toml`:

```toml
schema_version = 1

description = "LLM settings for the incident summary agent"
schema = "../schemas/llm-config.schema.json"

[lint]
path = "../lint/llm-agent-config.lua"

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

The variable file stays focused on the application contract and selection
rules. The large values live in separate files.

## External values

Create `variables/llm-agent-config-values/enterprise.toml`:

```toml
[value]
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

Create similar files for `local` and `standard`.

## Policy lint

Create `lint/llm-agent-config.lua`:

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

The schema validates shape. Custom lint enforces local policy.

## Verify the behavior

```sh
rototo variable resolve llm-agent-config \
  --workspace config/ \
  --env prod \
  --context '{"account":{"plan":"enterprise","seats":250}}' \
  --json
```

Expected selected key:

```text
enterprise
```

## Tests to keep

Test the default production path, enterprise production path, and development
path. Also include one lint fixture or test change that proves policy catches a
value above the token ceiling.

## Fit

Use this when application code needs a structured object and the structure
should be validated before release.

Do not use this pattern for a single scalar setting. A primitive variable is
clearer for small knobs.

## Related docs

- `how-to-move-large-values-out-of-toml`
- `how-to-enforce-a-config-policy`
- `variable-reference`
- `value-types-reference`
