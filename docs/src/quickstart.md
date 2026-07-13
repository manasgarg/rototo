# Quickstart with Rototo

Let's take a simple use case: we want to vary the order amount beyond which customers get free shipping.
Customers in `standard` tier must have at least $50 as cart total while customers in `premium` tier get free shipping after $25.
To accomplish this, we would do two things:
- Create a Rototo configuration package.
- Load the configuration package and resolve free shipping threshold in our application.

### Create and publish a configuration package

First, install the Rototo cli from crates.io:
```sh
cargo install rototo --version 0.1.0-alpha.7
```

Now, create a configuration package for the application:
```sh
# Create app-config package with a variable named free_shipping_threshold
rototo init app-config --variable free_shipping_threshold
```

You should see the following in `app-config/` dir:
```sh
$> tree app-config
app-config
├── rototo-package.toml
├── variables
│   └── free_shipping_threshold.toml
├── model
│   ├── catalogs
│   └── context
├── data
│   └── catalogs
└── lint

7 directories, 2 files
```

We explain the package model in [Rototo Concepts](./concepts.md). For now, we would focus on the variable `free_shipping_threshold`. Replace the contents of `free_shipping_threshold.toml` with the following:
```toml
schema_version = 1
description = "$ threshold for free shipping."
type = "int"

[resolve]
default = 50  # by default, free shipping beyond $50.

[[resolve.rule]]
when = '(context.account.tier == "premium")'
value = 25    # for premium account tier, free shipping beyond $25.
```

We can now validate our configuration to ensure that we got it right:
```sh
rototo lint app-config
```

We can further ensure that `free_shipping_threshold` resolves as expected.

```sh
# default value: should give 50
rototo resolve app-config --variable free_shipping_threshold
```

```sh
# standard account tier: should give 50
rototo resolve app-config --variable free_shipping_threshold --context account.tier=standard
```

```sh
# premium account tier: should give 25
rototo resolve app-config --variable free_shipping_threshold --context account.tier=premium
```

### Load the configuration package and resolve the threshold

Now let's read that value from an application. This is where the SDK comes in:
your app points it at the same package source you've been using on the command
line, and asks for the variable by name. rototo ships SDKs for Rust, Python,
TypeScript, Go, and Java - pick yours below.

First install the SDK for your language - `pip install rototo` for Python,
`npm install rototo` for Node, the `rototo` crate for Rust, and so on.

Then save this little program. It loads the package, and every couple of seconds
prints the free-shipping threshold for a standard account and a premium one.
The interesting bit is that it loads a *refreshing* package - one that re-reads
the source in the background - so it'll notice config changes while it's running.

:::sdk-snippet quickstart-app
```rust
use std::time::Duration;

use rototo::{EvaluationContext, RefreshOptions, RefreshingPackage};
use serde_json::json;

const VARIABLE_ID: &str = "free_shipping_threshold";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_config = RefreshingPackage::load(
        "app-config",
        RefreshOptions::new().with_period(Duration::from_secs(1)),
    )
    .await?;

    loop {
        println!("---");
        for tier in ["standard", "premium"] {
            let context = EvaluationContext::from_json(json!({ "account": { "tier": tier } }))?;
            let resolution = app_config.resolve_variable(VARIABLE_ID, &context)?;
            println!("{tier}: {} USD", resolution.value);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
```

```python
import asyncio

import rototo

VARIABLE_ID = "free_shipping_threshold"


def print_threshold(app_config, tier):
    resolution = app_config.resolve_variable(
        VARIABLE_ID,
        {"account": {"tier": tier}},
    )
    print(f"{tier}: {resolution.value} USD")


async def main():
    app_config = await rototo.RefreshingPackage.load("app-config", period_seconds=1.0)
    try:
        while True:
            print("---")
            print_threshold(app_config, "standard")
            print_threshold(app_config, "premium")
            await asyncio.sleep(2.0)
    finally:
        await app_config.shutdown()


asyncio.run(main())
```

