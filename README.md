# So, what is rototo?

Every substantial software system eventually externalizes behavior into configuration: feature availability, model selection, tenant overrides, offers, prompts, retry policies, logging controls, and rollout rules.
The problem is that this configuration usually leaves the engineering lifecycle and moves into databases, dashboards, spreadsheets, admin consoles, and feature-flag systems.
It becomes harder to validate, test, review, promote, and explain.
Rototo provides a coherent control plane for behavioral configuration: versioned, reviewable, testable, contract-enforced configuration that applications can resolve at runtime.

## rototo's hello world

Let's take a simple use case: we want to vary the order amount beyond which customers get free shipping.
Customers in `standard` tier must have at least $50 as cart total while customers in `premium` tier get free shipping after $25.
To accomplish this, we would do two things:
- Create a rototo configuration package.
- Load the configuration package and resolve free shipping threshold in our application.

### Create and publish a configuration package

First, install the rototo cli from crates.io:
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
├── request-contexts
├── qualifiers
└── variables
    └── free-shipping-threshold.toml
├── catalogs
├── lint
6 directories, 2 files
```

We explain the full package structure in [Anatomy of a rototo package](docs/src/anatomy-of-a-rototo-package). For now, we would focus on the variable `free-shipping-threshold`. Replace the contents of `free-shipping-threshold.toml` with the following:
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

### Load the configuration package and resolve free shipping threshold

Install the rototo's python package:
```sh
python -m pip install rototo
```

We would use Python for our hello world. Save the following in `hello-rototo.py`:
```python
import asyncio
import sys

import rototo

VARIABLE_ID = "free-shipping-threshold"

def print_threshold(app_config: rototo.RefreshingPackage, tier: str) -> None:
    resolution = app_config.resolve_variable(
        variable = VARIABLE_ID,
        context = {
            "account": {
                "tier": tier,
            },
        },
    )

    threshold = resolution.value
    print(f'{tier}: {threshold} USD')


async def main() -> None:
    config_source = sys.argv[1] if len(sys.argv) > 1 else "app-config"

    app_config = await rototo.RefreshingPackage.load(
        config_source,
        period_seconds=1.0,
    )

    try:
        while True:
            print("---")
            print_threshold(app_config, "standard")
            print_threshold(app_config, "premium")
            await asyncio.sleep(2.0)
    finally:
        await app_config.shutdown()


if __name__ == "__main__":
    asyncio.run(main())

```

Run it with:
```sh
python hello-rototo.py app-config/
```

It would print the following:
```
---
standard: 50 USD
premium: 25 USD
```

Now, go ahead and edit `free-shipping-threshold.toml` and change the default value to 35. You would now see the following on console:
```
---
standard: 50 USD
premium: 35 USD
```

## Documentation

Public docs are available on [rototo.dev](https://rototo.dev).
The rototo cli also ships with the same documents in markdown.

You and your agent can use the `docs` command in the cli:
```sh
# show available docs
rototo docs

# search for docs
rototo docs -s <search terms>

# fetch doc based on doc id prefix
rototo docs -p anatomy-of
```

## rototo is designed for people and agents

Agents are now among the most important users of any development tool.
Hence, rototo is designed from ground up to work well both for people and agents.
- The configuration package is simply a dir tree of files that brings battle-tested ergnomics of file organization and editing.
- `rototo docs` to discover rototo's capabilities and the recipes to use it.
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
