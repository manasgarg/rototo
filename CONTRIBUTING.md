# Contributing to rototo


## Setup

Install `mise` and `just`, then run:

```sh
mise trust
just setup
```

Rust is pinned by `rust-toolchain.toml`. Python is pinned by `.tool-versions`
and is used to install `pre-commit`.

## Checks

Run the local gate before pushing:

```sh
just check
```

`just check` runs formatting checks, clippy with warnings denied, and the test
suite.
