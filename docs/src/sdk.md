# The SDK

The CLI is for authoring, checking, and operating a package. The **SDK** is for
the other side: your running application, asking rototo for config values at
request time. This page covers that runtime surface - load a package, resolve
variables, and keep a long-running service refreshed.

rototo ships SDKs for Rust, Python, TypeScript, Go, and Java. They're all thin,
idiomatic wrappers over the same Rust core, so they behave identically - same
loading, same lint gate, same resolution. The snippets below switch by language;
pick yours with the toggle.

A note on naming styles before we start: each SDK follows its ecosystem's
conventions. Rust and Python use `resolve_variable`, TypeScript and Java use
`resolveVariable`, Go uses `ResolveVariable`. Same operation, local spelling.

## Loading a package and resolving a variable

This is the whole job, end to end: point the SDK at a [package
source](./package-sources.md), hand it a context object with your request facts,
and read back the resolved value.

The context is just plain JSON in whatever your language calls a dictionary or
map - no special type to construct (except in Rust, where you wrap it once).
Loading is asynchronous everywhere, because a source might be a remote repo or
archive. And loading runs lint: a package with errors is rejected at load, so a
broken package can't quietly start serving.

:::sdk-snippet load-and-resolve
```rust
use rototo::{EvaluationContext, Package};

let package = Package::load("examples/basic").await?;

let context = EvaluationContext::from_json(serde_json::json!({
    "user": { "tier": "premium" }
}))?;

let resolution = package.resolve_variable("premium-message", &context)?;
println!("{}", resolution.value); // the resolved JSON value
```

```python
import rototo

package = await rototo.Package.load("examples/basic")

resolution = package.resolve_variable(
    "premium-message",
    {"user": {"tier": "premium"}},
)
print(resolution.value)  # the resolved JSON value
```

```typescript
import { Package } from "rototo";

const pkg = await Package.load("examples/basic");

const resolution = pkg.resolveVariable("premium-message", {
  user: { tier: "premium" },
});
console.log(resolution.value); // the resolved JSON value
```

```java
import dev.rototo.Package;
import dev.rototo.VariableResolution;
import java.util.Map;

Package pkg = Package.load("examples/basic").get();

VariableResolution resolution = pkg.resolveVariable(
    "premium-message",
    Map.of("user", Map.of("tier", "premium"))
);
System.out.println(resolution.value()); // the resolved JSON value
```

```go
pkg, err := rototo.Load(ctx, "examples/basic", nil)
if err != nil {
    return err
}
defer pkg.Close(ctx)

resolution, err := pkg.ResolveVariable("premium-message", map[string]any{
    "user": map[string]any{"tier": "premium"},
}, nil)
if err != nil {
    return err
}
fmt.Println(resolution.Value) // the resolved JSON value
```
:::

### What a resolution gives you back

A variable resolution carries three things:

- the **id** of the variable;
- the **value** - the resolved config value, as native JSON (a bool, number,
  string, list, or object);
- the **source** - where the value came from, so you can see *why* you got it.
  `source.kind` is `literal` for a plain value, `catalog` for a single
  [catalog](./package-format.md) entry (with the catalog and entry ids), or
  `catalog_list` for a `list<catalog:...>` query result.

For a catalog-backed variable, `value` is the full structured entry - heading,
image, body, whatever the catalog defines - not just the entry's name.

## Resolving a condition variable

Sometimes you don't want a config value, you just want to know whether a named
condition holds - "is this a premium user?" Packages name those conditions as
**condition variables**: bool variables that default to `false` and flip to
`true` when the condition matches. There's no special API for them - you resolve
one like any other variable, and its `value` is a plain boolean.

:::sdk-snippet resolve-condition
```rust
let resolution = package.resolve_variable("premium-users", &context)?;
if resolution.value == serde_json::json!(true) {
    // ...
}
```

```python
resolution = package.resolve_variable("premium-users", {"user": {"tier": "premium"}})
if resolution.value:
    ...
```

```typescript
const resolution = pkg.resolveVariable("premium-users", {
  user: { tier: "premium" },
});
if (resolution.value === true) {
  // ...
}
```

```java
VariableResolution resolution = pkg.resolveVariable(
    "premium-users",
    Map.of("user", Map.of("tier", "premium"))
);
boolean isPremium = Boolean.TRUE.equals(resolution.value());
```

```go
resolution, err := pkg.ResolveVariable("premium-users", map[string]any{
    "user": map[string]any{"tier": "premium"},
}, nil)
if err != nil {
    return err
}
isPremium := resolution.Value == true
```
:::

