# Variable Values Reference

Variables matter only if the selected value has a shape the application can
trust. Rototo validates primitive values directly and validates structured
entries through catalogs.

That split keeps the app boundary clear: primitive values stay inline, while
structured entries get schemas and entry files.
[Variable Resolution](reference-variable-resolution.html) covers resolution
order.

## Primitive Values

Primitive variables store values inline under `[values]`:

```toml
schema_version = 1
type = "int"

[values]
standard = 3
expanded = 25

[resolve]
default = "standard"
```

The table key is the value key. The TOML value is converted to JSON and
returned as the selected value.

## Primitive Type Rules

Primitive types are intentionally narrow. If the value needs object structure,
move it to a catalog instead of trying to encode an object as a primitive
value.

| Type | Accepted JSON shape |
| --- | --- |
| `bool` | Boolean |
| `int` | Integer number |
| `number` | Any JSON number |
| `string` | String |
| `list` | Array |

Examples:

```toml
type = "bool"

[values]
off = false
on = true
```

```toml
type = "list"

[values]
standard = ["email"]
expanded = ["email", "sms"]
```

Objects are not accepted as primitive values. Use a
[catalog-backed variable](reference-catalogs.html) when the value needs
object structure.

## Catalog Entry Values

Structured values live as catalog entries so rototo can validate the shape
before the application receives the selected payload:

```text
catalogs/
  account-limit-profile.toml
  account-limit-profile-entries/
    growth.toml
    enterprise.toml
```

The catalog file declares the schema:

```toml
schema_version = 1
description = "Account limit profiles"
schema = "../schemas/account-limit-profile.schema.json"
```

Each entry file contains the payload:

```toml
enabled_features = ["audit-log"]

[limits]
projects = 100
members = 250
```

The variable then selects entry keys:

```toml
type = "catalog:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
qualifier = "enterprise-account"
value = "enterprise"
```

## Validation

Validation is part of the release boundary. Primitive values are checked
against the declared primitive type during lint.

Catalog entries are checked against the catalog's JSON Schema during lint.
If an entry does not match its schema, rototo reports
`rototo/catalog-entry-schema-mismatch`.

If a variable default or rule references a missing value, rototo reports
`rototo/variable-unknown-value`.

## Value Keys

Value keys are operational names. They appear in:

- variable files;
- catalog entry filenames;
- CLI and SDK resolution results;
- generated fixtures;
- logs and metrics you build from resolution traces.

Keep keys stable once application tests, fixtures, dashboards, or runbooks
refer to them.

## What Values Do Not Store

Rototo values are configuration, not mutable application state. Do not store
user records, transactions, counters, queues, or analytics events as rototo
values. Put those in the systems that already own their consistency and write
patterns.
