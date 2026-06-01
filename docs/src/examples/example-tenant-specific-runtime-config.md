# Example: Manage Tenant Exceptions Without App Branches

This example models a tenant exception as reviewed runtime configuration
instead of a special case in application code.

Use this pattern when a small number of tenants or account classes need
different runtime configuration, and those exceptions should be reviewed,
tested, and observable.

## Production problem

Most tenants use the standard search backend. A named enterprise tenant uses a
dedicated backend with a higher timeout.

The application resolves:

```text
search-config
```

and passes tenant facts as context.

## Workspace shape

```text
config/
  rototo-workspace.toml
  schemas/
    context.schema.json
    search-config.schema.json
  qualifiers/
    acme-enterprise-tenant.toml
  variables/
    search-config.toml
```

## Runtime context

```json
{
  "tenant": {
    "id": "acme",
    "plan": "enterprise"
  }
}
```

Use opaque tenant ids or stable account ids. Do not put sensitive tenant
metadata in context unless the application already treats it as safe telemetry.

## Qualifier

Create `qualifiers/acme-enterprise-tenant.toml`:

```toml
schema_version = 1

description = "Acme enterprise tenant"

[[predicate]]
attribute = "tenant.id"
op = "eq"
value = "acme"

[[predicate]]
attribute = "tenant.plan"
op = "eq"
value = "enterprise"
```

This keeps the tenant exception visible in the workspace instead of buried in
application conditionals.

## Variable

Create `variables/search-config.toml`:

```toml
schema_version = 1

description = "Search backend settings"
schema = "../schemas/search-config.schema.json"

[values.standard]
backend = "shared-search"
timeout_ms = 1500

[values.acme]
backend = "acme-dedicated-search"
timeout_ms = 3000

[env._]
value = "standard"

[env.prod]
value = "standard"

[[env.prod.rule]]
description = "Acme uses a dedicated production search backend"
qualifier = "acme-enterprise-tenant"
value = "acme"
```

## Verify the behavior

```sh
rototo resolve config/ --variable search-config \
  --env prod \
  --context '{"tenant":{"id":"acme","plan":"enterprise"}}' \
  --json
```

Expected selected key:

```text
acme
```

## Tests to keep

Test the named tenant and at least one ordinary tenant. If the tenant exception
is production-only, also test that non-production environments do not select
the dedicated backend unless intended.

## Fit

Use this for explicit, reviewed tenant exceptions or account-class behavior.

Do not use this to encode a large tenant database in configuration. rototo
should select runtime behavior, not replace application data storage.

## Related docs

- `how-to-select-a-value-for-a-runtime-condition`
- `how-to-add-a-new-context-field`
- `context-reference`
- `variable-reference`
