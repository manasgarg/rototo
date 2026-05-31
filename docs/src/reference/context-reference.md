# Context Reference

Runtime context is the JSON object supplied when resolving qualifiers and
variables. It contains request-time facts such as account plan, seats, country,
tenant, user, operation, or rollout bucket.

## Context Object

Resolution context must be a JSON object.

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 250
  },
  "request": {
    "country": "DE"
  }
}
```

Qualifiers read values from this object through dot-separated paths:

```toml
attribute = "account.plan"
```

## CLI Context Inputs

Resolve commands accept one or more `--context` values. Later values override
earlier values.

### Inline JSON

```sh
rototo resolve --variable llm-agent-config \
  --env prod \
  --context '{"account":{"plan":"enterprise","seats":250}}'
```

Inline JSON must be an object.

### JSON File

Prefix a path with `@`:

```sh
rototo resolve --variable llm-agent-config \
  --env prod \
  --context @context/prod-enterprise.json
```

The file must contain a JSON object.

### Path Assignment

Use `path=value` for small overrides:

```sh
rototo resolve --variable llm-agent-config \
  --env prod \
  --context account.plan=enterprise \
  --context account.seats=250
```

Assignment paths are split on `.` and create nested objects:

```text
account.plan=enterprise
```

becomes:

```json
{
  "account": {
    "plan": "enterprise"
  }
}
```

Assignment values are parsed as JSON when possible. Otherwise they are strings:

```text
account.seats=250       -> number 250
account.active=true     -> boolean true
account.plan=enterprise -> string "enterprise"
```

## Merge Behavior

Multiple context inputs are merged left to right.

Objects merge recursively:

```sh
--context '{"account":{"plan":"enterprise"}}' \
--context '{"account":{"seats":250}}'
```

produces:

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 250
  }
}
```

When the same key appears more than once, the later value wins.

## Context Schema

A workspace can declare a context schema in `rototo-workspace.toml`:

```toml
[context]
schema = "schemas/context.schema.json"
```

When present, rototo validates runtime context against that JSON Schema before
resolution continues. Invalid context fails before qualifiers and variable rules
are evaluated.

The schema also lets lint catch qualifier predicates that reference undeclared
context paths.

## Missing Attributes

If a predicate reads a context path that is missing, the predicate resolves to
`false`.

Use a context schema when missing attributes should be rejected instead of
falling through to a default branch.

## SDK Context

The Rust SDK accepts context through `ResolveContext`. SDK context must be a
JSON object. The CLI conveniences for `@file` and `path=value` are CLI-only.
