# Using Rototo

This page assumes you've been through the [quickstart](./quickstart.md) and understand [Rototo's concepts](./concepts.md).

To run Rototo as your runtime configuration control plane, there are really four decisions to make. Everything else builds on top of them:

- where the configuration package lives;
- how you make sure the configuration does what you mean it to;
- how it gets out to your running apps;
- how you watch what it's doing in production.

We'll walk through them one at a time.

## Where the package lives

We've already said Rototo packages live in git. You can keep the package right alongside your app's code, or give it its own dedicated repo - both work fine, so pick whichever fits how your team already works.

Once you've picked the repo, it's worth running `rototo setup`. It wires up:

- your **editor** (Neovim and VS Code), so editing configuration is comfortable;
- your **agent**, so it knows about the `rototo` CLI - Rototo is built to be agent-friendly, and it even ships all of its reference docs inside the CLI;
- your **shell**, so the `rototo` CLI is easier on the fingers.

## Making sure configuration does what you mean

A little effort here pays off enormously in misconfigurations you never ship. Concretely:

- Add `rototo lint` to both a pre-commit hook and your CI pipeline, so a broken package can't get committed or merged.
- Write a JSON schema for your evaluation context (in `<package-dir>/model/context/evaluation.schema.json`). It should describe exactly the context your app passes to the Rototo SDK at runtime. `rototo lint` leans on this to catch drift between what your app sends and what the variables and their rules expect.
- Write custom lint rules (in `<package-dir>/lint`) as extra guards Rototo can't infer on its own - say, "users on the `standard` tier must never get more than 5 projects."
- Write an integration test that runs your app through resolution of every variable, so you're testing the real loading path, not just the package model.

## Getting the package out to your fleet

Rototo can load packages from a bunch of sources and protocols, but two of them are the ones you'll actually reach for:

- for a small setup, you can get away with loading the package straight from the git repo;
- for a large fleet, move to a CDN or object store.

To distribute through a CDN or object store, the easiest path is:

- use `rototo package` to build a package archive;
- upload that archive to your CDN or object store;
- atomically move the environment channel pointer (like `prod/current`) to that digest, with a short cache lifetime;
- point your app at the package source `https://<your-domain>/rototo/<package-name>/prod/current.tar.gz`.

Keep both the object-store cache lifetime and your app's refresh period short (~5 seconds), and configuration changes will propagate to the fleet quickly.

## Watching it in production

Two things are worth watching in production:

- package refresh in a running app;
- resolution traces.

Tracking package refresh matters so you can be sure your config changes actually rolled out. The easiest way is to have your app subscribe to refresh events from the SDK and log them through your normal telemetry stack.

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

For resolution traces, the question is *which* resolutions to capture - and there are two ways to decide.

The powerful one is to let the package decide. You add a `[[trace]]` policy to `rototo-package.toml`, so you can turn tracing on for exactly the case you're chasing through a reviewed change - no app deploy:

```toml
[[trace]]
when = 'env.resolving.variable == "checkout_redesign" && context.user.id == "tester-123"'
```

The other way is for the app to ask for a trace on a specific call, when the app itself knows the request is interesting (a `?debug=1` flag, a support session, a sampled request):

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

Either way, the traces come out in one place: the trace stream. Your app subscribes and forwards them to its logs or debugger, off the resolve path:

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

## Wrapping up

As we said in the [motivation](./motivation.md), Rototo is about bringing the engineering rigor of code to runtime configuration without dragging along code's operational constraints. What we've walked through here is the whole lifecycle of a configuration package - where it can be reviewed, versioned, tested, released, and observed just like code, while still shipping on its own separate path.

For the exact details, the reference docs are the place to go (and they're a good thing to point your agent at, too).
