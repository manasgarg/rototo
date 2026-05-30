# Example: Keep Deployment-Lane Limits Out of Application Code

This example models a limit that changes by deployment lane while application
code keeps resolving the same variable.

Use this pattern for limits that should vary by deployment lane but do not need
request-specific context: token limits, timeouts, retry counts, page sizes, or
batch sizes.

## Production problem

An incident summarizer should use small outputs in development, realistic
outputs in staging, and larger outputs in production.

```text
dev   -> 500
stage -> 1000
prod  -> 2000
```

The application always resolves `max-output-tokens`. It does not branch on the
environment itself.

## Workspace shape

```text
config/
  rototo-workspace.toml
  variables/
    max-output-tokens.toml
```

## Manifest

```toml
schema_version = 1

[environments]
values = ["dev", "stage", "prod"]
```

## Variable

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

The `_` mapping is the explicit fallback. Named environment blocks override it.

## Verify the behavior

```sh
rototo workspace lint config/
```

```sh
rototo variable resolve max-output-tokens \
  --workspace config/ \
  --env prod \
  --context '{}'
```

Expected selected key:

```text
large
```

## Tests to keep

Test each declared environment and assert the selected value key:

```text
dev   -> small
stage -> standard
prod  -> large
```

This prevents a production-only change from accidentally changing development
or staging behavior.

## Fit

Use this when environment is the only selection input.

Do not use this pattern when tenant, account, user, request, or rollout facts
should influence the value. Add runtime context and qualifiers for that.

## Related docs

- `how-to-add-a-new-runtime-config-value`
- `how-to-change-a-config-value-for-one-environment`
- `variable-reference`
