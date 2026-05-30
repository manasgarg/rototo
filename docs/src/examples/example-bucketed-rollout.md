# Example: Run a Stable Percentage Rollout from Config

This example models a deterministic rollout bucket controlled by workspace
configuration.

Use this pattern when a stable fraction of accounts, tenants, or users should
receive new behavior before everyone else. The same id and salt always map to
the same bucket, so the rollout is stable across requests.

## Production problem

Ten percent of accounts should use the new search backend in production.
Everyone else keeps the current backend.

The application resolves:

```text
search-backend
```

and passes a stable account id in runtime context.

## Workspace shape

```text
config/
  rototo-workspace.toml
  schemas/
    context.schema.json
  qualifiers/
    search-v2-10-percent.toml
  variables/
    search-backend.toml
```

## Runtime context

```json
{
  "account": {
    "id": "acct_123"
  }
}
```

The bucket input should be stable. Do not use request ids or session ids unless
you intentionally want behavior to vary from request to request.

## Qualifier

Create `qualifiers/search-v2-10-percent.toml`:

```toml
schema_version = 1

[qualifier]
description = "Stable 10 percent search backend rollout"

[[qualifier.predicate]]
attribute = "account.id"
op = "bucket"
salt = "search-v2-v1"
range = [0, 1000]
```

Buckets run from `0` through `9999`. The range `[0, 1000]` selects ten percent
of bucket values. Change the end of the range to increase the rollout:

```text
10 percent -> [0, 1000]
25 percent -> [0, 2500]
50 percent -> [0, 5000]
```

Keep the salt stable while expanding the range. Changing the salt reshuffles
which accounts are selected.

## Variable

Create `variables/search-backend.toml`:

```toml
schema_version = 1

[variable]
description = "Search backend"
type = "string"

[variable.values]
current = "search-v1"
next = "search-v2"

[variable.env._]
value = "current"

[variable.env.prod]
value = "current"

[[variable.env.prod.rule]]
description = "Stable account bucket uses the new search backend"
qualifier = "search-v2-10-percent"
value = "next"
```

## Verify the behavior

```sh
rototo variable resolve search-backend \
  --workspace config/ \
  --env prod \
  --context '{"account":{"id":"acct_123"}}'
```

Try several stable account ids when testing locally. Some should select `next`;
most should select `current` at ten percent.

## Tests to keep

Use fixed account ids in tests and assert their selected value keys. Keep those
fixtures stable as the rollout expands so reviewers can see which accounts are
intended to move.

## Fit

Use this for stable percentage rollout controlled from the workspace.

Do not use this when the target is a business condition such as enterprise
accounts or EU users. Use explicit predicates for those conditions.

## Related docs

- `predicate-reference`
- `how-to-select-a-value-for-a-runtime-condition`
- `how-to-investigate-why-a-value-was-selected`
