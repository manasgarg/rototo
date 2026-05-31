# How to Add a New Context Field

Use this when a config rule needs a runtime fact the application does not
currently send, such as account region, tenant tier, operation name, rollout
bucket input, or request country.

This is a contract change between the application and the workspace. The
workspace should not start depending on a new context path until the context
schema, application payload, tests, and representative resolutions agree on
that field.

## Expected outcome

After this change:

- The context schema declares the new field.
- Application code sends the field during resolution.
- Qualifiers can reference the field without silent misspellings.
- Lint and representative resolution prove the new field works.

## Update the context schema

Find the schema declared by the workspace manifest:

```toml
[context]
schema = "schemas/context.schema.json"
```

Add the new field to that JSON Schema. For example, to let rules read
`request.country`:

```json
{
  "type": "object",
  "required": ["account", "request"],
  "additionalProperties": false,
  "properties": {
    "account": {
      "type": "object",
      "required": ["plan"],
      "additionalProperties": true,
      "properties": {
        "plan": { "type": "string" }
      }
    },
    "request": {
      "type": "object",
      "required": ["country"],
      "additionalProperties": true,
      "properties": {
        "country": { "type": "string" }
      }
    }
  }
}
```

Make the field required only when every caller can send it. If the application
will roll out the field gradually, make it optional first and add rules that
handle the missing-field behavior intentionally.

## Update application resolution input

Update the application code that builds runtime context so it sends the new
field:

```json
{
  "account": {
    "plan": "enterprise"
  },
  "request": {
    "country": "DE"
  }
}
```

The SDK context must be a JSON object. Keep this object focused on facts the
workspace is allowed to use for runtime decisions.

## Use the field in a qualifier

After the schema and application payload are aligned, reference the field from a
qualifier:

```toml
schema_version = 1

description = "Requests from Germany"

[[predicate]]
attribute = "request.country"
op = "eq"
value = "DE"
```

With a context schema in place, lint can catch qualifiers that reference paths
the schema does not declare.

## Verify the change

Lint the workspace:

```sh
rototo lint config/
```

Resolve with context that includes the new field:

```sh
rototo resolve config/ --qualifier germany-requests \
  --context '{"account":{"plan":"enterprise"},"request":{"country":"DE"}}'
```

If the field is required, also verify that missing context fails before rule
evaluation:

```sh
rototo resolve config/ --qualifier germany-requests \
  --context '{"account":{"plan":"enterprise"}}'
```

## Common mistakes

Do not add a predicate before updating the context schema. Without a schema, a
misspelled or missing path can quietly resolve as `false`.

Do not make a new field required before all application callers can send it.
That turns a config release into a runtime integration break.

Do not put derived policy decisions in context. Send facts, such as
`request.country`; let the workspace define the decision, such as
`germany-requests`.

## Related docs

- `context-reference` explains context input and schema validation.
- `predicate-reference` specifies context path predicates.
- `qualifier-reference` explains missing context behavior.
