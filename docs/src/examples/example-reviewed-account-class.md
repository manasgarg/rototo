# Example: Select Behavior for a Reviewed Account Class

This example models behavior selected for a named account class, with the
condition owned by the workspace instead of application branches or an external
rule system.

Use this pattern when a reviewed account class or request condition should
receive different behavior: enterprise accounts, internal users, countries,
regions, or tenants.

## Production problem

Payment reviews should use the standard queue by default in production, but
premium accounts in Germany should use a regional priority queue.

The application resolves one variable:

```text
payment-review-queue
```

The workspace owns the condition definition and the selection rule.

## Workspace shape

```text
config/
  rototo-workspace.toml
  schemas/
    context.schema.json
  qualifiers/
    premium-germany.toml
  variables/
    payment-review-queue.toml
```

## Runtime context

The application supplies account and request facts:

```json
{
  "account": {
    "plan": "premium"
  },
  "request": {
    "country": "DE"
  }
}
```

Declare that shape in the context schema so rule paths are validated.

## Qualifier

Create `qualifiers/premium-germany.toml`:

```toml
schema_version = 1

description = "Premium accounts making requests from Germany"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "premium"

[[predicate]]
attribute = "request.country"
op = "eq"
value = "DE"
```

The qualifier gives the account class a name. The variable can now use
`premium-germany` instead of repeating raw predicates.

## Variable

Create `variables/payment-review-queue.toml`:

```toml
schema_version = 1

description = "Payment review queue"
type = "string"

[values]
standard = "standard-review"
regional_priority = "de-priority-review"

[env._]
value = "standard"

[env.prod]
value = "standard"

[[env.prod.rule]]
description = "Premium German accounts use regional priority review"
qualifier = "premium-germany"
value = "regional_priority"
```

## Verify the behavior

Matching context:

```sh
rototo variable resolve payment-review-queue \
  --workspace config/ \
  --env prod \
  --context '{"account":{"plan":"premium"},"request":{"country":"DE"}}'
```

Non-matching context:

```sh
rototo variable resolve payment-review-queue \
  --workspace config/ \
  --env prod \
  --context '{"account":{"plan":"free"},"request":{"country":"DE"}}'
```

The matching request should select `regional_priority`. The non-matching
request should select `standard`.

## Tests to keep

Test at least:

- matching account class satisfies all predicates;
- non-matching account class misses one predicate;
- context schema rejects a malformed request context.

## Fit

Use this when a named account class should receive different behavior and the
condition definition should be reviewed in configuration.

Do not use this pattern for percentage rollout. Use a bucket predicate for a
deterministic rollout bucket.

## Related docs

- `how-to-select-a-value-for-a-runtime-condition`
- `how-to-add-a-new-context-field`
- `qualifier-reference`
- `predicate-reference`
