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
use rototo::{ResolveContext, Package};

let pkg = Package::load("examples/basic").await?;
let context = ResolveContext::from_json(serde_json::json!({
    "user": {
        "tier": "premium"
    }
}))?;

let resolution = pkg
    .resolve_variable("premium-message", &context)
    .await?;
```

Use [`Package::load`](reference-sdk-loading.html) for application runtime
paths. Use `Package::inspect` for tools that need to load a package without
compiling a runtime model.

## Refreshing Handle

```rust
use std::time::Duration;
use rototo::{RefreshOptions, RefreshingPackage};

let refresh = RefreshOptions::new().with_period(Duration::from_secs(30));
let pkg = RefreshingPackage::load(source, refresh).await?;
```

[`RefreshingPackage`](reference-sdk-refresh.html) keeps the last successfully
loaded package active when a later refresh fails.

## Error Type

SDK calls return `rototo::Result<T>`, whose error type is `RototoError`.

## API Surface

The Rust crate exposes the broadest SDK surface:

| API | Purpose |
| --- | --- |
| `Package::load` | Load, lint, and compile a runtime package. |
| `Package::inspect` | Load package files without compiling runtime state. |
| `Package::lint` | Run lint against the loaded root. |
| `Package::resolve_variable` | Resolve one variable. |
| `Package::resolve_qualifier` | Resolve one qualifier to a boolean. |
| `RefreshingPackage::load` | Load and optionally start periodic refresh. |
| `RefreshingPackage::refresh_now` | Run a manual refresh. |
| `RefreshingPackage::status` | Read refresh state. |
| `RefreshingPackage::shutdown` | Stop the refresh loop. |

The crate also exports lower-level list, read, lint, resolve, trace, source,
catalog, and [testing](testing-runtime-configuration.html) helpers for Rust
tools and test suites.
