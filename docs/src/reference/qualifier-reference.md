# Qualifier File Reference

A qualifier is a named condition over runtime context. Variables use qualifiers
inside rules to select values.

## Location and ID

Qualifier files live under the workspace `qualifiers/` directory:

```text
qualifiers/<qualifier-id>.toml
```

The file stem is the qualifier id:

```text
qualifiers/enterprise-accounts.toml -> enterprise-accounts
```

## Minimal Shape

```toml
schema_version = 1

description = "Accounts on the enterprise plan with at least 100 seats"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"

[[predicate]]
attribute = "account.seats"
op = "gte"
value = 100
```

All predicates in a qualifier are ANDed. If any predicate is false, the
qualifier resolves to `false`.

## `schema_version`

Required. Must be:

```toml
schema_version = 1
```

Unsupported or missing schema versions fail lint.

## `description`

Optional but recommended. Use it to explain the named condition in application
or product language.

```toml
description = "Premium users in Germany"
```

## `[[predicate]]`

Required. A qualifier must have at least one predicate.

```toml
[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"
```

Predicate fields:

- `attribute`: required. Dot-separated context path, or `qualifier.<id>` to
  reference another qualifier.
- `op`: required. Predicate operator.
- `value`: required for every operator except `bucket`.
- `salt`: required for `bucket`.
- `range`: required for `bucket`.

See `predicate-reference` for operator semantics.

## Qualifier References

A predicate can reference another qualifier:

```toml
[[predicate]]
attribute = "qualifier.enterprise-accounts"
op = "eq"
value = true
```

Referenced qualifiers must exist. Cycles fail at resolution time.

## Missing Context

If a predicate reads a context path that is missing from the runtime context,
the predicate resolves to `false`.

If the workspace has a context schema, lint also checks that qualifier context
attributes are declared by that schema. Attributes beginning with `qualifier.`
are qualifier references and are not checked against the context schema.

## Complete Example

```toml
schema_version = 1

description = "Enterprise accounts in Germany"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"

[[predicate]]
attribute = "account.seats"
op = "gte"
value = 100

[[predicate]]
attribute = "request.country"
op = "eq"
value = "DE"
```

## Validation

Qualifier lint checks:

- `schema_version = 1` exists.
- At least one `[[predicate]]` exists.
- Each predicate is a table.
- Each predicate has `attribute` and `op`.
- Operators are known.
- Non-`bucket` predicates contain `value`.
- `in` and `not_in` values are lists.
- Numeric comparison values are numbers.
- `bucket` predicates contain `salt` and `range`.
- `bucket` ranges satisfy `0 <= start < end <= 10000`.
- `bucket` predicates do not contain `value`.
- `qualifier.<id>` references point at known qualifiers.
- Context attributes are declared by the context schema when one is present.