## Keeping a long-running service fresh

`Package.load` gives you a snapshot: it loads once and never changes. That's
right for a CLI run or a short-lived job. But a service that runs for days wants
to pick up reviewed config changes *without* a redeploy - and that's what the
refreshing package is for.

You load it with a refresh period. It loads once up front, then re-checks the
source in the background on that interval. A successful refresh affects future
resolutions; a *failed* one keeps the last good package serving, so a bad update
never takes down a running service. You resolve against it exactly like a plain
package.

When the service shuts down, tell it to stop the background work.

:::sdk-snippet refresh
```rust
use rototo::{RefreshOptions, RefreshingPackage};
use std::time::Duration;

let package = RefreshingPackage::load(
    "https://config.acme.com/checkout/prod/current.tar.gz",
    RefreshOptions::new().with_period(Duration::from_secs(300)),
).await?;

let resolution = package.resolve_variable("premium-message", &context)?;

// on shutdown:
package.shutdown().await;
```

```python
package = await rototo.RefreshingPackage.load(
    "https://config.acme.com/checkout/prod/current.tar.gz",
    period_seconds=300,
)

resolution = package.resolve_variable("premium-message", {"user": {"tier": "premium"}})

# on shutdown:
await package.shutdown()
```

```typescript
import { RefreshingPackage } from "rototo";

const pkg = await RefreshingPackage.load(
  "https://config.acme.com/checkout/prod/current.tar.gz",
  { periodSeconds: 300 },
);

const resolution = pkg.resolveVariable("premium-message", {
  user: { tier: "premium" },
});

// on shutdown:
await pkg.shutdown();
```

```java
import dev.rototo.RefreshingPackage;

RefreshingPackage pkg = RefreshingPackage.load(
    "https://config.acme.com/checkout/prod/current.tar.gz"
).get();

VariableResolution resolution = pkg.resolveVariable(
    "premium-message",
    Map.of("user", Map.of("tier", "premium"))
);

// on shutdown:
pkg.shutdown().get();
```

```go
pkg, err := rototo.LoadRefreshing(ctx, "https://config.acme.com/checkout/prod/current.tar.gz", nil)
if err != nil {
    return err
}
defer pkg.Shutdown(ctx)

resolution, err := pkg.ResolveVariable("premium-message", map[string]any{
    "user": map[string]any{"tier": "premium"},
}, nil)
if err != nil {
    return err
}
```
:::

## Watching refreshes happen

A refreshing package works fine if you never look at it. But the moment you run
more than one instance, you'll want to know: did my reviewed change actually
reach the fleet, or is some box still serving the old package? The refreshing
package answers that by emitting a **refresh event** every time it checks the
source - whether nothing changed, a new package loaded, or a refresh failed.

You subscribe and forward those events to your normal logging or metrics, where
your ops tooling can join them up across instances. In Rust, Python, TypeScript,
and Go you read them as a stream; in Java you register a listener.

:::sdk-snippet refresh-events
```rust
let mut events = package.subscribe_refresh_events();
tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        tracing::info!("rototo refresh: {event:?}");
    }
});
```

```python
async for event in package.refresh_events():
    logging.info("rototo refresh: %s", event)
```

```typescript
for await (const event of pkg.refreshEvents()) {
  console.log("rototo refresh:", event);
}
```

```java
pkg.addRefreshListener(event -> {
    System.out.println("rototo refresh: " + event);
});
```

```go
events, err := pkg.RefreshEvents(ctx)
if err != nil {
    return err
}
go func() {
    for event := range events {
        log.Printf("rototo refresh: %+v", event)
    }
}()
```
:::

Each event tells you what kind of refresh it was, how long it took, and the
release identity it ended on - enough to build a dashboard that says "every
instance is on the release I just shipped." One thing to know about the delivery:
the stream is **best-effort and bounded**. It never blocks a refresh, and a
consumer that falls behind drops the *oldest* events rather than stalling the
service. So treat events as the audit trail of what changed and when, and treat
the snapshot (the current state, which you can always ask for) as the source of
truth you reconcile against.

## Tracing a single resolution

Refresh events tell you which package is live. The other question that shows up
- usually as a support ticket - is "why did *this one user* get *this* value?"
That's what a **trace** answers: the full record of one resolve, the rules it
tried, which one matched, the other variables it consulted along the way, and
the context it ran against.

