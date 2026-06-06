# Qualifiers Reference

Qualifiers turn request-time facts into named runtime conditions. Variables
refer to those names instead of repeating predicate logic in every resolve
rule.

Qualifier files live under `qualifiers/*.toml`. The file stem is the qualifier
id.

## Minimal Qualifier

```toml
schema_version = 1

description = "Paid accounts"

[[predicate]]
attribute = "account.plan"
op = "in"
value = ["growth", "enterprise"]
```

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Qualifier format version. The only supported value is `1`. |
| `description` | No | string | Human description shown by inspect and editor tooling. |
| `predicate` | Yes | array of tables | Predicates that must all match. |

`[[predicate]]` must be an array of tables. A qualifier with no predicates is
invalid.

## Predicate Shape

Comparison predicates use `attribute`, `op`, and `value`:

```toml
[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"
```

Bucket predicates use `attribute`, `op`, `salt`, and `range`:

```toml
[[predicate]]
attribute = "account.id"
op = "bucket"
salt = "account-limit-policy-2026-06"
range = [0, 1000]
```

Bucket ranges are half-open: `[start, end)`. The allowed bucket space is
`0..10000`.

## Runtime Context Attributes

Most predicates read paths from the runtime context supplied by the
application:

```toml
attribute = "account.plan"
```

During resolution, rototo reads `context.account.plan`. Missing paths fail
resolution.

If `schemas/context.schema.json` exists, lint checks that context attributes
are declared in that schema and that predicate values are compatible with the
declared types.

## Qualifier References

A predicate can read another qualifier:

```toml
[[predicate]]
attribute = "qualifier.paid-account"
op = "eq"
value = true
```

This composes named conditions. The referenced qualifier must exist. Rototo
rejects cycles during resolution.

The top-level context field `qualifier` is reserved for these references. A
context schema that declares `qualifier` as an application-provided field is
invalid.

## AND Semantics

All predicates in a qualifier are ANDed. The qualifier resolves to `true` only
when every predicate resolves to `true`.

Rototo records predicate traces in order. If a predicate is false, the
qualifier is false. That trace is the debugging surface for why a rule did or
did not match.

## Duplicate Predicates

Rototo reports duplicate predicates with
`rototo/qualifier-predicate-duplicate`. A duplicate usually means a condition
was copied without changing the attribute, operator, or value.

## Related Pages

See `reference-predicate-operators` for operator semantics and
`reference-qualifier-resolution` for trace behavior.
