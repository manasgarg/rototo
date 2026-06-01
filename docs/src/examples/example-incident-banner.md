# Example: Ship an Operational Override Without Redeploying

This example models an operational override that can move through the config
repository and be picked up by running services after refresh.

Use this pattern when operators need to change visible behavior quickly but the
change still needs review, validation, promotion, refresh, and observability.

## Production problem

During a payment provider incident, users in affected countries should see a
checkout banner. Everyone else should see no banner.

The application resolves:

```text
checkout-banner
```

and receives a small structured object.

## Workspace shape

```text
config/
  rototo-workspace.toml
  schemas/
    context.schema.json
    banner.schema.json
  qualifiers/
    affected-payment-region.toml
  variables/
    checkout-banner.toml
  resources/
    checkout-banner.toml
    checkout-banner-objects/
      none.toml
      payment_incident.toml
```

## Qualifier

Create `qualifiers/affected-payment-region.toml`:

```toml
schema_version = 1

description = "Requests from regions affected by the payment provider incident"

[[predicate]]
attribute = "request.country"
op = "in"
value = ["DE", "FR", "NL"]
```

## Variable

Create `variables/checkout-banner.toml`:

```toml
schema_version = 1

description = "Checkout banner shown during operational incidents"
type = "resource:checkout-banner"

[env._]
value = "none"

[env.prod]
value = "none"

[[env.prod.rule]]
description = "Show the payment incident banner in affected regions"
qualifier = "affected-payment-region"
value = "payment_incident"
```

Create `resources/checkout-banner.toml`:

```toml
schema_version = 1
schema = "../schemas/banner.schema.json"
```

Create `resources/checkout-banner-objects/none.toml`:

```toml
enabled = false
message = ""
severity = "info"
```

Create `resources/checkout-banner-objects/payment_incident.toml`:

```toml
enabled = true
message = "Some payment methods may be delayed. Card checkout is still available."
severity = "warning"
```

## Verify the behavior

```sh
rototo resolve config/ --variable checkout-banner \
  --env prod \
  --context '{"request":{"country":"DE"}}' \
  --json
```

Expected selected key:

```text
payment_incident
```

## Tests to keep

Test one affected country and one unaffected country. Also test the shape of
the returned object so the application never receives a banner without
`enabled`, `message`, and `severity`.

## Production behavior

Deploy the application with a movable workspace source such as a production
branch. After the config change is reviewed and that ref moves, running
services can pick up the new banner on refresh.

## Fit

Use this for short-lived operational behavior that must be controlled outside
application deploys.

Do not use it for messages that need a content-management workflow, rich
localization, approvals beyond code review, or non-engineering editing.

## Related docs

- `how-to-keep-config-fresh-in-a-running-app`
- `how-to-load-config-from-a-git-repo-in-an-app`
- `how-to-select-a-value-for-a-runtime-condition`
- `predicate-reference`
- `resource-reference`