Traces are verbose and meant for debugging, so they're emitted selectively, and
there are two ways to decide which resolutions to trace.

The first lives in the package, as a `[[trace]]` policy in the manifest (covered
in [package format](./package-format.md) and [Using Rototo](./adoption.md)) -
which means you can turn tracing on for exactly the case you're chasing through a
reviewed change, no app redeploy.

The second is to ask on a specific call, when the app itself knows a request is
worth tracing - a debug flag, a support session, a sampled request. Pass the
trace option to the resolve call:

:::sdk-snippet per-call-trace
```rust
use rototo::ResolveOptions;

let options = ResolveOptions { trace: true, ..ResolveOptions::default() };
let resolution = package.resolve_variable_with_options("checkout-redesign", &context, options)?;
```

```python
resolution = package.resolve_variable("checkout-redesign", context, trace=True)
```

```typescript
const resolution = pkg.resolveVariable("checkout-redesign", context, { trace: true });
```

```java
VariableResolution resolution = pkg.resolveVariable(
    "checkout-redesign", context, ResolveOptions.trace(true));
```

```go
resolution, err := pkg.ResolveVariable("checkout-redesign", context, &rototo.ResolveOptions{Trace: true})
```
:::

Either way, the traces come out on the same stream, which the SDK delivers
alongside the refresh events:

:::sdk-snippet trace-events
```rust
let mut traces = package.subscribe_trace_events();
tokio::spawn(async move {
    while let Some(item) = traces.recv().await {
        match item {
            rototo::TraceStreamItem::Trace(trace) => tracing::info!("trace: {trace:?}"),
            rototo::TraceStreamItem::Dropped { count } => {
                tracing::warn!(count, "rototo traces dropped")
            }
        }
    }
});
```

```python
async for item in package.trace_events():
    if item["kind"] == "trace":
        logging.info("trace: %s", item["trace"])
    else:  # {"kind": "dropped", "count": n}
        logging.warning("rototo traces dropped: %s", item["count"])
```

```typescript
for await (const item of pkg.traceEvents()) {
  if (item.kind === "trace") {
    console.log("trace:", item.trace);
  } else {
    console.warn("rototo traces dropped:", item.count);
  }
}
```

```java
pkg.addTraceListener(trace -> {
    System.out.println("trace: " + trace);
});
```

```go
traces, err := pkg.TraceEvents(ctx)
if err != nil {
    return err
}
go func() {
    for item := range traces {
        log.Printf("trace: %+v", item)
    }
}()
```
:::

Two things make this safe to leave wired up. First, tracing is only computed
while something is actually listening - with no subscriber, a `[[trace]]` policy
costs nothing, because rototo skips the work. Second, the stream is bounded the
same way refresh events are: a consumer that falls behind gets a **dropped
marker** with a count instead of stalling resolution. That marker matters when
you're debugging - silence then means "not traced," never "traced but lost."

One caution: a trace carries the *full* request context so you can see exactly
what the resolve saw, and that context often holds user identifiers. Redacting
before you log is the application's job - same boundary as everywhere else,
rototo hands you the facts and you decide what's safe to keep.

## A few things that hold across every SDK

**Private sources.** When a source needs a token, pass it at load time - the
SDKs take a package-token option, mirroring the CLI's `--package-token`.

**Errors.** Each SDK maps rototo's failures into the language's normal error
type - an exception in Python, a rejected promise in TypeScript, an `error`
return in Go, a `Result` in Rust, a thrown exception in Java. A lint failure at
load shows up the same way, so "the package is broken" and "the network is down"
both surface through your existing error handling.

**Context validation.** By default, the SDK checks your context against the
package's [evaluation-context schema](./package-format.md) before resolving, so a
malformed context is caught early. You can turn that off per call if you've
already validated upstream.

**Version.** Every SDK exposes the canonical rototo version (currently
`0.1.0-alpha.6`) - as `rototo.__version__` in Python, a `version()` call in
TypeScript, Go, and Java, and the crate version in Rust. The Python wheel
displays its ecosystem-normalized spelling (`0.1.0a5`) in package metadata, but
the version the runtime reports is the canonical one.

## Inspecting without the lint gate

One last loader worth knowing: alongside `load`, there's `inspect`. It stages the
same package data but *doesn't* run the lint gate, so it's the one to use for
tools that need to look at a package even when it has problems - an editor, a
dashboard, a diagnostics viewer. For anything that's going to actually serve
values, use `load`, so the lint gate stays between a broken package and
production.
