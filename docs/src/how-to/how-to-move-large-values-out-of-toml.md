# How to Move Large Values Into Resources

Use this when a primitive variable is no longer the right shape because the
configured value has become a structured object, prompt bundle, message, or
long configuration document.

The goal is to keep the variable file focused on the application contract and
selection logic while each large value lives in its own file.

## Expected outcome

After this change:

- The variable file still owns the variable id and environment rules.
- The structured value contract lives in a resource schema.
- Large values live under `resources/<resource-id>-objects/`.
- The same value keys resolve as before.
- Lint validates resource objects before applications load the workspace.

## Create the Resource

For this variable:

```text
variables/llm-agent-config.toml
```

create a resource definition and object directory:

```text
resources/llm-agent-config.toml
resources/llm-agent-config-objects/
```

The resource definition points at the schema for every object:

```toml
schema_version = 1
schema = "../schemas/llm-config.schema.json"
```

Each TOML file in the object directory defines one value key:

```text
resources/
  llm-agent-config.toml
  llm-agent-config-objects/
    standard.toml
    enterprise.toml
```

The file stems define the value keys `standard` and `enterprise`.

## Move the Object Content

If the inline value was an object:

```toml
[values.enterprise]
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

move it to `resources/llm-agent-config-objects/enterprise.toml`:

```toml
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

## Keep selection logic in the variable file

The variable file should point at the resource and keep the environment rules:

```toml
schema_version = 1

description = "LLM settings for the incident summary agent"
type = "resource:llm-agent-config"

[env._]
value = "standard"

[env.prod]
value = "standard"

[[env.prod.rule]]
description = "Enterprise accounts get the larger agent configuration"
qualifier = "enterprise-accounts"
value = "enterprise"
```

Do not keep `[values]` on a resource-backed variable. The object directory is
the source of truth for selectable values.

## Verify the move

Lint the workspace:

```sh
rototo lint config/
```

Resolve a value that moved:

```sh
rototo resolve config/ --variable llm-agent-config \
  --env prod \
  --context '{"account":{"plan":"enterprise","seats":250}}' \
  --json
```

Check that `value_key` is unchanged and the returned `value` has the expected
object.

## Common mistakes

Do not change value keys while moving files. First move the storage location;
rename keys in a separate reviewed change if needed.

Do not move the environment mapping into the resource object. Selection stays in the
variable file.

## Related docs

- `resource-reference` specifies resources and resource objects.
- `variable-reference` specifies resource-backed variables.
- `value-types-reference` explains primitive and structured values.
- `json-output-reference` specifies `value_key` and `value`.
