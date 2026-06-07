# SDK Loading Reference

Applications should not parse workspace files directly. They should load a
workspace source with the SDK, let rototo lint it, and resolve named variables
from the loaded workspace.

The loading API is the boundary that decides whether the app receives a valid
control plane. Resolution and refresh have their own pages.

## Load A Workspace

:::sdk-snippet load-workspace
```rust
use rototo::Workspace;

let workspace = Workspace::load("git+https://github.com/acme/config.git#main").await?;
```

```python
import rototo

workspace = await rototo.Workspace.load(
    "git+https://github.com/acme/config.git#main",
)
```

```typescript
import { Workspace } from "rototo";

const workspace = await Workspace.load(
  "git+https://github.com/acme/config.git#main",
);
```
:::

Loading stages the source, inspects the workspace, runs lint, and rejects lint
failures. It accepts the same source forms as the CLI.

Use this for services that load configuration once at startup.

## Inspect A Workspace

:::sdk-snippet inspect-workspace
```rust
let workspace = Workspace::inspect("examples/basic").await?;
```

```python
workspace = await rototo.Workspace.inspect("examples/basic")
```

```typescript
const workspace = await Workspace.inspect("examples/basic");
```
:::

Inspection stages and inspects a workspace without requiring a lint-clean
runtime. It is the lower-level loader for tools that need to inspect broken
workspaces, editor state, or staged diagnostics.

Most application code should load a runtime workspace instead.

## Load Options

:::sdk-snippet load-options
```rust
use rototo::{LoadOptions, LintMode, SourceAuth};

let options = LoadOptions::new()
    .with_lint(LintMode::Deny)
    .with_source_auth(SourceAuth::Bearer(token));

let workspace = Workspace::load_with_options(source, options).await?;
```

```python
workspace = await rototo.Workspace.load(
    source,
    lint="deny",
    workspace_token=token,
)
```

```typescript
const workspace = await Workspace.load(source, {
  lint: "deny",
  workspaceToken: token,
});
```
:::

Lint deny is the default. It rejects lint failures during load.

Lint skip is available for tools that need to stage or inspect a workspace
without enforcing lint. Do not use it as the default in application runtime
paths.

## Workspace Metadata

:::sdk-snippet workspace-metadata
```rust
let root = workspace.root();
let inspection = workspace.inspection();
let context_schema = workspace.context_schema();
let fingerprint = workspace.source_fingerprint();
let immutable = workspace.immutable_source();
let layers = workspace.source_layers();
```

```python
root = workspace.root
```

```typescript
const root = workspace.root;
```
:::

The Rust SDK currently exposes the full loaded source metadata. The first
Python SDK release exposes the staged root path and keeps the runtime path
small; more inspection metadata can be added when Python tools need it.

## Temporary Staging

Remote sources are staged into temporary directories owned by the workspace
handle. Keep the workspace value alive for as long as the app needs to resolve
from it.

Do not retain paths into the staged root after dropping the workspace.

## Context Schema

When the loaded workspace contains `schemas/context.schema.json`, resolution
validates context against that schema by default.

See `reference-context` and `reference-sdk-resolution`.
