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

## Releasing

Releases are maintainer-only. The crate release is handled by the GitHub
`Release` workflow in `.github/workflows/release.yml`; it runs when a tag named
`v*.*.*` is pushed.

Before tagging a release:

1. Update the package version in `Cargo.toml`.
2. Update `Cargo.lock` so the `rototo` package entry has the same version.
3. Update version snippets in public docs and examples, especially
   `README.md`, `docs/src/getting-started.md`, and
   `examples/sdk-app/Cargo.toml`.
4. Add a `CHANGELOG.md` entry for the new version.
5. Run `just check`.
6. Commit the version bump and documentation updates.
7. Merge the commit to `main`.

After `main` is updated, confirm the Cloudflare Pages workflow completed. That
workflow publishes the public docs site from `main`.

Then tag the exact commit on `main`:

```sh
git checkout main
git pull --ff-only origin main
git tag v0.1.0-alpha.3
git push origin v0.1.0-alpha.3
```

The pushed tag starts the release workflow. The workflow runs
`cargo publish --dry-run --locked`, authenticates to crates.io, then runs
`cargo publish --locked`.

After the workflow finishes, verify:

- the GitHub `Release` workflow completed successfully;
- crates.io shows the new version;
- docs.rs built the new crate documentation;
- the public docs site reflects the merged `main` content.
