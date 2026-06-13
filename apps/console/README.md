# rototo console frontend

The console UI: a Vite + React single-page app served by `rototo console`.
The Rust server (src/console/) owns all data access — workspace staging,
lint, resolution, the GitHub write path — and this app talks to it over
`/api/*`. Built assets land in `dist/` and are embedded into release builds
of the `rototo` binary.

Development:

```sh
just console-setup     # npm install
just console-dev       # vite dev server, proxies /api to 127.0.0.1:7686
cargo run -- console   # the API the dev server proxies to
```

Build for embedding:

```sh
just console-build     # typecheck + vite build into dist/
```
