<!-- Generated from README.md by `rototo docs --package-readme python --out sdks/python/README.md`. Do not edit directly. -->

# rototo Python SDK

Every substantial software system eventually needs a configuration subsystem. The software provides the underlying capabilities; configuration steers those capabilities to behave in a particular way.

Some configuration is settings-style: things like database URLs and encryption keys, usually held in environment variables and fixed once the software is deployed. That's not the kind we're concerned with here. What interests us instead is the configuration that governs the system's runtime behavior: feature availability, model selection, tenant overrides, offers, retry policies, logging controls, rollout plans, and so on.

Rototo provides a control plane for this kind of runtime configuration. It rests on a simple premise: runtime configuration should be treated like code. It should live alongside the code and follow a similar release cycle, and it should be testable and contract-enforced in the same way.

To that end, Rototo models configuration as files that are versioned, reviewed, tested, and released as packages. The Rototo SDK loads these packages within the application runtime to guide the application's behavior. Configuration thus follows the same release process as code, while gaining a hot-swappable deployment mechanism.

## Rototo's hello world

Let's take a simple use case: we want to vary the order amount beyond which customers get free shipping.
Customers in `standard` tier must have at least $50 as cart total while customers in `premium` tier get free shipping after $25.
To accomplish this, we would do two things:
- Create a Rototo configuration package.
- Load the configuration package and resolve free shipping threshold in our application.

### Create and publish a configuration package

First, install the Rototo cli from crates.io:
```sh
cargo install rototo --version 0.1.0-alpha.5
```

Now, create a configuration package for the application:
```sh
# Create app-config package with a variable named free-shipping-threshold
rototo init app-config --variable free-shipping-threshold
```

You should see the following in `app-config/` dir:
```sh
$> tree app-config
app-config
├── rototo-package.toml
├── evaluation-contexts
├── qualifiers
└── variables
    └── free-shipping-threshold.toml
├── catalogs
├── lint
6 directories, 2 files
```

We explain the package model in [Rototo Concepts](https://docs.rototo.dev/concepts.html). For now, we would focus on the variable `free-shipping-threshold`. Replace the contents of `free-shipping-threshold.toml` with the following:
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

We can further ensure that `free-shipping-threshold` resolves as expected.

```sh
# default value: should give 50
rototo resolve app-config --variable free-shipping-threshold
```

```sh
# standard account tier: should give 50
rototo resolve app-config --variable free-shipping-threshold --context account.tier=standard
```

```sh
# premium account tier: should give 25
rototo resolve app-config --variable free-shipping-threshold --context account.tier=premium
```

### Load the configuration package and resolve the threshold

Now let's read that value from an application. Install the rototo Python SDK:

```sh
python -m pip install rototo
```

Save this as `hello-rototo.py`. It loads a *refreshing* package (one that re-reads the source in the background) and prints the free-shipping threshold for a standard and a premium account every couple of seconds:

```python
import asyncio

import rototo

VARIABLE_ID = "free-shipping-threshold"


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

Run it (`python hello-rototo.py`) from the directory that holds `app-config`, and it prints:

```text
---
standard: 50 USD
premium: 25 USD
```

Now edit `free-shipping-threshold.toml`, change the default to 35, and save. Because the package refreshes every second, the next tick shows:

```text
---
standard: 50 USD
premium: 35 USD
```

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
- The configuration package is simply a dir tree of files that brings battle-tested ergnomics of file organization and editing.
- `rototo docs` to discover Rototo's capabilities and the recipes to use it.
- `rototo lint` as the backbone for configuration validation that can be run after every edit.
- `rototo inspect` to reason about the package structure and how everything resolves at runtime.
- `rototo resolve` for test automation of invariants (e.g. customer X must always receive configuration Y otherwise something is wrong).
- `rototo lsp` to provide feedback (and help) during editing.
- `rototo console` to have the comfort of a react based UI for inspecting and editing the package.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.
