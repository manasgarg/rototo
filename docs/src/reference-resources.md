# Resources Reference

Resources are for structured configuration that deserves its own schema. Use
them when the application should receive one reviewed object, not a loose group
of primitive variables.

Resource files live under `resources/*.toml`. The file stem is the resource id.
Objects for that resource live under `resources/<resource-id>-objects/*.toml`.
[Variables](reference-variables.html) then select object keys from that
resource.

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

A variable turns resource objects into runtime configuration by
[selecting object keys](reference-variable-resolution.html) with
`type = "resource:<resource-id>"`:

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

## Editor Hints In Schemas

A schema property describes what a value must be; it cannot say how a person
should pick one. A rollout percentage edits better on a slider than in a text
box, and a hex color is easier to review with a swatch next to it. Schemas may
attach an editor hint to a property with `x-rototo-ui`:

```json
{
  "type": "object",
  "properties": {
    "rollout_percent": {
      "type": "integer",
      "x-rototo-ui": { "widget": "slider", "min": 0, "max": 100, "step": 5 }
    },
    "accent": {
      "type": "string",
      "x-rototo-ui": { "widget": "color" }
    },
    "body": {
      "type": "string",
      "x-rototo-ui": { "widget": "textarea", "rows": 4 }
    }
  }
}
```

Widgets are pre-registered; the vocabulary is part of the workspace format and
editor tooling renders only widgets it knows:

| Widget | Property types | Parameters |
| --- | --- | --- |
| `checkbox` | `boolean` | none |
| `code` | `string` | `language`, `rows` |
| `color` | `string` | none |
| `date` | `string` | none |
| `datetime` | `string` | none |
| `email` | `string` | none |
| `markdown` | `string` | `rows` |
| `multiselect` | `array` | none. Requires an `enum` on `items`. |
| `number` | `integer`, `number` | `min`, `max`, `step` |
| `percent` | `integer`, `number` | `min`, `max`, `step`. Defaults to 0–100. |
| `radio` | `string` | none. Requires an `enum` on the property. |
| `slider` | `integer`, `number` | `min`, `max`, `step`. Bounds may also come from the schema's `minimum` and `maximum`; a slider must have bounds from one of the two. |
| `tags` | `array` | none |
| `textarea` | `string` | `rows` |
| `time` | `string` | none |
| `toggle` | `boolean` | none |
| `url` | `string` | none |

Without a hint, editor tooling still maps a few standard `format` values to
widgets: `color`, `date`, `date-time`, `email`, `time`, and `uri`.

Hints never change validation or resolution. A wrong hint degrades to the
default editor control, and lint reports it:

| Rule | Meaning |
| --- | --- |
| `rototo/schema-ui-unknown-widget` | `x-rototo-ui` names a widget outside the vocabulary. |
| `rototo/schema-ui-widget-type-mismatch` | The widget does not support the property's declared type. |
| `rototo/schema-ui-widget-params` | The hint is malformed: missing widget string, unknown or mistyped parameters, a slider without bounds, or a radio or multiselect without an enum. |

These diagnostics are warnings: the configuration still resolves correctly, so
they surface in review without blocking a publish.

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
