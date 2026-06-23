# Qualifiers Reference

Qualifiers turn request-time facts into named runtime conditions. Variables
refer to those names instead of repeating condition logic in every resolve rule.

Qualifier files live under `qualifiers/*.toml`. The file stem is the qualifier
id.

## Minimal Qualifier

```toml
schema_version = 1

description = "Paid accounts"

when = 'context.account.plan in ["growth", "enterprise"]'
```

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Qualifier format version. The only supported value is `1`. |
| `description` | No | string | Human description shown by inspect and editor tooling. |
| `when` | Yes | string | Expression that must evaluate to `true` for the qualifier to match. |

`when` uses rototo's expression profile. It can read `context`, call supported helper
functions, and reference other qualifiers with `qualifier["id"]`.

## Condition Shape

Most comparisons read a request context path:

```toml
when = 'context.account.plan == "enterprise"'
```

Presence checks use `has(...)`:

```toml
when = '!has(context.account.contract_end)'
```

Bucket checks call `bucket(value, salt, start, end)`:

```toml
when = 'bucket(context.account.id, "account-limit-policy-2026-06", 0, 1000)'
```

Bucket ranges are half-open: `[start, end)`. The allowed bucket space is
`0..10000`.

Use `!` to invert any condition:

```toml
when = '!suffix(context.user.email, "@example.com")'
```

## Runtime Context Attributes

Most qualifier expressions read paths from the runtime context supplied by the
application:

```toml
when = 'context.account.plan == "enterprise"'
```

During resolution, rototo reads `context.account.plan`. Missing paths fail
resolution.

Lint checks that every context attribute is satisfied by at least one
[request context schema](reference-context.html).

## Qualifier References

A qualifier can read another qualifier:

```toml
when = 'qualifier["paid-account"]'
```

This composes named conditions. The referenced qualifier must exist. Rototo
rejects cycles during resolution.

The top-level context field `qualifier` is reserved for these references. A
context schema that declares `qualifier` as an application-provided field is
invalid.

## Boolean Semantics

Use `&&`, `||`, and `!` for composition. Rototo short-circuits boolean
operators during resolution, so `false && context.missing.path == "x"` does not
read the missing path.

Rototo records the `when` expression and final boolean value in qualifier
traces. Missing paths fail resolution unless guarded with `has(...)` or boolean
short-circuiting.

## Related Pages

See [Expressions](reference-predicate-operators.html) for expression helper
semantics and [Qualifier Resolution](reference-qualifier-resolution.html) for
trace behavior.
