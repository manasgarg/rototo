# Predicate Reference

Predicates are the boolean tests inside qualifier files. A qualifier resolves to
`true` only when all of its predicates resolve to `true`.

## Shape

```toml
[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"
```

The `attribute` field reads either:

- a dot-separated path from runtime context, such as `account.plan`; or
- another qualifier through `qualifier.<id>`.

If the context path is missing, the predicate is `false`.

## Operators

### `eq`

True when the actual value equals `value`.

```toml
attribute = "account.plan"
op = "eq"
value = "enterprise"
```

### `neq`

True when the actual value does not equal `value`.

```toml
attribute = "account.plan"
op = "neq"
value = "free"
```

### `in`

True when the actual value is equal to one element in the `value` list.

```toml
attribute = "request.country"
op = "in"
value = ["DE", "FR", "NL"]
```

The `value` field must be a list.

### `not_in`

True when the actual value is not equal to any element in the `value` list.

```toml
attribute = "request.country"
op = "not_in"
value = ["US", "CA"]
```

The `value` field must be a list.

### `gt`, `gte`, `lt`, `lte`

Numeric comparisons.

```toml
attribute = "account.seats"
op = "gte"
value = 100
```

Both the actual context value and expected `value` must be numeric. If the
actual value is missing or not numeric, the predicate is `false`.

### `bucket`

Deterministic rollout bucketing.

```toml
attribute = "account.id"
op = "bucket"
salt = "search-v2-v1"
range = [0, 1000]
```

The actual context value is hashed with `salt` into a bucket from `0` through
`9999`. The predicate is true when the bucket is inside the half-open range:

```text
start <= bucket < end
```

Bucket rules:

- `salt` is required and must be a string.
- `range` is required and must contain two integers.
- The range must satisfy `0 <= start < end <= 10000`.
- `value` must not be present.

## Qualifier References

Predicates can read another qualifier by using `qualifier.<id>` as the
attribute:

```toml
[[predicate]]
attribute = "qualifier.enterprise-accounts"
op = "eq"
value = true
```

This lets a workspace compose named conditions. Referenced qualifiers must
exist. Cycles are rejected during resolution.

## Type Behavior

Predicate comparison uses JSON-style values after TOML values are converted to
JSON.

- `eq` and `neq` compare full values.
- `in` and `not_in` compare the actual value against elements in the expected
  list.
- Numeric comparisons use numeric conversion.
- Missing context paths resolve to `false`.

## Context Schema Interaction

When the workspace manifest declares a context schema, lint checks that context
attributes used by predicates are declared by the schema.

Attributes starting with `qualifier.` are qualifier references, not context
paths, and are not checked against the context schema.
