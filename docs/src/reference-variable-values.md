# Variable Values Reference

Variables matter only if the selected value has a shape the application can
trust. Rototo validates primitive values directly and validates structured
objects through resources.

That split keeps the app boundary clear: primitive values stay inline, while
structured objects get schemas and object files. `reference-variable-resolution`
covers resolution order.

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
move it to a resource instead of trying to encode an object as a primitive
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

Objects are not accepted as primitive values. Use a resource-backed variable
when the value needs object structure.

## Resource Object Values

Structured values live as resource objects so rototo can validate the shape
before the application receives the selected payload:

```text
resources/
  account-limit-profile.toml
  account-limit-profile-objects/
    growth.toml
    enterprise.toml
```

The resource file declares the schema:

```toml
schema_version = 1
description = "Account limit profiles"
schema = "../schemas/account-limit-profile.schema.json"
```

Each object file contains the object payload:

```toml
enabled_features = ["audit-log"]

[limits]
projects = 100
members = 250
```

The variable then selects object keys:

```toml
type = "resource:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
qualifier = "enterprise-account"
value = "enterprise"
```

## Validation

Validation is part of the release boundary. Primitive values are checked
against the declared primitive type during lint.

Resource objects are checked against the resource's JSON Schema during lint.
If an object does not match its schema, rototo reports
`rototo/resource-object-schema-mismatch`.

If a variable default or rule references a missing value, rototo reports
`rototo/variable-unknown-value`.

## Value Keys

Value keys are operational names. They appear in:

- variable files;
- resource object filenames;
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
