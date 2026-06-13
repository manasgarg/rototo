# Variable Resolution Reference

Variable resolution is where rototo turns a named runtime configuration
request into one selected value. The app supplies context, rototo evaluates
rules, and the selected value comes from the workspace version currently loaded
by the app.

## Input

Variable resolution needs:

- a [loaded, lint-clean workspace](reference-sdk-loading.html);
- a variable id;
- a [JSON object context](reference-context.html).

CLI:

```sh
rototo resolve account-config \
  --variable account-limits \
  --context account.plan=enterprise
```

SDK:

:::sdk-snippet variable-resolution-sdk
```rust
let resolution = workspace
    .resolve_variable("account-limits", &context)
    .await?;
```

```python
resolution = await workspace.resolve_variable("account-limits", context)
```

```typescript
const resolution = await workspace.resolveVariable("account-limits", context);
```

```java
VariableResolution resolution = workspace
    .resolveVariable("account-limits", context)
    .get();
```

```go
resolution, err := workspace.ResolveVariable(
    ctx,
    "account-limits",
    resolveContext,
    nil,
)
```
:::

## Rule Evaluation

Variable rules are evaluated in file order:

```toml
[resolve]
default = "growth"

[[resolve.rule]]
qualifier = "enterprise-account"
value = "enterprise"

[[resolve.rule]]
qualifier = "paid-account"
value = "paid"
```

Resolution follows this path:

1. Resolve the first rule's qualifier.
2. If it is true, select that rule's value and stop.
3. If it is false, continue to the next rule.
4. If no rule matches, select the default value.

The default is always the fallback. It is not evaluated as a qualifier.

## Value Selection

For [primitive variables](reference-variable-values.html), selected values come
from `[values]`.

For [catalog-backed variables](reference-catalogs.html), selected values come
from `catalogs/<catalog-id>-entries/*.toml` and are validated against the
catalog schema.

The resolution result includes both:

- `value_key`: the selected key;
- `value`: the selected JSON value.

## Shadowed Rules

If two rules use the same qualifier, the later rule can never win. Rototo
reports `rototo/variable-rule-shadowed`.

If a rule selects the same value as the default, rototo reports
`rototo/variable-rule-selects-default-value`.

These rules are warnings about operational clarity. A reader should not have to
guess whether a later rule is meaningful.

## Trace Shape

[CLI `--json`](reference-json-output.html) returns a variable trace:

```json
{
  "resolution": {
    "id": "account-limits",
    "value_key": "enterprise",
    "value": {
      "projects": 100,
      "members": 250
    }
  },
  "default_value": "growth",
  "rules": [
    {
      "index": 0,
      "qualifier": "enterprise-account",
      "value": "enterprise",
      "matched": true
    }
  ],
  "qualifier_traces": [
    {
      "id": "enterprise-account",
      "value": true,
      "predicates": []
    }
  ]
}
```

`qualifier_traces` contains the qualifiers evaluated while resolving the
variable. Use it to explain why the selected value won.

## Context Validation

If [`schemas/context.schema.json`](reference-context.html) exists, rototo
validates context before resolution. SDK callers can disable that with
[`ResolveOptions`](reference-sdk-resolution.html), but the default is validation
on.

## Multiple Variables

`rototo resolve --variables` resolves all variables in workspace order with the
same context. The SDK free function `rototo::resolve_variables` does the same
for a local workspace root.

If any variable resolution fails, the command or API call fails. Use targeted
selectors when debugging a context that is not meant to satisfy every variable.
