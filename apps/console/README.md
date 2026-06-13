# rototo console frontend

The console UI: a Vite + React single-page app served by `rototo console`.
The Rust server (src/console/) owns all data access — workspace staging,
lint, resolution, the GitHub write path — and this app talks to it over
`/api/*`. Built assets land in `dist/` and are embedded into release builds
of the `rototo` binary.

Development:

```sh
just setup          # one-time repo bootstrap, including console dependencies
just console-dev    # auto-reloading Rust API plus Vite UI for https://dev.rototo.dev
```

`just console-dev` writes local observability data to
`.rototo/dev/observability/`:

- `console-api.ndjson` for API latency, status, and operation events;
- `console-ui.ndjson` for browser API timing, route load, and error events;
- `console-dev.log` for raw Rust/Vite process output.

After exercising the console, run:

```sh
just console-observe
```

To fail when the current observability data has actionable findings above the
configured local thresholds:

```sh
just console-observe-check
```

When you only want one side of the stack:

```sh
just console-api    # Rust API at 127.0.0.1:7686 for dev.rototo.dev
just console-ui     # Vite UI at 127.0.0.1:5173
```

Build and run the embedded production shape:

```sh
just console-demo    # https://demo.rototo.dev via Caddy, API/UI at 127.0.0.1:7687
```

`just console-preview` still runs the embedded console on its default local
bind when you do not want the Caddy-hosted demo domain.
