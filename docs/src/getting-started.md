# Getting Started

I like starting with one value because it keeps the whole system honest. If
`max-active-projects` can live in a workspace, pass lint, resolve from the CLI,
and update inside a running app, then the core rototo loop is working.

So we will build one workspace, one variable, one app process, and one refresh
path. The example is small on purpose. Once this loop is clear, the production
workflow is mostly about adding guardrails around the same loop.

You will create two directories next to each other:

```text
account-config/
account-app/
```

`account-config` is the rototo workspace. `account-app` is a process that
loads that workspace as its runtime configuration source.

## Create A Workspace

Install the rototo CLI:

```sh
cargo install rototo
```

Create a workspace with one variable template:

```sh
rototo init account-config --variable max-active-projects
```

The workspace is the control-plane boundary. Everything rototo needs to
understand this configuration starts at
[`rototo-workspace.toml`](reference-workspace-manifest.html) and lives in the
[directories beside it](reference-workspace-layout.html):

```text
account-config/
  rototo-workspace.toml
  lint/
  qualifiers/
  catalogs/
  request-contexts/
  variables/
    max-active-projects.toml
```

For the first pass, we only need one
[variable](reference-variables.html). The other directories are not ceremony;
they are places we will use later for
[conditions](reference-qualifiers.html),
[catalogs](reference-catalogs.html), request contexts, and
[custom lint](reference-custom-lua-lint.html).

Replace `account-config/variables/max-active-projects.toml` with one variable
the app can actually use:

```toml
schema_version = 1

description = "Maximum active projects for an account"
type = "int"

[resolve]
default = 3
```

The variable declares one
[typed value](reference-variable-values.html). The
`[resolve]` block says that `3` is the value to use when no
[rule](reference-variable-resolution.html) selects something else.

Before an application uses the workspace, I want the workspace to prove it is
valid on its own:

```sh
rototo lint account-config
```

## Resolve From The CLI

The next check is resolution. Before I wire configuration into an app, I want to
see the value the app would receive.

```sh
rototo resolve account-config --variable max-active-projects
```

Because no `--context` was passed, rototo evaluates the variable with an empty
JSON object, `{}`. The selected path is intentionally plain: no rules match, so
the default value wins.

```text
workspace: account-config
variable: max-active-projects
  pathway:
    default -> 3
  result:
    source: literal
    value: 3
```

That CLI check is small, but it matters. It proves the workspace loads, lints,
and resolves before the application is involved.

## Load From An App

Now we move the same resolution into a process. The app should not parse TOML,
walk workspace files, or copy resolution rules. It should
[load a workspace source](reference-sdk-loading.html) and
[ask for a named variable](reference-sdk-resolution.html).

If you are following the Rust path, create the app next to `account-config`:

```sh
cargo new account-app
cd account-app
```

Add rototo, Tokio, and serde_json to `Cargo.toml`:

```toml
[dependencies]
rototo = "0.1.0-alpha.4"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

The app loop is the same in each SDK: load a refreshing workspace, resolve the
same variable repeatedly, and let successful refreshes affect later
resolutions.

:::sdk-snippet getting-started-app-loop
```rust
use std::{error::Error, time::Duration};

use rototo::{RefreshOptions, RefreshingWorkspace, ResolveContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let source = std::env::args()
        .nth(1)
        .expect("usage: account-app <workspace-source>");

    let refresh = RefreshOptions::new().with_period(Duration::from_secs(5));
    let workspace = RefreshingWorkspace::load(source, refresh).await?;
    let context = ResolveContext::from_json(serde_json::json!({}))?;

    loop {
        let resolution = workspace
            .resolve_variable("max-active-projects", &context)
            .await?;

        println!(
            "max-active-projects: {} ({})",
            resolution.value, resolution.source
        );

        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
```

```python
import asyncio
import sys
import rototo


async def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: account-app <workspace-source>")

    workspace = await rototo.RefreshingWorkspace.load(
        sys.argv[1],
        period_seconds=5,
    )

    try:
        while True:
            resolution = await workspace.resolve_variable(
                "max-active-projects",
                {},
            )
            print(
                f"max-active-projects: "
                f"{resolution.value} ({resolution.source})"
            )
            await asyncio.sleep(5)
    finally:
        await workspace.shutdown()


asyncio.run(main())
```

```typescript
import { RefreshingWorkspace } from "rototo";

const source = process.argv[2];
if (!source) {
  throw new Error("usage: account-app <workspace-source>");
}

const workspace = await RefreshingWorkspace.load(source, {
  periodSeconds: 5,
});

try {
  while (true) {
    const resolution = await workspace.resolveVariable(
      "max-active-projects",
      {},
    );
    console.log(
      `max-active-projects: ${resolution.value} (${resolution.source})`,
    );
    await new Promise((resolve) => setTimeout(resolve, 5000));
  }
} finally {
  await workspace.shutdown();
}
```

```java
RefreshingWorkspaceOptions options = RefreshingWorkspaceOptions.builder()
    .periodSeconds(5.0)
    .build();

RefreshingWorkspace workspace = RefreshingWorkspace
    .load(args[0], options)
    .get();

try {
    while (true) {
        VariableResolution resolution = workspace
            .resolveVariable("max-active-projects", Map.of())
            .get();

        System.out.printf(
            "max-active-projects: %s (%s)%n",
            resolution.value(),
            resolution.source()
        );
        Thread.sleep(5_000);
    }
} finally {
    workspace.shutdown().get();
}
```

```go
periodSeconds := 5.0
workspace, err := rototo.LoadRefreshing(
    context.Background(),
    os.Args[1],
    &rototo.RefreshingWorkspaceOptions{
        PeriodSeconds: &periodSeconds,
    },
)
if err != nil {
    return err
}
defer workspace.Close(context.Background())

for {
    resolution, err := workspace.ResolveVariable(
        context.Background(),
        "max-active-projects",
        map[string]any{},
        nil,
    )
    if err != nil {
        return err
    }

    fmt.Printf(
        "max-active-projects: %v (%s)\n",
        resolution.Value,
        resolution.Source,
    )
    time.Sleep(5 * time.Second)
}
```
:::

I am using [`RefreshingWorkspace`](reference-sdk-refresh.html) even in the
first app because refresh is part of the runtime model. The service starts with
one known-good workspace, then future successful refreshes affect future
resolutions.

Run the app with the workspace source:

```sh
cargo run -- ../account-config
```

The app loads the workspace, lints it, and resolves the value in process:

```text
max-active-projects: 3 (standard)
max-active-projects: 3 (standard)
```

Leave it running.

## Change The Workspace

Now change the workspace while the app keeps running. In another terminal, edit
the workspace value:

```sh
cd /path/to/account-config
```

Change `standard` in `variables/max-active-projects.toml`:

```toml
standard = 5
```

Lint the workspace after the edit:

```sh
rototo lint .
```

Return to the app terminal. Within a refresh cycle, the new value should appear:

```text
max-active-projects: 5 (standard)
```

That is the first moment the rototo model pays off. The app process did not
restart. It loaded a workspace source at startup, resolved a named variable,
refreshed that same source in the background, and served the last successfully
loaded workspace while it kept running.

Stop the app with `Ctrl-C`.

## What Comes Next

This first loop used one unconditional account limit. Production work usually
adds runtime context, named qualifiers, workspace lint rules, tests, and a
hosted git source so configuration changes move through review and CI.

The [production workflow](production-workflow.html) builds those pieces onto
this same `account-config` workspace. The loop stays the same; we just add the
checks I would want before trusting this path in a service.
