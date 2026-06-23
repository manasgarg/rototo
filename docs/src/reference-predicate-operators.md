# Expressions Reference

Qualifier `when` expressions use rototo's expression profile. The expression must
evaluate to a boolean. It can read request context through `context`, compose
other qualifiers through `qualifier["id"]`, and call the helper functions below.

## Core Syntax

```toml
schema_version = 1
description = "Enterprise accounts in the European request path"

when = 'context.account.plan == "enterprise" && context.request.region in ["DE", "FR", "NL"]'
```

Supported operators:

| Syntax | Meaning |
| --- | --- |
| `==`, `!=` | JSON equality and inequality. |
| `<`, `<=`, `>`, `>=` | Numeric or string ordering. |
| `value in [a, b]` | List membership. |
| `&&`, `||`, `!` | Boolean composition with short-circuiting. |
| `has(context.path)` | True when a path is present, including `null`. |

Missing paths fail resolution unless guarded with `has(...)` or skipped by
boolean short-circuiting.

## Strings And Patterns

```toml
when = 'suffix(context.user.email, "@example.com")'
```

| Function | Meaning |
| --- | --- |
| `prefix(value, text)` | True when `value` starts with `text`. |
| `suffix(value, text)` | True when `value` ends with `text`. |
| `contains(value, text)` | True when a string contains text, or an array contains a value. |
| `regex(value, pattern)` | Rust regex match. |
| `glob(value, pattern)` | Whole-string glob match. |

String helpers are case-sensitive and do not normalize Unicode.

## SemVer

```toml
when = 'semver(context.app.version, ">=1.4.0, <2.0.0")'
```

`semver(value, requirement)` parses `value` as a SemVer version and
`requirement` as a SemVer requirement.

## Time

```toml
when = 'time_between(context.request.time, "2026-07-01T00:00:00Z", "2026-08-01T00:00:00Z")'
```

| Function | Meaning |
| --- | --- |
| `time_after(value, instant)` | True when `value` is after `instant`. |
| `time_at_or_after(value, instant)` | True when `value` is at or after `instant`. |
| `time_before(value, instant)` | True when `value` is before `instant`. |
| `time_at_or_before(value, instant)` | True when `value` is at or before `instant`. |
| `time_between(value, start, end)` | True for the half-open instant range `[start, end)`. |

Time helpers compare RFC3339 timestamps as instants. They do not read the
system clock; applications should pass the request or decision time in context.

## Presence And Null

```toml
when = '!has(context.account.contract_end)'
```

`has(context.path)` checks presence without evaluating the path as a value. For
explicit null handling, combine `has(...)` with equality:

```toml
when = 'has(context.user.nickname) && context.user.nickname == null'
```

## Arrays

```toml
when = 'contains(context.user.roles, "admin") && contains(context.user.roles, "billing")'
```

Use `contains(array, value)` for array membership. Compose calls with `&&` or
`||` for all/any behavior.

## CIDR

```toml
when = 'cidr(context.request.ip, ["10.0.0.0/8", "fd00::/8"])'
```

`cidr(ip, block_or_blocks)` parses the context value as an IPv4 or IPv6 address.
A bare IP block is treated as an exact host match.

## Bucket

```toml
when = 'bucket(context.account.id, "billing-policy-2026-06", 0, 2500)'
```

`bucket(value, salt, start, end)` hashes the salt and canonical context value
into a bucket from `0` through `9999`. The condition is true when the bucket is
inside the half-open range `[start, end)`.

The range must satisfy:

```text
0 <= start < end <= 10000
```

## Qualifier References

```toml
when = 'qualifier["paid-account"] && context.request.region == "eu"'
```

Qualifier references produce booleans. The referenced qualifier must exist, and
rototo rejects reference cycles during resolution.
