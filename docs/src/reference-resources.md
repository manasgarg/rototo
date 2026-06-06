# Resources Reference

Resources are for structured configuration that deserves its own schema. Use
them when the application should receive one reviewed object, not a loose group
of primitive variables.

Resource files live under `resources/*.toml`. The file stem is the resource id.
Objects for that resource live under `resources/<resource-id>-objects/*.toml`.
Variables then select object keys from that resource.

## Resource File

A resource file declares the schema for a family of objects. It does not hold
the objects themselves.

```toml
schema_version = 1

description = "Account limit profiles"
schema = "../schemas/account-limit-profile.schema.json"
```

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Resource format version. The only supported value is `1`. |
| `description` | No | string | Human description shown by inspect and editor tooling. |
| `schema` | Yes | string | Workspace-relative path to a JSON Schema file. |

The `schema` path is resolved relative to the resource file. It must stay
inside the workspace and must point to a discovered schema document under
`schemas/`.

## Object Files

Objects live beside the resource in a directory named for that resource:

```text
resources/
  account-limit-profile.toml
  account-limit-profile-objects/
    growth.toml
    enterprise.toml
```

The object key is the file stem:

```text
growth.toml     -> growth
enterprise.toml -> enterprise
```

The whole TOML document is converted to JSON and validated against the resource
schema before an app can receive it through a variable.

```toml
enabled_features = ["audit-log"]

[limits]
projects = 100
members = 250
```

## Variable Integration

A variable turns resource objects into runtime configuration by selecting
object keys with `type = "resource:<resource-id>"`:

```toml
schema_version = 1
type = "resource:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
qualifier = "enterprise-account"
value = "enterprise"
```

Resource-backed variables must not contain `[values]`. The selectable values
come from object files.

## Resource References In Schemas

Sometimes one structured object needs to point at another reviewed object. JSON
Schemas may mark string fields as references to other resource objects with
`x-rototo-resource`:

```json
{
  "type": "object",
  "properties": {
    "banner_id": {
      "type": "string",
      "x-rototo-resource": "support-banner"
    }
  }
}
```

When an object contains `banner_id = "incident"`, rototo checks that:

- `resource://support-banner` exists;
- `resources/support-banner-objects/incident.toml` exists.

Rototo follows `x-rototo-resource` through `properties`, `items`, `allOf`,
`anyOf`, and `oneOf`.

## Validation Errors

Common resource diagnostics include:

| Rule | Meaning |
| --- | --- |
| `rototo/resource-schema-version` | Resource file does not declare `schema_version = 1`. |
| `rototo/resource-schema-ref` | `schema` is missing, invalid, or points outside the workspace contract. |
| `rototo/resource-object-schema-mismatch` | Object does not match its resource schema. |
| `rototo/resource-object-unknown-reference` | Object references a missing resource or object through `x-rototo-resource`. |

## What Resources Do Not Do

Resources do not provide a database or object lifecycle. They are structured
configuration selected by variables. High-volume mutable records should stay in
application storage.
