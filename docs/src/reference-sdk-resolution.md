# SDK Resolution Reference

Runtime application code should resolve named
[variables](reference-variables.html) and
[qualifiers](reference-qualifiers.html) through a
[loaded workspace handle](reference-sdk-loading.html). That keeps file parsing,
lint, context validation, and selection semantics inside rototo instead of
copying them into the app.

## Context

Resolution uses a [JSON object context](reference-context.html):

:::sdk-snippet resolution-context
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

The JSON value must be an object.

## Resolve A Variable

:::sdk-snippet resolve-variable
```rust
let resolution = workspace
    .resolve_variable("account-limits", &context)
    .await?;

println!("{:?} -> {}", resolution.source, resolution.value);
```

```python
resolution = await workspace.resolve_variable(
    "account-limits",
    context,
)

print(f"{resolution.source} -> {resolution.value}")
```

```typescript
const resolution = await workspace.resolveVariable(
  "account-limits",
  context,
);

console.log(`${resolution.source.kind} -> ${resolution.value}`);
```

```java
VariableResolution resolution = workspace
    .resolveVariable("account-limits", context)
    .get();

System.out.println(resolution.source() + " -> " + resolution.value());
```

```go
resolution, err := workspace.ResolveVariable(
    ctx,
    "account-limits",
    resolveContext,
    nil,
)
if err != nil {
    return err
}

fmt.Printf("%v -> %v\n", resolution.Source, resolution.Value)
```
:::

`VariableResolution` contains:

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Variable id. |
| `value` | JSON value | Selected value. |
| `source` | object | Selected source. Literal values use `{ "kind": "literal" }`; catalog values include `catalog` and `value`. |

The TypeScript, Java, and Go SDKs expose `source` with their normal field or
method casing.

## Resolve A Qualifier

:::sdk-snippet resolve-qualifier
```rust
let resolution = workspace
    .resolve_qualifier("enterprise-account", &context)
    .await?;

println!("{}", resolution.value);
```

```python
resolution = await workspace.resolve_qualifier(
    "enterprise-account",
    context,
)

print(resolution.value)
```

```typescript
const resolution = await workspace.resolveQualifier(
  "enterprise-account",
  context,
);

console.log(resolution.value);
```

```java
QualifierResolution resolution = workspace
    .resolveQualifier("enterprise-account", context)
    .get();

System.out.println(resolution.value());
```

```go
resolution, err := workspace.ResolveQualifier(
    ctx,
    "enterprise-account",
    resolveContext,
    nil,
)
if err != nil {
    return err
}

fmt.Println(resolution.Value)
```
:::

`QualifierResolution` contains:

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Qualifier id. |
| `value` | boolean | Final qualifier result. |

## Context Validation Options

By default, SDK resolution validates context against
`schemas/context.schema.json` when the workspace provides that schema.

To skip validation for a specific call:

:::sdk-snippet context-validation-options
```rust
use rototo::ResolveOptions;

let options = ResolveOptions {
    validate_context: false,
};

let resolution = workspace
    .resolve_variable_with_options("account-limits", &context, options)
    .await?;
```

```python
resolution = await workspace.resolve_variable(
    "account-limits",
    context,
    validate_context=False,
)
```

```typescript
const resolution = await workspace.resolveVariable(
  "account-limits",
  context,
  { validateContext: false },
);
```

```java
VariableResolution resolution = workspace
    .resolveVariable(
        "account-limits",
        context,
        ResolveOptions.validateContext(false)
    )
    .get();
```

```go
resolution, err := workspace.ResolveVariable(
    ctx,
    "account-limits",
    resolveContext,
    &rototo.ResolveOptions{SkipContextValidation: true},
)
```
:::

Skipping validation does not make missing context paths valid. A qualifier that
reads a missing path can still fail resolution. This option only skips
[JSON Schema validation](reference-context.html) of the context object.

## Workspace Loaded Without Runtime

Inspection loads a workspace without compiling a runtime model. Resolution from
that handle fails with:

```text
workspace was loaded without a runtime model; use Workspace::load with lint enabled
```

Use loaded runtime workspaces or
[refreshing workspaces](reference-sdk-refresh.html) for application runtime
paths.

## Rust Free Functions

The Rust crate also exports filesystem-oriented functions:

```rust
rototo::resolve_variable(workspace_root, "account-limits", context_json).await?;
rototo::resolve_variables(workspace_root, context_json).await?;
rototo::resolve_qualifier(workspace_root, "enterprise-account", context_json).await?;
rototo::resolve_qualifiers(workspace_root, context_json).await?;
```

These compile the runtime workspace from a local root for each call. They are
handy for Rust tests and tools. Long-running services should prefer a loaded
workspace.

## Traces

The loaded SDK APIs return compact resolutions. The CLI and Rust free trace
functions return explanation traces:

```rust
rototo::trace_variable_resolution(workspace_root, "account-limits", context_json).await?;
rototo::trace_qualifier_resolution(workspace_root, "enterprise-account", context_json).await?;
```

Use [traces](reference-resolution-output.html) for tests, diagnostics, or
observability where you need to explain why a value was selected.
