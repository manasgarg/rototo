# Rust SDK Reference

The Rust SDK is the core implementation. Other language SDKs delegate runtime
behavior to this code so that loading, lint, refresh, and resolution semantics
stay in one place.

## Install

```toml
[dependencies]
rototo = "0.1.0-alpha.5"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

## Runtime Handle

```rust
use rototo::{ResolveContext, Workspace};

let workspace = Workspace::load("examples/basic").await?;
let context = ResolveContext::from_json(serde_json::json!({
    "user": {
        "tier": "premium"
    }
}))?;

let resolution = workspace
    .resolve_variable("premium-message", &context)
    .await?;
```

Use [`Workspace::load`](reference-sdk-loading.html) for application runtime
paths. Use `Workspace::inspect` for tools that need to load a workspace without
compiling a runtime model.

## Refreshing Handle

```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingWorkspace};

let refresh = RefreshOptions::new().with_period(Duration::from_secs(30));
let workspace = RefreshingWorkspace::load(source, refresh).await?;
```

[`RefreshingWorkspace`](reference-sdk-refresh.html) keeps the last successfully
loaded workspace active when a later refresh fails.

## Error Type

SDK calls return `rototo::Result<T>`, whose error type is `RototoError`.

## API Surface

The Rust crate exposes the broadest SDK surface:

| API | Purpose |
| --- | --- |
| `Workspace::load` | Load, lint, and compile a runtime workspace. |
| `Workspace::inspect` | Load workspace files without compiling runtime state. |
| `Workspace::lint` | Run lint against the loaded root. |
| `Workspace::resolve_variable` | Resolve one variable. |
| `Workspace::resolve_qualifier` | Resolve one qualifier to a boolean. |
| `RefreshingWorkspace::load` | Load and optionally start periodic refresh. |
| `RefreshingWorkspace::refresh_now` | Run a manual refresh. |
| `RefreshingWorkspace::status` | Read refresh state. |
| `RefreshingWorkspace::shutdown` | Stop the refresh loop. |

The crate also exports lower-level list, read, lint, resolve, trace, source,
catalog, and [testing](testing-runtime-configuration.html) helpers for Rust
tools and test suites.
