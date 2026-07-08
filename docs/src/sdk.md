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

let resolution = package.resolve_variable("premium_message", &context)?;
println!("{}", resolution.value); // the resolved JSON value
```

```python
import rototo

package = await rototo.Package.load("examples/basic")

resolution = package.resolve_variable(
    "premium_message",
    {"user": {"tier": "premium"}},
)
print(resolution.value)  # the resolved JSON value
```

```typescript
import { Package } from "rototo";

const pkg = await Package.load("examples/basic");

const resolution = pkg.resolveVariable("premium_message", {
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
    "premium_message",
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

resolution, err := pkg.ResolveVariable("premium_message", map[string]any{
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
  string, array, or object);
- the **source** - where the value came from, so you can see *why* you got it.
  `source.kind` is `literal` for a plain value, `catalog` for a single
  [catalog](./package-format.md) entry (with the catalog and entry ids), or
  `catalog_list` for a `array<catalog:...>` value.

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
let resolution = package.resolve_variable("premium_users", &context)?;
if resolution.value == serde_json::json!(true) {
    // ...
}
```

```python
resolution = package.resolve_variable("premium_users", {"user": {"tier": "premium"}})
if resolution.value:
    ...
```

```typescript
const resolution = pkg.resolveVariable("premium_users", {
  user: { tier: "premium" },
});
if (resolution.value === true) {
  // ...
}
```

```java
VariableResolution resolution = pkg.resolveVariable(
    "premium_users",
    Map.of("user", Map.of("tier", "premium"))
);
boolean isPremium = Boolean.TRUE.equals(resolution.value());
```

```go
resolution, err := pkg.ResolveVariable("premium_users", map[string]any{
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

let resolution = package.resolve_variable("premium_message", &context)?;

// on shutdown:
package.shutdown().await;
```

```python
package = await rototo.RefreshingPackage.load(
    "https://config.acme.com/checkout/prod/current.tar.gz",
    period_seconds=300,
)

resolution = package.resolve_variable("premium_message", {"user": {"tier": "premium"}})

# on shutdown:
await package.shutdown()
```

```typescript
import { RefreshingPackage } from "rototo";

const pkg = await RefreshingPackage.load(
  "https://config.acme.com/checkout/prod/current.tar.gz",
  { periodSeconds: 300 },
);

const resolution = pkg.resolveVariable("premium_message", {
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
    "premium_message",
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

resolution, err := pkg.ResolveVariable("premium_message", map[string]any{
    "user": map[string]any{"tier": "premium"},
}, nil)
if err != nil {
    return err
}
```
:::

### Tuning refresh

`RefreshOptions` has three knobs beyond the period, and the defaults are meant
to be left alone until you have a reason:

- **`with_period(duration)`** - how often to re-check the source. No period
  means no background refresh: the package loads once and stays put. A source
  pinned to an immutable ref (a commit hash) can never produce a new result,
  so periodic refresh is disabled for it and the SDK logs a warning at load.
- **`with_failure_backoff(min, max)`** - what happens after a failed refresh.
  The loop retries with exponential backoff: the first failure waits `min`,
  each consecutive failure doubles the wait, and `max` caps it. Defaults are
  5 seconds and 5 minutes. The last good package serves the whole time; the
  backoff only spaces out the retries.
- **`with_max_staleness(duration)`** - your freshness budget, for health
  checks. Rototo doesn't act on this itself; it's the threshold you hand to
  `status().stale(...)` to answer "has it been too long since a successful
  refresh?" Wire that into a readiness probe or an alert, and a source that's
  been failing for an hour becomes visible instead of silently stale.

The current state is always one call away: `status()` returns the last
attempt and success times, the consecutive-failure count, the last error
string, and whether a refresh is running right now; `snapshot()` bundles that
with the current package identity for one-line logging.

### Starting degraded on a bundled fallback

Refresh protects a service that is already running: a failed refresh keeps the
last good package serving. But it can't help at startup, because there is no
last good package yet. If the config source is down when your process boots,
the load fails and the process doesn't start - your app's availability is now
coupled to your config host's.

The fallback source breaks that coupling. Ship the app with a copy of the
package it was tested against - a directory in the container image or app
bundle - and name it in the load options:

```rust
let package = RefreshingPackage::load_with_options(
    "https://config.acme.com/checkout/current.tar.gz",
    LoadOptions::new().with_fallback_source("/app/config-bundled"),
    RefreshOptions::new().with_period(Duration::from_secs(300)),
)
.await?;
```

The rules are strict so behavior stays predictable:

- A healthy primary always wins. The fallback only loads when the primary
  fails - fetch, auth, staging, parse, or the lint gate - and it goes through
  the exact same pipeline with no leniency. If both fail, you get one error
  naming both attempts and reasons, primary first.
- A refreshing package that started on the fallback keeps refreshing **from
  the primary** on the normal period and backoff. The fallback is static; it
  is never refreshed from. The first successful refresh is the ordinary
  refreshed event, and the primary is serving again.
- Starting on the fallback emits a `fallback_loaded` event carrying why the
  primary failed, and `status().serving_fallback()` stays true until the
  primary recovers. Pair it with `with_max_staleness` in your health checks so
  running degraded too long becomes an alarm, not a surprise.

To produce the bundled copy, project the same package your pipeline ships:
`rototo package <source> --unpacked /app/config-bundled` writes the flattened
tree as a plain directory at build time. An immutable-ref primary plus a
bundled fallback from the same commit is the reproducible shape: you can say
exactly what config any instance is serving, degraded or not.

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

The event itself is a flat record, and every field exists to be joined across
instances:

- **`event_type`** - one of `loaded` (the initial load), `refresh_started`,
  `unchanged`, `refreshed`, `failed`, `immutable` (a refresh was asked of a
  pinned source), or `shutdown`.
- **`event_id`** - a unique id for deduplicating shipped logs.
- **`source`** - the package source with any embedded credentials redacted,
  safe to log as-is.
- **`previous` / `current`** - the package identity on each side of the event:
  source fingerprint, load time, and release ref where the source has one.
  A `refreshed` event with both sides is exactly the "instance X moved from
  release A to B" line your dashboard wants.
- **`attempted_at` / `completed_at` / `duration`** - when and how long.
- **`outcome`** - for a completed check: `unchanged`, `refreshed`, or
  `immutable`.
- **`consecutive_failures` / `error`** - how deep a failure streak is and what
  the last error said.
- **`sdk`** - which SDK and version emitted the event, for fleets that mix
  languages.

## Tracing a single resolution

Refresh events tell you which package is live. The other question that shows up
- usually as a support ticket - is "why did *this one user* get *this* value?"
That's what a **trace** answers: the full record of one resolve, the rules it
tried, which one matched, the other variables it consulted along the way, and
the context it ran against. For a package composed from layers, the trace also
carries `provenance`: the layer whose `[resolve]` block produced the value, so
"which layer decided this" is right there in the record.

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
let resolution = package.resolve_variable_with_options("checkout_redesign", &context, options)?;
```

```python
resolution = package.resolve_variable("checkout_redesign", context, trace=True)
```

```typescript
const resolution = pkg.resolveVariable("checkout_redesign", context, { trace: true });
```

```java
VariableResolution resolution = pkg.resolveVariable(
    "checkout_redesign", context, ResolveOptions.trace(true));
```

```go
resolution, err := pkg.ResolveVariable("checkout_redesign", context, &rototo.ResolveOptions{Trace: true})
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

## Load options

Every loader takes options; in Rust they're a `LoadOptions` value, in the
other SDKs they're the load call's option bag. Five things live there:

- **Lint mode.** `load` runs the lint gate and refuses a failing package;
  `with_lint(LintMode::Skip)` turns the gate off, which is what `inspect` does
  for you (next section). Leave it on for anything that serves values.
- **Source auth.** `with_source_auth` carries bearer tokens for private HTTPS
  archive sources - the SDK-side twin of `--package-token`. Two shapes: a
  single token (binds to the load graph's one archive origin) or a map from
  `https://` URL prefixes to tokens, where the longest matching prefix wins
  and unmatched requests go out anonymous. Git sources authenticate through
  git itself.
- **Fallback source.** `with_fallback_source` names a second source to load
  when the primary fails, so a broken config fetch degrades your start instead
  of blocking it. The full story is in
  [Starting degraded on a bundled fallback](#starting-degraded-on-a-bundled-fallback).
- **Trace capacity.** The buffer depth of the trace-event stream. A consumer
  that falls behind drops the oldest events past this depth and sees a dropped
  marker with the count.
- **Refresh capacity.** The same, for the refresh-event stream.

Per-resolve options are separate and small: `ResolveOptions` holds
`validate_context` (the schema check described above, on by default) and
`trace` (compute a resolution trace for this call).

## Asking a package what it is

A loaded package can identify itself, which matters the moment logs from two
instances disagree:

- **`identity()`** - the package's identity as one value: the redacted source,
  the source fingerprint, and the load time. Log it at startup and every
  "which config is this box on?" question becomes grep.
- **`source_fingerprint()`** - the fingerprint alone: a stable hash of the
  staged content, so two instances serving identical bytes agree on it even
  if they loaded at different times.
- **`loaded_at()`** - when this package was staged.
- **`immutable_source()`** - whether the source is pinned (a commit ref), and
  so can never refresh into something new.
- **`source_layers()`** - for a composed package, the sources it was flattened
  from, in order.
- **`inspection()` and `context_schema()`** - the staged package data and the
  evaluation-context schema, for tools that introspect rather than resolve.

## Inspecting without the lint gate

One last loader worth knowing: alongside `load`, there's `inspect`. It stages the
same package data but *doesn't* run the lint gate, so it's the one to use for
tools that need to look at a package even when it has problems - an editor, a
dashboard, a diagnostics viewer. For anything that's going to actually serve
values, use `load`, so the lint gate stays between a broken package and
production.
