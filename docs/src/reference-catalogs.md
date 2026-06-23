# Catalogs Reference

Catalogs are for structured configuration that deserves its own schema. Use
them when the application should receive one reviewed value, not a loose group
of primitive variables.

Catalog schema files live under `catalogs/<catalog-id>.schema.json`. The name
before `.schema.json` is the catalog id. Values for that catalog live under
`catalogs/<catalog-id>-entries/*.toml`. [Variables](reference-variables.html)
then select value names from that catalog.

## Catalog File

A catalog file is the JSON Schema for a family of values. It does not hold the
values themselves.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Account limit profiles",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "enabled_features": {
      "type": "array",
      "items": { "type": "string" }
    },
    "limits": {
      "type": "object",
      "properties": {
        "projects": { "type": "integer" },
        "members": { "type": "integer" }
      },
      "required": ["projects", "members"],
      "additionalProperties": false
    }
  },
  "required": ["enabled_features", "limits"]
}
```

## Fields

Catalog files are JSON Schema documents. The schema's `description` is shown
by inspect and editor tooling when present. Rototo parses and compiles the file
as JSON Schema before validating entries.

## Value Files

Values live beside the catalog in a directory named for that catalog:

```text
catalogs/
  account-limit-profile.schema.json
  account-limit-profile-entries/
    growth.toml
    enterprise.toml
```

The value name is the file stem:

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

A variable turns catalog values into runtime configuration by
[selecting value names](reference-variable-resolution.html) with
`type = "catalog:<catalog-id>"`:

```toml
schema_version = 1
type = "catalog:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
when = 'qualifier["enterprise-account"]'
value = "enterprise"
```

Catalog-backed variables must not contain `[values]`. The selectable values
come from value files.

## Catalog References In Schemas

Sometimes one structured value needs to point at another reviewed value. JSON
Schemas may mark string fields as references to other catalog values with
`x-rototo-catalog-ref`:

```json
{
  "type": "object",
  "properties": {
    "banner_id": {
      "type": "string",
      "x-rototo-catalog-ref": "support-banner"
    }
  }
}
```

When a value contains `banner_id = "incident"`, rototo checks that:

- `catalog://support-banner` exists;
- `catalogs/support-banner-entries/incident.toml` exists.

The string may point at a path inside the target entry by appending a JSON
Pointer fragment:

```json
{
  "banner_id": "incident#/variants/eu/title"
}
```

That reference points at entry `incident` in `catalog://support-banner`, then at
`/variants/eu/title` inside the loaded entry value.

When a field may point at one of several catalogs, list the allowed catalogs:

```json
{
  "type": "string",
  "x-rototo-catalog-ref": ["email-template", "sms-template"]
}
```

Rototo searches those catalogs for the referenced entry. Zero matches are an
unknown reference. More than one match is ambiguous, so the value should use the
explicit object form:

```json
{
  "type": "object",
  "required": ["catalog", "entry"],
  "properties": {
    "catalog": { "enum": ["email-template", "sms-template"] },
    "entry": { "type": "string" },
    "pointer": { "type": "string", "format": "json-pointer" }
  },
  "additionalProperties": false,
  "x-rototo-catalog-ref": true
}
```

The corresponding value is:

```json
{
  "catalog": "sms-template",
  "entry": "payment_failed",
  "pointer": "/body"
}
```

Rototo follows catalog reference annotations through `properties`, `items`,
`prefixItems`, `allOf`, `anyOf`, `oneOf`, `$defs`, and local or package-local
`$ref` references. Package-local schemas are addressed as
`rototo://catalogs/<catalog-id>.schema.json`; rototo does not fetch schemas from
the network during lint.

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

Widgets are pre-registered; the vocabulary is part of the package format and
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
| `rototo/catalog-parse-failed` | Catalog schema JSON could not be parsed. |
| `rototo/catalog-schema-invalid` | Catalog schema JSON could not compile as JSON Schema. |
| `rototo/catalog-entry-schema-mismatch` | Entry does not match its catalog schema. |
| `rototo/catalog-entry-unknown-reference` | Entry references a missing catalog, missing entry, invalid pointer, missing pointer path, or ambiguous entry through `x-rototo-catalog-ref`. |

## What Catalogs Do Not Do

Catalogs do not provide a database or entry lifecycle. They are structured
configuration selected by variables. High-volume mutable records should stay in
application storage.
