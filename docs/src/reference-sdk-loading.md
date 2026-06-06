# SDK Loading Reference

Applications should not parse workspace files directly. They should load a
workspace source with the SDK, let rototo lint it, and resolve named variables
from the loaded workspace.

The loading API is the boundary that decides whether the app receives a valid
control plane. Resolution and refresh have their own pages.

## `Workspace::load`

```rust
use rototo::Workspace;

let workspace = Workspace::load("git+https://github.com/acme/config.git#main").await?;
```

`Workspace::load` stages the source, inspects the workspace, runs lint, and
rejects lint failures. It accepts the same source forms as the CLI.

Use this for services that load configuration once at startup.

## `Workspace::inspect`

```rust
let workspace = Workspace::inspect("examples/basic").await?;
```

`Workspace::inspect` stages and inspects a workspace without requiring a
lint-clean runtime. It is the lower-level loader for tools that need to inspect
broken workspaces, editor state, or staged diagnostics.

Most application code should use `Workspace::load`.

## Load Options

```rust
use rototo::{LoadOptions, LintMode, SourceAuth};

let options = LoadOptions::new()
    .with_lint(LintMode::Deny)
    .with_source_auth(SourceAuth::Bearer(token));

let workspace = Workspace::load_with_options(source, options).await?;
```

`LintMode::Deny` is the default. It rejects lint failures during load.

`LintMode::Skip` is available for tools that need to stage or inspect a
workspace without enforcing lint. Do not use it as the default in application
runtime paths.

## Source Options

`LoadOptions` owns `SourceOptions`. Source options control auth and staging
limits:

```rust
use rototo::{SourceAuth, SourceOptions};
use std::time::Duration;

let source_options = SourceOptions::new()
    .with_auth(SourceAuth::Bearer(token))
    .with_http_timeout(Duration::from_secs(10));
```

Use source options when you need shorter network timeouts, archive limits, or
Bearer auth for HTTPS archive sources.

## Workspace Metadata

`Workspace` exposes loaded source metadata:

```rust
let root = workspace.root();
let inspection = workspace.inspection();
let context_schema = workspace.context_schema();
let fingerprint = workspace.source_fingerprint();
let immutable = workspace.immutable_source();
let layers = workspace.source_layers();
```

These are useful for observability. A service can log the source fingerprint
that selected a value, and can expose whether its loaded source is immutable.

## Temporary Staging

Remote sources are staged into temporary directories owned by the workspace
handle. Keep the `Workspace` value alive for as long as the app needs to
resolve from it.

Do not retain paths into the staged root after dropping the `Workspace`.

## Context Schema

When the loaded workspace contains `schemas/context.schema.json`,
`workspace.context_schema()` returns it. Resolution validates context against
that schema by default.

See `reference-context` and `reference-sdk-resolution`.
