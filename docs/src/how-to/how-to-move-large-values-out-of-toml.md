# How to Move Large Values Out of TOML

Use this when a variable file is becoming hard to review because structured
values, prompts, messages, or long configuration objects are inline beside the
selection rules.

The goal is to keep the variable file focused on the application contract and
selection logic while each large value lives in its own file.

## Expected outcome

After this change:

- The variable file still owns the variable id, schema or type, and environment
  rules.
- Large values live under a sibling `*-values/` directory.
- The same value keys resolve as before.
- Lint validates external values exactly like inline values.

## Create the value directory

For this variable:

```text
variables/llm-agent-config.toml
```

create this sibling directory:

```text
variables/llm-agent-config-values/
```

Each TOML file in that directory defines one value key:

```text
variables/
  llm-agent-config.toml
  llm-agent-config-values/
    standard.toml
    enterprise.toml
```

The file stems define the value keys `standard` and `enterprise`.

## Move the value content

If the inline value is an object:

```toml
[values.enterprise]
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

move it to `variables/llm-agent-config-values/enterprise.toml`:

```toml
[value]
model = "gpt-5"
gateway = "openai"
prompt = "Summarize the incident for an enterprise support workflow."
max_output_tokens = 5000
temperature = 0.2
```

For scalar values, use a single top-level `value` key:

```toml
value = "Welcome back, premium member."
```

## Keep selection logic in the variable file

The variable file should keep the contract and environment rules:

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

Do not keep the same value key inline and externally. rototo rejects duplicate
value keys because there would be two sources of truth.

## Verify the move

Lint the workspace:

```sh
rototo workspace lint config/
```

Resolve a value that moved:

```sh
rototo variable resolve llm-agent-config \
  --workspace config/ \
  --env prod \
  --context '{"account":{"plan":"enterprise","seats":250}}' \
  --json
```

Check that `value_key` is unchanged and the returned `value` has the expected
object or scalar.

## Common mistakes

Do not change value keys while moving files. First move the storage location;
rename keys in a separate reviewed change if needed.

Do not leave duplicate inline and external values with the same key. Loading
fails.

Do not move the environment mapping into the value file. Selection stays in the
variable file.

## Related docs

- `variable-reference` specifies external value files.
- `value-types-reference` explains primitive and structured values.
- `json-output-reference` specifies `value_key` and `value`.
