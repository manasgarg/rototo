# Request Context Reference

Resolution depends on facts the application knows at request time: account
plan, service lane, country, stable account id, or other runtime state. Rototo
calls each JSON object a resolve context. A workspace can describe the allowed
shapes for those objects with request context schemas.

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

If no context is passed to `rototo resolve`, rototo resolves selected targets
against stored sample entries from compatible request contexts. If a target has
no compatible stored sample, resolution reports that gap instead of guessing.

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

## Request Context Schemas

Request context schemas live under `request-contexts/`:

```text
request-contexts/
  request.schema.json
  startup.schema.json
```

The id comes from the filename before `.schema.json`. The example above defines
`request` and `startup` request contexts.

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

Each schema has three jobs:

- lint verifies that each qualifier has at least one compatible request
  context;
- variable compatibility is inferred from the qualifiers used by its rules;
- runtime resolution validates the context object against a compatible request
  context before evaluating qualifiers.

When a supplied context validates against more than one compatible schema,
rototo may use any matching schema. Resolution fails only when none of the
target's compatible request contexts validate the supplied object.

## Sample Entries

Sample entries for a request context live in a sibling entries directory:

```text
request-contexts/
  request.schema.json
  request-entries/
    premium-enterprise.json
    free-us.json
```

Each entry must be a JSON object that validates against its schema. The CLI and
console use these entries for inspect and resolve screens when no explicit
context is supplied.

SDK callers can disable runtime context validation with
[`ResolveOptions`](reference-sdk-resolution.html), but that should be a
deliberate app boundary decision.

## Reserved Field

The top-level field `qualifier` is reserved. Rototo uses
`qualifier.<id>` attributes to
[reference other qualifiers](reference-qualifiers.html) during condition
evaluation.

Do not declare `qualifier` in a request context schema as an
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
