<!-- Generated from README.md by `rototo docs --package-readme go --out sdks/go/README.md`. Do not edit directly. -->

# rototo Go SDK

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
cargo install rototo --version 0.1.0-alpha.6
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

We explain the package model in [Rototo Concepts](https://docs.rototo.dev/concepts.html). For now, we would focus on the variable `free_shipping_threshold`. Replace the contents of `free_shipping_threshold.toml` with the following:
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

Now let's read that value from an application. Install the rototo Go SDK:

```sh
go get github.com/manasgarg/rototo/sdks/go@v0.1.0-alpha.6
```

Save this as `main.go`. It loads a *refreshing* package (one that re-reads the source in the background) and prints the free-shipping threshold for a standard and a premium account every couple of seconds:

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

Run it (`go run main.go`) from the directory that holds `app-config`, and it prints:

```text
---
standard: 50 USD
premium: 25 USD
```

Now edit `free_shipping_threshold.toml`, change the default to 35, and save. Because the package refreshes every second, the next tick shows:

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

The `use-cases` page (`rototo docs -p use-cases`) tours what teams put in a
package - release control, experiments, pricing, tenant overlays, regional
policy, environment separation - and each job points at a worked example
package under `examples/` in this repository.

## Rototo is designed for people and agents

Agents are now among the most important users of any development tool.
Hence, Rototo is designed from ground up to work well both for people and agents.
- The configuration package is simply a dir tree of files that brings battle-tested ergonomics of file organization and editing.
- `rototo docs` to discover Rototo's capabilities and the recipes to use it.
- `rototo lint` as the backbone for configuration validation that can be run after every edit.
- `rototo inspect` to reason about the package structure and how everything resolves at runtime.
- `rototo resolve` for test automation of invariants (e.g. customer X must always receive configuration Y otherwise something is wrong).
- `rototo lsp` to provide feedback (and help) during editing.
- `rototo console` to have the comfort of a react based UI for inspecting and editing the package.

## Roadmap: hard things rototo does not do yet

Runtime configuration earns trust in the ugly parts, not the feature tour, so
we keep this list in the open. Each item is a real production complication we
have looked at and not solved yet. (Some other hard things are deliberate
non-goals rather than roadmap items: exposure logging and experiment stats,
metric-driven auto-rollback, enumerated ID lists as targeting, secrets,
identity resolution, and Terraform-style enforcement of resolved state all
belong to the application or its other tools.)

For orientation, the things that used to be on this list and are now shipped
and demonstrated under `examples/`: structured composition (entry add, update,
and delete; atomic `[resolve]` override; namespaced variables; enum member
union and delete), the `governance.toml` layering contract enforced at compose time,
layers and allocations for rollouts and experiments, catalog queries with
filter/sort/limit and effective dating on `env.now`, and dev/staging/prod as
vertical layers over one contract. What
remains:

1. **Canarying a value change.** Staged rollout for a change to an existing
   variable's value, not just for new features. Config changes cause outages at
   the same rate as code changes.
2. **A break-glass path.** Kill switches need seconds; git review takes minutes
   to hours. An emergency change mechanism with mandatory post-hoc review.
3. **Flag lifecycle.** Owner and expiry metadata on variables, staleness
   warnings, and a worked "concluding an experiment" example: winner folded into
   the default, allocation removed.
4. **Grandfathering.** Pinning accounts to the plans and prices as of when they
   signed up: frozen old account classes beside evolving new ones.
5. **Totality lint.** "Exactly one entry for every cell of plan x market":
   completeness over enum cross-products, not just uniqueness.
6. **Jurisdiction dominance.** A deny that no lower layer, experiment, or tenant
   override can re-enable. Governance narrows grants; it cannot yet pin an
   outcome.
7. **Time-boundary awareness.** Timezone semantics for effective dates, and
   cache invalidation when a rule is known to flip at a time.
8. **Version-skew honesty.** Consumers refresh independently; multi-variable
   changes are not atomic in effect.
9. **Weighted rollout units.** Tenant-unit migrations where one tenant is a
    third of the load.
10. **The one-hop dereference built-in.** Following a catalog reference to an
    expression-typed field during a query, so audiences can carry authored
    conditions instead of fixed data bounds.
11. **Contract lockdown for vertical layers.** Environment layering wants a
    package-level governance default (a wildcard grant), and an overlay can
    still introduce a brand-new variable without any grant. "Environments
    differ in values, never in contract" is convention plus review, not yet a
    hard guarantee.
12. **The custom-lint execution boundary.** Loading a package runs its Lua lint
    today, including for remote sources you do not control. The invariant to
    establish: loading or resolving a package never executes package-supplied
    code; only author-time gates (pre-push, CI) do.
13. **Nested trace provenance.** A resolution trace says which rule matched,
    but not why a referenced condition variable was true; the trace should
    follow the reference chain. Related: variables have no visibility marker
    yet (app-facing versus internal helper), so the cross-variable dependency
    graph is disciplined only by convention.
14. **The web console, re-attached.** The console predates the current package
    layout, composition, and resolution methods; it is parked outside the core
    gate until it is brought back up against today's engine.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.
