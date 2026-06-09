# Predicate Operators Reference

Predicate operators define how a
[qualifier](reference-qualifiers.html) compares runtime context against the
policy written in the workspace. They are intentionally small, because every
operator needs clear lint behavior and clear runtime traces.

## Operators

| Operator | Predicate value | Context value | Result |
| --- | --- | --- | --- |
| `eq` | Any JSON value | Any JSON value | True when the values are equal. |
| `neq` | Any JSON value | Any JSON value | True when the values are not equal. |
| `in` | List | Any JSON value | True when the context value equals one list member. |
| `not_in` | List | Any JSON value | True when the context value equals no list member. |
| `gt` | Number | Number | True when context is greater than value. |
| `gte` | Number | Number | True when context is greater than or equal to value. |
| `lt` | Number | Number | True when context is less than value. |
| `lte` | Number | Number | True when context is less than or equal to value. |
| `bucket` | No `value` field | Scalar | True when the computed bucket is in range. |

Unknown operators are invalid.

## Equality

```toml
[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"
```

`eq` and `neq` compare JSON values. Integers and numbers compare by numeric
value when possible.

## Set Membership

```toml
[[predicate]]
attribute = "account.plan"
op = "in"
value = ["growth", "enterprise"]
```

`in` and `not_in` require the predicate `value` to be a list. The context value
is compared against each list member using equality semantics.

## Numeric Comparison

```toml
[[predicate]]
attribute = "account.seats"
op = "gte"
value = 100
```

`gt`, `gte`, `lt`, and `lte` require a numeric predicate value and numeric
runtime context. If the context schema declares the attribute as a non-numeric
type, lint reports a type mismatch before runtime.

## Bucket

```toml
[[predicate]]
attribute = "account.id"
op = "bucket"
salt = "billing-policy-2026-06"
range = [0, 2500]
```

`bucket` hashes `salt` and the canonical context value into a bucket from
`0` through `9999`. The predicate is true when the bucket is within the
half-open range `[start, end)`.

Bucket predicates must contain:

- `attribute`;
- `op = "bucket"`;
- `salt`;
- `range = [start, end]`.

Bucket predicates must not contain `value`.

The range must satisfy:

```text
0 <= start < end <= 10000
```

## Context Schema Compatibility

When [`schemas/context.schema.json`](reference-context.html) exists, lint checks
operator compatibility:

- `eq` and `neq` values must match the declared context attribute type;
- `in` and `not_in` list members must match the declared type;
- numeric operators require numeric context attributes;
- `bucket` requires a scalar context attribute: boolean, integer, number, or string.

The runtime context is also validated against the context schema before
resolution unless [SDK options](reference-sdk-resolution.html) disable context
validation.

## Qualifier Attributes

For `attribute = "qualifier.<id>"`, the actual value is the referenced
qualifier's boolean result. The usual comparison operators apply:

```toml
[[predicate]]
attribute = "qualifier.paid-account"
op = "eq"
value = true
```

`bucket` is not useful for qualifier references, because qualifier references
produce booleans and are meant to compose named conditions.
