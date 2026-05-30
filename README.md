# rototo

`rototo` is a Rust project.

## Setup

Install `mise` and `just`, then run:

```sh
mise trust
just setup
```

Rust is pinned in `rust-toolchain.toml`. Python is pinned in
`.tool-versions` and is used to install `pre-commit`.

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

The CLI ships bundled Markdown documentation and can export or serve the same
pages as static HTML:

```sh
cargo run -- docs list
cargo run -- docs show quickstart
cargo run -- docs export --out site
cargo run -- docs serve
```

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.