```typescript
import { RefreshingPackage } from "rototo";

const VARIABLE_ID = "free_shipping_threshold";
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const appConfig = await RefreshingPackage.load("app-config", { periodSeconds: 1 });

try {
  while (true) {
    console.log("---");
    for (const tier of ["standard", "premium"]) {
      const resolution = appConfig.resolveVariable(VARIABLE_ID, { account: { tier } });
      console.log(`${tier}: ${resolution.value} USD`);
    }
    await sleep(2000);
  }
} finally {
  await appConfig.shutdown();
}
```

```java
import dev.rototo.RefreshingPackage;
import dev.rototo.RefreshingPackageOptions;
import dev.rototo.VariableResolution;
import java.util.Map;

public class HelloRototo {
    static final String VARIABLE_ID = "free_shipping_threshold";

    public static void main(String[] args) throws Exception {
        RefreshingPackage appConfig = RefreshingPackage.load(
            "app-config",
            RefreshingPackageOptions.builder().periodSeconds(1.0).build()
        ).get();

        try {
            while (true) {
                System.out.println("---");
                for (String tier : new String[] {"standard", "premium"}) {
                    VariableResolution resolution = appConfig.resolveVariable(
                        VARIABLE_ID,
                        Map.of("account", Map.of("tier", tier))
                    );
                    System.out.println(tier + ": " + resolution.value() + " USD");
                }
                Thread.sleep(2000);
            }
        } finally {
            appConfig.shutdown().get();
        }
    }
}
```

```go
package main

import (
    "context"
    "fmt"
    "time"

    "github.com/manasgarg/rototo/sdks/go"
)

const variableID = "free_shipping_threshold"

func main() {
    ctx := context.Background()
    period := 1.0
    appConfig, err := rototo.LoadRefreshing(ctx, "app-config", &rototo.RefreshingPackageOptions{
        PeriodSeconds: &period,
    })
    if err != nil {
        panic(err)
    }
    defer appConfig.Shutdown(ctx)

    for {
        fmt.Println("---")
        for _, tier := range []string{"standard", "premium"} {
            resolution, err := appConfig.ResolveVariable(variableID, map[string]any{
                "account": map[string]any{"tier": tier},
            }, nil)
            if err != nil {
                panic(err)
            }
            fmt.Printf("%s: %v USD\n", tier, resolution.Value)
        }
        time.Sleep(2 * time.Second)
    }
}
```
:::

Run it, pointing at the package you created. It prints:

```
---
standard: 50 USD
premium: 25 USD
```

Now leave it running, open `free_shipping_threshold.toml`, change the default
value to 35, and save. Because the program reloads the package every second, the
next tick picks up your change on its own - no restart:

```
---
standard: 50 USD
premium: 35 USD
```

That's the whole loop: edit reviewed config in the package, and a running app
refreshes into it.

One variable is the smallest possible package, but the same loop carries
feature rollouts, pricing tables, tenant overlays, and provider failover. The
[use cases](./use-cases.md) page tours those jobs, each with a worked example
package in the repository.

## Documentation

Public docs are available on [rototo.dev](https://rototo.dev).
The `rototo` cli also ships with the same documents in markdown.

You and your agent can use the `docs` command in the cli:
```sh
# show available docs
rototo docs

# search for docs
rototo docs -s <search terms>

# fetch doc based on doc id prefix
rototo docs -p concepts
```

## Rototo is designed for people and agents

Agents are now among the most important users of any development tool.
Hence, Rototo is designed from ground up to work well both for people and agents.
- The configuration package is simply a dir tree of files that brings battle-tested ergonomics of file organization and editing.
- `rototo docs` to discover Rototo's capabilities and the recipes to use it.
- `rototo lint` as the backbone for configuration validation that can be run after every edit.
- `rototo inspect` to reason about the package structure and how everything resolves at runtime.
- `rototo resolve` for test automation of invariants (e.g. customer X must always receive configuration Y otherwise something is wrong).
- `rototo lsp` to provide feedback (and help) during editing.
- The rototo console, a companion web app that ships separately as `rototo-console`, for a friendly UI over inspecting and editing the package.
