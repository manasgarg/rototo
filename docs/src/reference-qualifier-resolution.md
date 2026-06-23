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
boolean result = workspace
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

Rototo evaluates the qualifier's `when` expression as a boolean expression.
Boolean operators short-circuit, so later paths are not read when an earlier
`&&` operand is already false or an earlier `||` operand is already true.

## Context Attributes

For a context path:

```toml
when = 'context.account.plan == "enterprise"'
```

rototo reads the JSON path from the context object. Missing paths fail
resolution.

## Qualifier References

For a qualifier reference:

```toml
when = 'qualifier["paid-account"]'
```

rototo resolves the referenced qualifier and uses its boolean result in the
expression.

If qualifier references form a cycle, resolution fails with a qualifier cycle
error.

## Trace Shape

[CLI `--json`](reference-json-output.html) and `inspect --context` expose
qualifier traces:

```json
{
  "id": "paid-account",
  "when": "context.account.plan in [\"growth\", \"enterprise\"]",
  "value": true
}
```

The trace records the expression and final result. It does not currently expose
per-subexpression evaluation details.

## Error Boundaries

Qualifier resolution assumes lint has already validated workspace structure and
references. Runtime errors are still possible when the context does not satisfy
what the qualifier needs.

The usual production checks are:

- keep [request context schemas](reference-context.html) current;
- exercise important contexts in
  [app tests](testing-runtime-configuration.html);
- log enough [trace](reference-resolution-output.html) or selected metadata to
  explain policy decisions.
