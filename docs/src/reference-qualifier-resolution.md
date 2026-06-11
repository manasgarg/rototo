# Qualifier Resolution Reference

A qualifier answers one runtime question: does this named condition match the
context the application supplied?

Rototo evaluates qualifiers during variable resolution, and the CLI can resolve
qualifiers directly for debugging.

## Input

Qualifier resolution needs:

- a [loaded, lint-clean workspace](reference-sdk-loading.html);
- a qualifier id;
- a [JSON object context](reference-context.html).

CLI:

```sh
rototo resolve account-config \
  --qualifier paid-account \
  --context account.plan=enterprise
```

SDK:

:::sdk-snippet qualifier-resolution-sdk
```rust
let result = workspace
    .resolve_qualifier("paid-account", &context)
    .await?;
```

```python
result = await workspace.resolve_qualifier("paid-account", context)
```

```typescript
const result = await workspace.resolveQualifier("paid-account", context);
```

```java
QualifierResolution result = workspace
    .resolveQualifier("paid-account", context)
    .get();
```

```go
result, err := workspace.ResolveQualifier(
    ctx,
    "paid-account",
    resolveContext,
    nil,
)
```
:::

## Evaluation

Rototo evaluates predicates in file order.

For each predicate:

1. Read the predicate attribute.
2. Apply the predicate operator.
3. Record the trace result.

All predicates are ANDed. If any predicate is false, the qualifier is false.

## Context Attributes

For a context attribute:

```toml
attribute = "account.plan"
```

rototo reads the JSON path from the context object. Missing paths fail
resolution.

## Qualifier Attributes

For a qualifier attribute:

```toml
attribute = "qualifier.paid-account"
```

rototo resolves the referenced qualifier and uses its boolean result as the
actual value for the predicate.

If qualifier references form a cycle, resolution fails with a qualifier cycle
error.

## Trace Shape

[CLI `--json`](reference-json-output.html) and `inspect --context` expose
qualifier traces:

```json
{
  "id": "paid-account",
  "value": true,
  "predicates": [
    {
      "index": 0,
      "kind": "compare",
      "attribute": "account.plan",
      "op": "in",
      "expected": ["growth", "enterprise"],
      "actual": "enterprise",
      "result": true
    }
  ]
}
```

Bucket predicates include a `bucket` object:

```json
{
  "index": 0,
  "kind": "bucket",
  "attribute": "account.id",
  "bucket": {
    "salt": "billing-policy-2026-06",
    "start": 0,
    "end": 1000,
    "value": 427
  },
  "result": true
}
```

## Error Boundaries

Qualifier resolution assumes lint has already validated workspace structure and
references. Runtime errors are still possible when the context does not satisfy
what the qualifier needs.

The usual production checks are:

- keep [`schemas/context.schema.json`](reference-context.html) current;
- exercise important contexts in
  [app tests](testing-runtime-configuration.html);
- log enough [trace](reference-resolution-output.html) or selected metadata to
  explain policy decisions.
