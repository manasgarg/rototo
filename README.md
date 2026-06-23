# rototo

rototo is a control plane for runtime configuration.

It is built around a small premise: configuration that changes production
behavior should move through the same discipline as code, even when the
application does not need to be redeployed.

rototo gives teams two things:

- Runtime configuration that stays inside the software lifecycle: review,
  tests, CI, observability, and rollback.
- Long-running applications that can refresh reviewed configuration without
  restarting or redeploying the application binary.

## Why rototo exists

Most production systems eventually need behavior to vary by environment,
account, request context, rollout state, or operational condition.

At first, the values look harmless: a limit, a switch, a model name, a prompt,
a rollout bucket, an exception for one customer. Then one of those values
starts controlling real production behavior, and the place where it lives
begins to matter.

Environment variables are familiar, but they often couple configuration changes
to deploys or restarts. Feature flag systems solve part of the runtime problem,
but they can create a release path that drifts away from the code, tests, and
review process that depend on them. Bespoke admin systems are even more
expensive: authentication, authorization, audit logs, validation, approvals,
APIs, migrations, rollback, and the operating habits around all of it.

rototo keeps runtime policy in git-backed workspace files. Applications load a
workspace source, provide runtime context, and resolve named variables through
the SDK. Long-running services can refresh the same source and keep serving the
last successfully loaded workspace if a later refresh fails.

## The model

A rototo workspace is a directory tree rooted at `rototo-workspace.toml`:

```text
account-config/
  rototo-workspace.toml
  qualifiers/
  catalogs/
  schemas/
  variables/
```

The main concepts are deliberately small:

- Workspaces are the git-versioned control-plane boundary.
- Context is the runtime facts supplied by the application.
- Qualifiers turn those facts into named reusable conditions.
- Variables select typed values using defaults and qualifier rules.
- Catalogs hold structured policy values validated by JSON Schema.
- Lint and tests make workspace changes releasable.

The core loop is:

1. Edit workspace files.
2. Review the diff.
3. Run lint and tests.
4. Merge the change.
5. Let applications refresh the workspace source and use the new values.

The configuration moves independently from the application binary, but it does
not move outside the engineering process.

## Install

Install the CLI from crates.io:

```sh
cargo install rototo --version 0.1.0-alpha.4
```

Use the SDK from an application:

```toml
[dependencies]
rototo = "0.1.0-alpha.4"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

## First loop

Start with one value. Create a workspace with one variable:

```sh
rototo init account-config --variable max-active-projects
```

Define the variable:

```toml
schema_version = 1

description = "Maximum active projects for an account"
type = "int"

[resolve]
default = 3
```

Then prove the workspace can stand on its own:

```sh
rototo lint account-config
rototo resolve account-config --variable max-active-projects
```

With no `--context`, rototo resolves with `{}` context and selects the default
value.

## SDK sketch

Applications should load a workspace source and ask for named variables. They
should not parse workspace files or duplicate qualifier logic.

```rust
use std::{error::Error, time::Duration};

use rototo::{RefreshOptions, RefreshingWorkspace, ResolveContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let source = "git+https://github.com/acme/runtime-config.git#main:prod";
    let refresh = RefreshOptions::new().with_period(Duration::from_secs(30));
    let workspace = RefreshingWorkspace::load(source, refresh).await?;

    let context = ResolveContext::from_json(serde_json::json!({
        "account": {
            "plan": "enterprise"
        }
    }))?;

    let resolution = workspace
        .resolve_variable("max-active-projects", &context)
        .await?;

    println!(
        "selected {} from {:?}",
        resolution.source,
        workspace.source_fingerprint()
    );

    Ok(())
}
```

## Where rototo fits

rototo fits when a configuration value changes application behavior and
deserves release discipline:

- account and environment-specific limits;
- operational switches;
- account-specific exceptions;
- bucketed rollouts;
- incident banners;
- model, prompt, and provider settings;
- runtime policy for another system.

rototo is not ordinary application storage. User records, transactions,
analytics events, and high-volume mutable data should stay in the systems that
already own them.

## Console

The same binary serves a web console for browsing workspaces, tracing how
variables resolve against saved contexts, editing drafts on real branches,
and publishing pull requests:

```sh
rototo console
```

It starts on `http://127.0.0.1:7686` with no sign-in. Local workspaces can be
read from disk and edited in the local working tree when writes are enabled.
GitHub workspaces use your ambient GitHub token
(`ROTOTO_WORKSPACE_TOKEN` or `gh auth token`) when a credential is needed.
Hosted deployments configure GitHub OAuth. Fixed-source deployments use
`--workspace <source>` and choose writes with
`--write disabled|pull-request|direct-push`. See the self-hosting guide for
the deployment and write-policy shapes.

## Documentation

Read the public docs at <https://docs.rototo.dev>.

The CLI also ships the same documentation:

```sh
rototo docs
rototo docs -p getting-started
rototo docs --export site
```

## Development

Install `mise` and `just`, then run:

```sh
mise trust
just setup
```

Rust is pinned in `rust-toolchain.toml`. Non-Rust local development tools,
including Python, Node, and Wrangler, are pinned in `.tool-versions`.

Run the local check gate before pushing:

```sh
just check
```

`just check` is also what CI runs.

For console work, `just setup` installs the frontend dependencies too. Run the
full local stack with:

```sh
just console-dev
```

With the local Caddy setup, `https://dev.rototo.dev` points at that dev stack.
Use `just console-demo` when you want `https://demo.rototo.dev` to point at the
embedded frontend served from the Rust binary.

Logging uses `tracing` and reads `RUST_LOG`:

```sh
cargo run
RUST_LOG=debug cargo run
RUST_LOG=rototo=trace cargo run
```

To check the rendered docs site remotely before a production deploy, publish a
Cloudflare Pages preview:

```sh
export CLOUDFLARE_ACCOUNT_ID=...
export CLOUDFLARE_API_TOKEN=...
just docs-preview
```

The preview deploys to the `docs-dev` branch of the `rototo-docs` Pages project
by default. Use `CLOUDFLARE_PAGES_PROJECT` to target another project, or pass a
different preview branch with `just docs-preview branch=my-docs-branch`.
`docs-preview` refuses `branch=main`; production docs are published by the
GitHub workflow after `main` updates.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.
