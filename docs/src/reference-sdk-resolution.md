# SDK Resolution Reference

Runtime application code should resolve named
[variables](reference-variables.html) and
[qualifiers](reference-qualifiers.html) through a
[loaded package handle](reference-sdk-loading.html). That keeps file parsing,
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
let resolution = pkg
    .resolve_variable("account-limits", &context)?;

println!("{:?} -> {}", resolution.source, resolution.value);
```

```python
resolution = pkg.resolve_variable(
    "account-limits",
    context,
)

print(f"{resolution.source} -> {resolution.value}")
```

```typescript
const resolution = pkg.resolveVariable(
  "account-limits",
  context,
);

console.log(`${resolution.source.kind} -> ${resolution.value}`);
```

```java
VariableResolution resolution = pkg
    .resolveVariable("account-limits", context)
    .get();

System.out.println(resolution.source() + " -> " + resolution.value());
```

```go
resolution, err := pkg.ResolveVariable(
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
let matches = pkg
    .resolve_qualifier("enterprise-account", &context)?;

println!("{matches}");
```

```python
matches = pkg.resolve_qualifier(
    "enterprise-account",
    context,
)

print(matches)
```

```typescript
const matches = pkg.resolveQualifier(
  "enterprise-account",
  context,
);

console.log(matches);
```

```java
boolean matches = pkg
    .resolveQualifier("enterprise-account", context)
    .get();

System.out.println(matches);
```

```go
matches, err := pkg.ResolveQualifier(
    ctx,
    "enterprise-account",
    resolveContext,
    nil,
)
if err != nil {
    return err
}

fmt.Println(matches)
```
:::

Qualifier resolution returns the final boolean result.

## Context Validation Options

By default, SDK resolution validates context against a compatible request
context schema when the package provides one.

To skip validation for a specific call:

:::sdk-snippet context-validation-options
```rust
use rototo::ResolveOptions;

let options = ResolveOptions {
    validate_context: false,
};

let resolution = pkg
    .resolve_variable_with_options("account-limits", &context, options)?;
```

```python
resolution = pkg.resolve_variable(
    "account-limits",
    context,
    validate_context=False,
)
```

```typescript
const resolution = pkg.resolveVariable(
  "account-limits",
  context,
  { validateContext: false },
);
```

```java
VariableResolution resolution = pkg
    .resolveVariable(
        "account-limits",
        context,
        ResolveOptions.validateContext(false)
    )
    .get();
```

```go
resolution, err := pkg.ResolveVariable(
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

## Package Loaded Without Runtime

Inspection loads a package without compiling a runtime model. Resolution from
that handle fails with:

```text
package was loaded without a runtime model; use Package::load with lint enabled
```

Use loaded runtime packages or
[refreshing packages](reference-sdk-refresh.html) for application runtime
paths.

## Rust Free Functions

The Rust crate also exports filesystem-oriented functions:

```rust
rototo::resolve_variable(package_root, "account-limits", context_json).await?;
rototo::resolve_variables(package_root, context_json).await?;
rototo::resolve_qualifier(package_root, "enterprise-account", context_json).await?;
rototo::resolve_qualifiers(package_root, context_json).await?;
```

These compile the runtime package from a local root for each call. They are
handy for Rust tests and tools. Long-running services should prefer a loaded
package.

## Traces

The loaded SDK APIs return compact resolutions. The CLI and Rust free trace
functions return explanation traces:

```rust
rototo::trace_variable_resolution(package_root, "account-limits", context_json).await?;
rototo::trace_qualifier_resolution(package_root, "enterprise-account", context_json).await?;
```

Use [traces](reference-resolution-output.html) for tests, diagnostics, or
observability where you need to explain why a value was selected.
