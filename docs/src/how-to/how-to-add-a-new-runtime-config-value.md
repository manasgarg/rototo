# How to Add a New Runtime Config Value

Use this when application code needs a new named configuration value at
runtime, such as a token limit, timeout, model name, endpoint, or structured
settings object.

You are not only adding a TOML file. You are creating an application contract:
one stable id that code can resolve while the workspace owns the values,
environment mapping, validation, and later rollout rules.

## Expected outcome

After this change:

- The workspace contains a new variable file.
- `rototo workspace lint` validates the variable.
- `rototo variable resolve` returns the expected value in each environment.
- Application code can resolve one stable variable id.

## Before you start

Decide three things before editing files:

- The stable variable id the application will resolve.
- The value type or JSON Schema the application expects.
- The default value key to use when no environment-specific rule applies.

For a simple integer value, use `max-output-tokens` as the variable id.

## Add the variable file

Create a file under `variables/`. The file stem is the variable id:

```text
variables/max-output-tokens.toml
```

Add the variable contract, values, and environment mapping:

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

The application will resolve `max-output-tokens`. The selected value key is
returned with the value, so logs and evaluation records can show why the
application received `small`, `standard`, or `large`.

## Validate the workspace

Run lint before wiring the value into application code:

```sh
rototo workspace lint config/
```

Lint verifies that the variable declares a supported type, every value matches
that type, the fallback environment exists, and every environment mapping points
to a known value key.

## Resolve representative environments

Resolve the new variable for the environments that matter:

```sh
rototo variable resolve max-output-tokens \
  --workspace config/ \
  --env dev \
  --context '{}'
```

```sh
rototo variable resolve max-output-tokens \
  --workspace config/ \
  --env prod \
  --context '{}'
```

Use JSON output when adding automated tests:

```sh
rototo variable resolve max-output-tokens \
  --workspace config/ \
  --env prod \
  --context '{}' \
  --json
```

## Common mistakes

Do not skip `[variable.env._]`. The fallback is required and makes the default
behavior explicit.

Do not use a value key as the application contract. Application code should ask
for `max-output-tokens`, not `large`.

Do not encode structured objects as strings. Use a JSON Schema-backed variable
when the application expects an object.

## Related docs

- `variable-reference` specifies variable files.
- `environment-reference` explains environment selection.
- `json-output-reference` specifies CLI JSON output.
