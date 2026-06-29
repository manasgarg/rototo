# Contributing to rototo

## Setup

Install `mise` and `just`, then run:

```sh
mise trust
just setup
```

Rust is pinned by `rust-toolchain.toml`. Python is pinned by `.tool-versions`
and is used to install `pre-commit`. `just setup` is safe to rerun; it installs
the pinned local tools, console dependencies, and local pre-commit/pre-push
hooks.

To check the local environment without changing it, run:

```sh
just doctor
```

`just doctor` verifies the tools the normal development loop expects: Rust,
`cargo`, `just`, `mise`, Python, `pre-commit`, Node/npm, `cargo-watch`, and
the console dependencies. Optional tools such as `gh`, Go, Java, Maven, and
`sqlite3` are reported as warnings when they are missing.

Useful environment variables:

- `ROTOTO_PACKAGE_TOKEN` or `--package-token`: GitHub/API token for
  package source loading and local console auth.
- `ROTOTO_CONSOLE_DEV_PUBLIC_URL`: public URL used by `just console-dev`;
  defaults to `https://dev.rototo.dev`.
- `ROTOTO_CONSOLE_DEV_OBSERVABILITY`: local directory for console dev telemetry;
  defaults to `.rototo/dev/observability` when using `just console-dev`.
- `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY`: required for team-mode console token
  encryption.
- `ROTOTO_GITHUB_CLIENT_ID` and `ROTOTO_GITHUB_CLIENT_SECRET`: enable the
  console GitHub OAuth web flow.

## Checks

Run the local gate before pushing:

```sh
just check
```

`just check` runs the same lifecycle as CI: formatting checks, development
linters, Rust tests, console checks, SDK contract/package smoke tests, and the
Java package shape check when Maven is available. It prints the slowest major
steps at the end of the run.

The smaller commands are useful while iterating:

```sh
just fmt              # rewrite Rust and Go formatting; frontend formatters run if present
just fmt-check        # verify formatting and generated package README freshness
just lint             # clippy, console typecheck, and repository vocabulary checks
just test-rust        # Rust tests only
just test-console     # console typecheck and bundle build
just test-sdk-python  # Python SDK contract and smoke tests
just test-sdk-typescript
just test-sdk-java
just test-sdk-go
```

`rototo lint` is product behavior and is covered by tests. `just lint` is for
development-time static analysis and repository policy checks.

When local generated artifacts get in the way, run:

```sh
just clean-dev
```

That command only removes allowlisted generated development paths such as
`.rototo/dev`, console build output, and SDK build directories. It does not
touch source fixtures, staged work, or `.git`.

## Console Development

Run the full local console stack with:

```sh
just console-dev
```

That starts the Rust API through `cargo-watch`, waits for `/api/me`, then starts
the Vite UI. The command enables dev observability and writes local files under
`.rototo/dev/observability/`:

- `console-api.ndjson`: structured API request and operation events.
- `console-ui.ndjson`: browser-side API timing, route load, and error events.
- `console-dev.log`: raw API and Vite process output.
- `console-observe-summary.json`: latest summarized findings from
  `just console-observe`.

Summarize what happened after exercising the console:

```sh
just console-observe
```

For a live view while interacting with the UI:

```sh
just console-observe-watch
```

To fail a local check when observability has actionable findings above the
configured thresholds:

```sh
just console-observe-check
```

Common console failures:

- API never becomes ready: check `.rototo/dev/observability/console-dev.log`
  for Rust compile errors or port conflicts on `127.0.0.1:7686`.
- UI cannot reach the API: confirm `just console-dev` is still running and that
  the browser is using the same origin expected by `ROTOTO_CONSOLE_DEV_PUBLIC_URL`.
- GitHub operations fail: verify `ROTOTO_PACKAGE_TOKEN`, the stored
  device-flow sign-in, or `gh auth token`.
- OAuth/team mode fails: verify `ROTOTO_GITHUB_CLIENT_ID`,
  `ROTOTO_GITHUB_CLIENT_SECRET`, and `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY`.

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

Before tagging, maintainers should also run:

```sh
just release-check 0.1.0-alpha.3
just release-package-dry-run 0.1.0-alpha.3
```

`release-check` verifies canonical version surfaces and generated SDK package
README freshness. `release-package-dry-run` builds publishable package shapes
where local tooling is available and prints post-publish smoke links.

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
- the public docs site reflects the merged `main` content;
- PyPI, npm, Maven Central, and the Go module page show the new version when
  those SDKs are part of the release.
