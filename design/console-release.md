# Console release and distribution

Status: implemented for 0.1.0-alpha.7. The publish automation described
here is wired: the `private` flag is gone, `release.yml` carries the
`console-native` build matrix and the assemble-and-publish steps with the
tarball-shape assertion, and the console `package.json` versions are
release-prep and release-check surfaces. The decisions and mechanics below
remain the reference for how and why it works this way. It reuses the SDK
release model wherever it can rather than inventing a second one.

## What ships

One npm package, `rototo-console`, and nothing else. The console is a Node
product, not a compiled binary: the "embedded-SPA single binary" was
consciously dropped at C7 (`console-implementation-plan.md`).

The published tarball carries everything in `files`
(`apps/console-server/package.json`):

- `bin/rototo-console.mjs`: the executable named by `bin.rototo-console`.
  It imports `../dist/main.js` when the compiled output is present (the
  published package) and falls back to `../src/main.ts` in a repo checkout.
- `dist`: the server, compiled by `tsc -p tsconfig.build.json` (the
  `prepack` script, so `npm pack` and `npm publish` always build it).
- `web`: the built React SPA, staged by `npm run stage:web` (build
  `console-web`, copy its `dist` into `web/`). It is a build artifact, git
  ignored, so it must be staged before packing or the package serves
  API-only.
- `assets`: static server assets.
- `*.node`: the native bindings, one file per platform (see below).

The Rust core is reached through the napi crate `rototo-console-native`
(binary name `rototo-console-native`, a workspace member). At runtime
`src/native.ts` loads `rototo-console-native.<platform>-<arch>.node` from the
package root. TypeScript resolves git refs to 40-hex pins before it crosses
the boundary; the native layer only ever sees pins.

## How users run it

```sh
npx rototo-console
```

Node 24 or newer, no other prerequisite. With an empty environment the
server boots in local mode: no login, `127.0.0.1:7687`, trusting the
workstation's ambient GitHub token, serving both the JSON API and the web
app from one process. Runtime shape comes from flags (`--host`, `--port`,
`--data-dir`, `--public-url`, `--web`); auth and secrets come from the
environment variables documented in `apps/console-server/.env.example`.
Setting GitHub OAuth or OIDC flips the server into team mode.

For a hosted deployment, a user puts TLS in front of the process with a
reverse proxy and points a public hostname at it. The `dev.rototo.dev` and
`demo.rototo.dev` Caddy blocks are the reference for that shape: terminate
TLS, reverse-proxy to the console port.

## Decisions

1. **One package, native libraries bundled.** All supported platforms' `.node`
   files go into the single `rototo-console` tarball, exactly as the
   TypeScript SDK does it: a per-target build matrix produces one `.node`
   each, and the publish job gathers them with `download-artifact` plus
   `merge-multiple` before `npm publish`. We do not split into per-platform
   optional packages. Reason: the SDK already proves the bundled path, it
   avoids maintaining a second packaging idiom, and four native libraries is
   an acceptable tarball weight for a server users install once. If the
   tarball weight or an unsupported install platform ever forces the issue,
   the `optionalDependencies` split is the recorded fallback, not the
   starting point.

2. **The server ships compiled.** The first published tarball shipped `src`
   on the theory that Node 24 type-strips it at runtime, and `npx
   rototo-console` crashed on arrival: Node refuses to type-strip anything
   under `node_modules` (`ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING`), a
   deliberate restriction with no opt-out. So the package carries `dist/`,
   emitted by `tsc` with `rewriteRelativeImportExtensions` (the sources
   import with `.ts` extensions), and development is unchanged: `npm run
   dev` still runs `src` directly, and the repo checkout's bin falls back
   to source.

3. **Same tag, same cadence as the SDKs.** The console publishes from the
   same `v<version>` tag that publishes the crate and the language SDKs, on
   the default `latest` dist-tag. `npx rototo-console` resolves `latest`,
   every pre-stable release is the newest thing there is, and npm's OIDC
   trusted publishing carries no credential for a separate dist-tag call,
   so `latest` moves with the publish itself (an earlier draft said a
   shared `alpha` dist-tag; that left `latest` stranded on old versions).
   It is one more job in `release.yml`, not a separate pipeline.

