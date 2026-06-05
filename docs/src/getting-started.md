# Getting Started

Let's run the smallest full rototo loop to get a taste of what it is like to work with rototo. To achieve this you would:

- Install rototo cli and SDK
- Create rototo workspace, declare a variable `max-output-tokens` to value
`2000`.
- Write an app that loads the value of `max-output-tokens` from rototo workspace.
- Modify the value of `max-ouptut-tokens` to `3000` and see it change
automatically in your app.

## Create a configuration workspace

Install the rototo CLI:

```sh
cargo install rototo
```

Create a rototo workspace called `token-config` that defines `max-output-tokens` variable:

```sh
rototo init token-config --variable max-output-tokens
```

This would create a rototo directory tree that looks like the following:

```sh
token-config/
  |- rototo-workspace.toml
  |- lint/
  |- schemas/
  |- resources/
  |- qualifiers/
  |- variables/
    |- max-output-tokens.toml
```

For now, ignore all the directories and replace contents of `max-output-tokens.toml` with the following:

```toml
schema_version = 1

description = "Maximum output tokens for the summarizer"
type = "int"

# Possible values for max-output-tokens. In simplest case, there is only one value.
[values]
standard = 2000

# Default value for max-output-tokens. In this case, there is only one value.
[resolve]
value = "standard"
```

## Resolve from the CLI

Resolve the variable from the Git-backed workspace:

```sh
rototo resolve token-config \
  --variable max-output-tokens
```

Expected output:

```text
max-output-tokens=2000 (standard)
```

## Create an app

Return to the parent directory and create a Rust app:

```sh
cargo new token-app
cd token-app
```

In `Cargo.toml`, add rototo, Tokio, and serde_json:

```toml
[dependencies]
rototo = "0.1.0-alpha.1"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

Replace `src/main.rs`:

```rust
use std::{error::Error, time::Duration};

use rototo::{Environment, RefreshOptions, RefreshingWorkspace, ResolveContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let source = std::env::args()
        .nth(1)
        .expect("usage: token-app <workspace-source>");

    let refresh_options = RefreshOptions::new().with_period(Duration::from_secs(5));
    let workspace = RefreshingWorkspace::load(source, refresh_options).await?;
    let env = Environment::new("prod");
    let context = ResolveContext::from_json(serde_json::json!({}))?;

    loop {
        let resolution = workspace
            .resolve_variable("max-output-tokens", &env, &context)
            .await?;
        println!(
            "max-output-tokens: {} ({})",
            resolution.value, resolution.value_key
        );

        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
```

Run the app with the workspace source:

```sh
cargo run -- </path/to/token-config>
```

Expected output:

```text
max-output-tokens: 2000 (standard)
max-output-tokens: 2000 (standard)
```

Leave the app running.

## Change the workspace

In another terminal, go back to the workspace:

```sh
cd /path/to/token-config
```

Change the `standard` value in `variables/max-output-tokens.toml` from `2000` to
`3000`:

```toml
standard = 3000
```

## Watch the running app refresh

Return to the terminal where the app is running.

The app checks the workspace source every five seconds. Within a few refresh
cycles after the push, the new value should appear:

```text
max-output-tokens: 3000 (standard)
```

The app process did not restart. It loaded the first workspace version, resolved
`max-output-tokens`, kept running while the workspace changed, refreshed the
workspace source in the background, and resolved the same variable again.

Stop the app with `Ctrl-C`.

## What you learned

This was the smallest end-to-end rototo loop that gets you a feel for how it works. From this point, we can layer in more functionality to make it production grade as well as model complex use cases.
