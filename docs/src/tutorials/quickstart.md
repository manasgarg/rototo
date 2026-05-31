# Quickstart

This quickstart configures one runtime decision for an LLM summarizer: how many
tokens the summarizer is allowed to emit.

The product requirement is simple:

- In `dev`, keep responses small so local iteration is fast.
- In `stage`, use a moderate limit for realistic testing.
- In `prod`, allow longer summaries for real users.

Without rototo, this kind of decision often starts as a constant in application
code, then becomes an environment variable, then spreads across deployment
scripts and service defaults. rototo keeps the decision in a workspace: a small,
versionable directory that the CLI and SDK can load.

In this quickstart, the workspace will contain one variable:
`max-output-tokens`. The application asks for that variable in an environment,
and rototo returns both the selected value key and the selected integer.

```text
environment: dev   -> max-output-tokens = 500
environment: stage -> max-output-tokens = 1000
environment: prod  -> max-output-tokens = 2000
```

You will create the workspace, define the variable, lint the configuration, and
resolve the value from the CLI.

## Create a workspace

A workspace is the root directory for rototo configuration. It contains a
manifest named `rototo-workspace.toml` and files that define runtime behavior.

After this quickstart, the directory will look like this:

```text
token-config/
  rototo-workspace.toml
  variables/
    max-output-tokens.toml
```

Create `rototo-workspace.toml`:

```toml
schema_version = 1

[environments]
values = ["dev", "stage", "prod"]
```

The manifest declares the valid environments. That gives rototo a boundary: a
request for `prod` is valid, while a misspelled environment such as `prd` is an
error instead of an accidental fallback.

At this point, the workspace exists but has no configuration values. The next
step adds the value the application will ask for.

## Define a variable

A variable is the named config value an application resolves at runtime. Here,
the variable is `max-output-tokens`.

Create `variables/max-output-tokens.toml`:

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

[env.stage]
value = "standard"

[env.prod]
value = "large"
```

This file separates three ideas:

- `type = "int"` says the application should receive an integer.
- `[values]` defines the possible named values.
- `[env.*]` chooses which named value each environment receives.

The selected name is called the value key. In `prod`, the value key is `large`
and the returned value is `2000`. The `_` environment is the fallback used when
an environment does not define its own value.

The workspace now has the whole decision model for this quickstart:

```text
workspace
  +-- environments: dev, stage, prod
  +-- variable: max-output-tokens
        +-- values: small, standard, large
        +-- env mapping: dev -> small, stage -> standard, prod -> large
```

Before using it, validate that the files fit rototo's rules.

## Validate the workspace

Linting catches structural mistakes before application code depends on the
workspace. For this example, lint verifies that the manifest is valid, the
variable declares a type, the fallback exists, and every environment points to a
real value key.

Run:

```sh
rototo lint token-config/
```

Expected output:

```text
ok: /path/to/token-config
```

The exact path will be your local `token-config` directory.

Now the workspace is valid. The final step is to ask rototo the same question an
application would ask at runtime.

## Resolve the variable

Resolution takes a variable id, an environment, and runtime context. This first
workspace does not need request-specific context, so pass an empty JSON object
with `--context '{}'`.

The question rototo answers is:

```text
Given this workspace, environment, and context,
which reviewed value key applies, and what value should the application receive?
```

Resolve the variable in `dev`:

```sh
rototo resolve --variable max-output-tokens --env dev --context '{}'
```

Expected output:

```text
max-output-tokens=500 (small)
```

Resolve the same variable in `prod`:

```sh
rototo resolve --variable max-output-tokens --env prod --context '{}'
```

Expected output:

```text
max-output-tokens=2000 (large)
```

The application-facing name stayed the same: `max-output-tokens`. The
environment changed, so rototo selected a different value key and returned a
different integer.

For automation or application integration, use JSON output:

```sh
rototo resolve --variable max-output-tokens --env prod --context '{}' --json
```

Expected output:

```json
{
  "workspace": "/path/to/token-config",
  "variables": [
    {
      "id": "max-output-tokens",
      "environment": "prod",
      "value_key": "large",
      "value": 2000
    }
  ],
  "qualifiers": []
}
```

The JSON output includes both the selected value key and the selected value.
That is useful when logs or tests need to explain not only what value was
returned, but which configuration branch produced it.

## What you built

You built a local rototo workspace with one runtime configuration decision:

```text
token-config/
  rototo-workspace.toml
  variables/
    max-output-tokens.toml
```

The core model is:

```text
workspace + variable id + environment + context
  -> selected value key
  -> selected value
```

This quickstart intentionally kept the example small. It did not use runtime
context, qualifiers, schemas, external value files, custom lint, tests, Git
loading, refresh, or observability. Those are the next pieces once the basic
model is clear. Read `model` for the concepts, then `production-workflow` to
see the same model applied to a production-style workflow.