4. **Platform set is the console's own.** The console targets Linux
   x64/arm64 and macOS x64/arm64 (`napi.targets` in its manifest). It does
   not target Windows, even though the SDK does. Reason: the console is a
   hosted server product; a first-class Windows server build waits for
   demand. This is a real difference from the SDK's five targets and is
   intentional.

## Native binaries: the crux

This is the one part that a naive `npm publish` gets wrong today. `files`
lists `*.node`, which bundles whatever native library happens to sit in the
package directory at pack time. But `napi build --platform` builds only the
host architecture, so publishing from one machine ships a single-arch
package that fails to load everywhere else. `src/native.ts` would throw its
"failed to load the rototo-console-native module" error on every other
platform.

The fix is the SDK's matrix, retargeted at the console crate:

- A build job with a four-entry matrix (the console's `napi.targets`), each
  running `npm ci` and `npm run build:native -- --target <target>` in
  `apps/console-server`, uploading `rototo-console-native.*.node` as a
  per-target artifact.
- The publish job downloads all four with `merge-multiple: true` into
  `apps/console-server`, so every platform's `.node` is present before the
  package is packed.

## The release job

Added to `release.yml`, gated on `validate` like the others:

1. `console-native` (matrix over the four targets): build each `.node`,
   upload as `console-native-<artifact>`.
2. In the existing `publish` job, after the SDK steps:
   - download `console-native-*` with `merge-multiple` into
     `apps/console-server`;
   - `npm ci` and `npm run stage:web` to place the built SPA in `web/`;
   - `npm pack --dry-run` to confirm the tarball contains `bin`,
     `dist/main.js`, `web/index.html`, and all four `.node` files (the
     `prepack` script compiles `dist/` on every pack and publish);
   - `npm publish --provenance --access public --tag latest`.

`stage:web` in the pipeline is not optional. A published package with an
empty `web/` is an API-only server that looks broken to anyone expecting the
console UI, so the `pack --dry-run` check should assert `web/index.html` is
present and fail the release if it is not.

## Version and the same-tag release

The console already carries the canonical version (`0.1.0-alpha.6`), the
same string as the crate and the SDKs. To keep it from drifting, its
`package.json` version becomes one of the surfaces that `just release-prep`
rewrites and `just release-check` validates, alongside the existing crate,
docs, and SDK version surfaces. Registry-normalized spellings do not apply
here: npm takes SemVer as-is, so `0.1.0-alpha.6` is both the canonical and
the published version.

## What must change to ship

A checklist, each item small and independently verifiable:

1. Remove `"private": true` from `apps/console-server/package.json` (or
   publish with `--access public`, which the plan already assumes).
2. Add the `console-native` build matrix job to `release.yml`.
3. Add the console assemble-and-publish steps to the `publish` job, after
   the SDK steps.
4. Make `stage:web` part of the release assembly, with a `pack --dry-run`
   assertion that `web/index.html` shipped.
5. Add the console `package.json` version to the `release-check` and
   `release-prep` version surfaces.
6. Confirm `apps/console-server` has a committed `package-lock.json` for the
   matrix `npm ci` (it does, via `just _install-console-server-deps`).

None of these blocks the others except that the publish step (3) depends on
the native matrix (2).

## Deferred and out of scope

- **Per-platform optional packages.** Recorded fallback under Decision 1;
  not the starting shape.
- **Windows.** Trigger is real demand for a Windows server host.
- **A container image.** A published Docker image may become the primary
  hosted-deploy artifact later; for now the reverse-proxy-in-front pattern
  covers it. Trigger: a hosted deployment story that wants a container as
  the unit of deploy.
- **A standalone executable (Node SEA or similar).** The single-binary shape
  was dropped at C7 and stays dropped unless the product model reopens it.
- **Auto-update.** Users get new versions by rerunning `npx rototo-console`
  or bumping a pinned version; the server does not self-update.

## Open questions

- Settled while wiring: publishes land on the default `latest` dist-tag
  (Decision 3). Revisit a separate console dist-tag only if the console
  needs to ship a fix between SDK releases.
- Should the hosted-deploy story ship a reference `Caddyfile` (or compose
  file) in the package or docs, given that TLS termination is external? The
  `dev`/`demo` blocks are the working reference; promoting one into the repo
  is cheap when the deploy docs are written.
