# Resolve Context Reference

Resolution depends on facts the application knows at request time: account
plan, service lane, country, stable account id, or other runtime state. Rototo
calls those facts the resolve context.

The context is always a JSON object.

## CLI Context

The [CLI](reference-cli-overview.html) accepts repeatable `--context` values:

```sh
rototo resolve account-config \
  --variable account-limits \
  --context account.plan=enterprise \
  --context '{"request":{"country":"DE"}}'
```

Each `--context` value can be:

| Form | Example | Meaning |
| --- | --- | --- |
| JSON object | `'{"account":{"plan":"enterprise"}}'` | Parsed directly. |
| File | `@context.json` | Reads a JSON object from a file. |
| Assignment | `account.plan=enterprise` | Creates a nested object path. |

Assignments parse the right-hand side as JSON when possible. If parsing fails,
the value is treated as a string.

```sh
--context account.seats=42      # number
--context account.enabled=true  # boolean
--context account.plan=growth   # string
```

Repeatable context inputs merge left to right. Nested objects are merged.
Later scalar or array values replace earlier values at the same path.

If no context is passed to `rototo resolve`, rototo uses `{}`.

## SDK Context

The [SDK](reference-sdk-resolution.html) uses a JSON object context:

:::sdk-snippet reference-context-sdk
```rust
use rototo::ResolveContext;

let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "plan": "enterprise"
    }
}))?;
```

```python
context = {
    "account": {
        "plan": "enterprise",
    },
}
```

```typescript
const context = {
  account: {
    plan: "enterprise",
  },
};
```

```java
Map<String, Object> context = Map.of(
    "account",
    Map.of("plan", "enterprise")
);
```

```go
resolveContext := map[string]any{
    "account": map[string]any{
        "plan": "enterprise",
    },
}
```
:::

SDK resolution rejects non-object JSON context.

## Context Schema

When a workspace contains `schemas/context.schema.json`, rototo treats it as
the schema for resolve context.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["account"],
  "properties": {
    "account": {
      "type": "object",
      "required": ["plan"],
      "properties": {
        "plan": { "type": "string" },
        "seats": { "type": "integer" }
      }
    }
  }
}
```

The schema has two jobs:

- lint verifies qualifier attribute paths and operator compatibility;
- runtime resolution validates the context object before evaluating
  qualifiers.

SDK callers can disable runtime context validation with
[`ResolveOptions`](reference-sdk-resolution.html), but that should be a
deliberate app boundary decision.

## Reserved Field

The top-level field `qualifier` is reserved. Rototo uses
`qualifier.<id>` attributes to
[reference other qualifiers](reference-qualifiers.html) during predicate
evaluation.

Do not declare `qualifier` in `schemas/context.schema.json` as an
application-owned field.

## Missing Context

If a qualifier reads a context path that is missing at runtime, resolution
fails:

```text
missing resolve context attribute: account.plan required by qualifier://paid-account
```

Use context schemas and
[app tests](testing-runtime-configuration.html) to catch those failures before
the service depends on the workspace.
