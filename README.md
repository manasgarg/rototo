# rototo

`rototo` is a Rust project.

## Setup

Install `mise` and `just`, then run:

```sh
mise trust
just setup
```

Rust is pinned in `rust-toolchain.toml`. Non-Rust local development tools,
including Python, Node, and Wrangler, are pinned in `.tool-versions`.

## Development

Run the local check gate before pushing:

```sh
just check
```

`just check` is also what CI runs.

Logging uses `tracing` and reads `RUST_LOG`:

```sh
cargo run
RUST_LOG=debug cargo run
RUST_LOG=rototo=trace cargo run
```

```sh
git --git-dir=.bare worktree add -b my-branch my-branch main
```

## Documentation

The CLI ships bundled Markdown documentation and can export the same pages as
static HTML:

```sh
cargo run -- docs
cargo run -- docs -p quickstart
cargo run -- docs --export site
```

To check the rendered site remotely before a production deploy, publish a
Cloudflare Pages preview:

```sh
export CLOUDFLARE_ACCOUNT_ID=...
export CLOUDFLARE_API_TOKEN=...
just docs-preview
```

The preview deploys to the `docs-dev` branch of the `rototo-docs` Pages project
by default. Use `CLOUDFLARE_PAGES_PROJECT` to target another project, or pass a
different preview branch with `just docs-preview branch=my-docs-branch`.
`docs-preview` refuses `branch=main`; production docs are published by the
GitHub workflow after `main` updates.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.
