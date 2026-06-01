# Example: Control Structured LLM Agent Config Safely

This example models a structured LLM configuration that application code can
load at runtime while the workspace owns validation, selection rules, resource
objects, and local policy.

Use this pattern when the value is not a scalar knob. LLM configuration usually
has a model, gateway, prompt, token limit, temperature, and local policy.
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
  resources/
    llm-agent-config.toml
    llm-agent-config-objects/
      local.toml
      standard.toml
      enterprise.toml
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
    "max_output_tokens": { "type": "integer", "minimum": 1, "maximum": 5000 },
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

The variable file stays focused on the application-facing id and selection
rules. The large values live in resource object files.

## Resource Objects

Create `resources/llm-agent-config.toml`:

```toml
schema_version = 1
schema = "../schemas/llm-config.schema.json"
```

Create `resources/llm-agent-config-objects/enterprise.toml`:

```toml
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

Create similar files for `local` and `standard`.

The schema validates shape and enforces the token ceiling before the workspace
can be loaded.

## Verify the behavior

```sh
rototo resolve config/ --variable llm-agent-config \
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
path. Also include one lint fixture or test change that proves schema validation
catches a value above the token ceiling.

## Fit

Use this when application code needs a structured object and the structure
should be validated before release.

Do not use this pattern for a single scalar setting. A primitive variable is
clearer for small knobs.

## Related docs

- `how-to-move-large-values-out-of-toml`
- `how-to-enforce-a-config-policy`
- `variable-reference`
- `resource-reference`
- `value-types-reference`
