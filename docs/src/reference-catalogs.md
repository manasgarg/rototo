# Catalogs Reference

Catalogs are for structured configuration that deserves its own schema. Use
them when the application should receive one reviewed entry, not a loose group
of primitive variables.

Catalog files live under `catalogs/*.toml`. The file stem is the catalog id.
Entries for that catalog live under `catalogs/<catalog-id>-entries/*.toml`.
[Variables](reference-variables.html) then select entry keys from that
catalog.

## Catalog File

A catalog file declares the schema for a family of entries. It does not hold
the entries themselves.

```toml
schema_version = 1

description = "Account limit profiles"
schema = "../schemas/account-limit-profile.schema.json"
```

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Catalog format version. The only supported value is `1`. |
| `description` | No | string | Human description shown by inspect and editor tooling. |
| `schema` | Yes | string | Workspace-relative path to a JSON Schema file. |

The `schema` path is resolved relative to the catalog file. It must stay
inside the workspace and must point to a discovered schema document under
`schemas/`.

## Entry Files

Entries live beside the catalog in a directory named for that catalog:

```text
catalogs/
  account-limit-profile.toml
  account-limit-profile-entries/
    growth.toml
    enterprise.toml
```

The entry key is the file stem:

```text
growth.toml     -> growth
enterprise.toml -> enterprise
```

The whole TOML document is converted to JSON and validated against the catalog
schema before an app can receive it through a variable.

```toml
enabled_features = ["audit-log"]

[limits]
projects = 100
members = 250
```

## Variable Integration

A variable turns catalog entries into runtime configuration by
[selecting entry keys](reference-variable-resolution.html) with
`type = "catalog:<catalog-id>"`:

```toml
schema_version = 1
type = "catalog:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
qualifier = "enterprise-account"
value = "enterprise"
```

Catalog-backed variables must not contain `[values]`. The selectable values
come from entry files.

## Catalog References In Schemas

Sometimes one structured entry needs to point at another reviewed entry. JSON
Schemas may mark string fields as references to other catalog entries with
`x-rototo-catalog`:

```json
{
  "type": "object",
  "properties": {
    "banner_id": {
      "type": "string",
      "x-rototo-catalog": "support-banner"
    }
  }
}
```

When an entry contains `banner_id = "incident"`, rototo checks that:

- `catalog://support-banner` exists;
- `catalogs/support-banner-entries/incident.toml` exists.

Rototo follows `x-rototo-catalog` through `properties`, `items`, `allOf`,
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

Common catalog diagnostics include:

| Rule | Meaning |
| --- | --- |
| `rototo/catalog-schema-version` | Catalog file does not declare `schema_version = 1`. |
| `rototo/catalog-schema-ref` | `schema` is missing, invalid, or points outside the workspace contract. |
| `rototo/catalog-entry-schema-mismatch` | Entry does not match its catalog schema. |
| `rototo/catalog-entry-unknown-reference` | Entry references a missing catalog or entry through `x-rototo-catalog`. |

## What Catalogs Do Not Do

Catalogs do not provide a database or entry lifecycle. They are structured
configuration selected by variables. High-volume mutable records should stay in
application storage.
