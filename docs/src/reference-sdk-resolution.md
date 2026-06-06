# SDK Resolution Reference

Runtime application code should resolve named variables and qualifiers through a
loaded workspace handle. That keeps file parsing, lint, context validation, and
selection semantics inside rototo instead of copying them into the app.

## Context

Resolution uses `ResolveContext`:

```rust
use rototo::ResolveContext;

let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "plan": "enterprise"
    }
}))?;
```

The JSON value must be an object.

## Resolve A Variable

```rust
let resolution = workspace
    .resolve_variable("account-limits", &context)
    .await?;

println!("{} -> {}", resolution.value_key, resolution.value);
```

`VariableResolution` contains:

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Variable id. |
| `value_key` | string | Selected value key. |
| `value` | JSON value | Selected value. |

## Resolve A Qualifier

```rust
let resolution = workspace
    .resolve_qualifier("enterprise-account", &context)
    .await?;

println!("{}", resolution.value);
```

`QualifierResolution` contains:

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Qualifier id. |
| `value` | boolean | Final qualifier result. |

## Context Validation Options

By default, SDK resolution validates context against
`schemas/context.schema.json` when the workspace provides that schema.

To skip validation for a specific call:

```rust
use rototo::ResolveOptions;

let options = ResolveOptions {
    validate_context: false,
};

let resolution = workspace
    .resolve_variable_with_options("account-limits", &context, options)
    .await?;
```

Skipping validation does not make missing context paths valid. A qualifier that
reads a missing path can still fail resolution. This option only skips JSON
Schema validation of the context object.

## Workspace Loaded Without Runtime

`Workspace::inspect` and `Workspace::inspect_with_source_options` load a
workspace without compiling a runtime model. Resolution from those handles
fails with:

```text
workspace was loaded without a runtime model; use Workspace::load with lint enabled
```

Use `Workspace::load` or `RefreshingWorkspace::load` for application runtime
paths.

## Free Functions

The crate also exports filesystem-oriented functions:

```rust
rototo::resolve_variable(workspace_root, "account-limits", context_json).await?;
rototo::resolve_variables(workspace_root, context_json).await?;
rototo::resolve_qualifier(workspace_root, "enterprise-account", context_json).await?;
rototo::resolve_qualifiers(workspace_root, context_json).await?;
```

These compile the runtime workspace from a local root for each call. They are
handy for tests and tools. Long-running services should prefer a loaded
`Workspace` or `RefreshingWorkspace`.

## Traces

The loaded `Workspace` APIs return compact resolutions. The CLI and free trace
functions return explanation traces:

```rust
rototo::trace_variable_resolution(workspace_root, "account-limits", context_json).await?;
rototo::trace_qualifier_resolution(workspace_root, "enterprise-account", context_json).await?;
```

Use traces for tests, diagnostics, or observability where you need to explain
why a value was selected.
