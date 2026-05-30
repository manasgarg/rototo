# rototo Rust SDK

Use `Workspace` for embedded applications. Loading a workspace parses and lints
it by default, so applications only receive a workspace that is ready to resolve.

```rust
use rototo::{Environment, ResolveContext, Workspace};

let workspace = Workspace::load("./workspace").await?;
let env = Environment::new("prod");
let context = ResolveContext::from_json(serde_json::json!({
    "account": {
        "plan": "enterprise"
    }
}))?;

let qualifier = workspace
    .resolve_qualifier("enterprise-accounts", &context)
    .await?;
let variable = workspace
    .resolve_variable("payment-review-queue", &env, &context)
    .await?;
```

`Environment` is only the environment name. Environment-owned properties are not
part of the SDK contract; application/runtime facts belong in `ResolveContext`.

## Workspace Loading

- `Workspace::load(source).await` parses, lints, and prepares the workspace.
- `Workspace::load_with_options(source, LoadOptions::new().with_lint(LintMode::Skip)).await`
  is available for tools that need to inspect broken workspaces.
- `workspace.inspection()` exposes environments, qualifiers, and variables.
- `workspace.lint().await` reruns workspace lint and returns structured diagnostics.

`source` can be a local path, `file://` URI, `git+file://`, `git+https://`,
`git+ssh://`, or `https://` archive URL. Git sources support `#ref:subdir`;
archive URLs support `#:subdir`.

## Periodic Workspace Refresh

Applications that want the SDK to keep a workspace fresh in the background can
use `RefreshingWorkspace`:

```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingWorkspace};

let workspace = RefreshingWorkspace::load(
    "git+https://github.com/example/config.git#main:rototo",
    RefreshOptions::new().with_period(Duration::from_secs(60)),
).await?;
```

The initial load must succeed. After that, refresh failures are non-fatal:
resolution continues using the last successfully loaded workspace, and refresh
status records the failure.

```rust
let status = workspace.status().await;
if let Some(error) = status.last_error {
    tracing::warn!(%error, "workspace refresh is failing");
}
```

`refresh_now().await` runs the same refresh path on demand and returns whether
the source was unchanged, refreshed, or immutable.

Git sources check the remote ref before cloning when a ref is provided. Sources
pinned to a full commit SHA are treated as immutable: the SDK logs a warning and
does not run periodic refresh for them. HTTPS archive sources use `ETag` or
`Last-Modified` when available; otherwise the SDK falls back to loading and only
publishes the new workspace if it fully validates.

## Resolve Context Contract

Workspaces can declare a JSON Schema for application-provided resolve context:

```toml
[context]
schema = "schemas/context.schema.json"
```

The linter validates this contract:

- the schema file exists, parses, and is a valid JSON Schema
- qualifier context attributes are declared by the schema

Resolution validates each `ResolveContext` against the schema before evaluating
qualifiers or variables. Use `resolve_*_with_options` and
`ResolveOptions { validate_context: false }` only for tooling that deliberately
needs to bypass the application contract.

## External Variable Values

Variable values can be declared inline under `[variable.values]` or in TOML
files next to the variable file. For `variables/banner.toml`, rototo also loads
`variables/banner-values/*.toml`; each file stem is the value key.

Each file can use a single top-level `value` key:

```toml
value = "Welcome back."
```

Object values use a `[value]` table:

```toml
[value]
queue = "priority-review"
timeout_ms = 3000
```

Custom Lua lint can validate each expanded value by defining
`lint_value(value)`. The argument contains `name`, `value`, and `variable`.

## Lower-Level APIs

The crate still exposes the function APIs used by the CLI:

- `inspect_workspace(path).await`
- `lint_workspace(path).await`
- `list_qualifiers(workspace).await`
- `read_qualifier(workspace, id).await`
- `resolve_qualifier(workspace, id, context).await`
- `resolve_variable(workspace, id, environment, context).await`

Prefer `Workspace` for embedded applications because it loads once, enforces
lint on load, and keeps context validation close to resolution.
