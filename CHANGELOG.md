# Changelog

All notable changes to rototo are documented here.

## 0.1.0-alpha.7

- Reworked the package layout around contracts and values: `model/` holds
  catalog and evaluation-context schemas, `data/` holds catalog entries,
  directories namespace ids for every collection, and rototo-recognized ids
  are lowercase snake_case. Schema fields pin their values through
  `x-rototo-ref` targets (`catalog=<id>`, `list=<id>`, or dynamic).
- Added lists (`lists/*.toml`): named closed sets of scalar values, with
  `list=<id>` variable types, a `lists` expression root for membership
  tests, and list-aware validation of catalog entries and context samples.
- Added resolve methods beyond first-match rules: `method = "query"`
  selects catalog entries through a filter/sort pipeline, and
  `method = "allocation"` splits traffic across arms through layers,
  diversions, and bucket predicates.
- Added structured composition: a package `extends` bases, overlays change
  entities through explicit update and deleted markers, `governance.toml`
  grants what overlays may change (deny by default with a `[defaults]`
  block), and traces record which layer produced each resolution.
- Added cross-variable references through the `variables` expression root
  and removed the qualifier concept; a named runtime condition is now a
  bool condition variable that other variables reference.
- Added package reflection to the Rust, Python, TypeScript, Java, and Go
  SDKs: apps follow catalog references explicitly and receive raw entries.
  Enumeration APIs are now `<noun>_ids`; a fallback package source covers
  degraded starts.
- Replaced the in-binary console with `rototo-console`, a standalone Node
  app (`apps/console-server` and `apps/console-web`): change sets as
  branches and pull requests over the GitHub API, a workbench with
  semantic editing and raw TOML editing backed by live lint diagnostics,
  context picking with saved samples and synthesized boundary contexts,
  semantic change review across variables, catalogs, lists, samples, and
  schemas, surfaces with build-time experiences, OIDC sign-in with
  enrollment and grants, a GitHub App write path for stakeholders without
  GitHub accounts, and change-set collaborators added by GitHub login.
  The rototo binary carries no console; there is no `rototo console`
  command.
- The CLI and console follow the XDG base directories for cached and
  stored state. `rototo package` gains an unpacked-directory mode, and
  bearer tokens for HTTPS archive sources scope to URL prefixes.

## 0.1.0-alpha.6

- Added `rototo console`: the web console and its JSON API now ship inside
  the rototo binary, with local (ambient token), team (GitHub OAuth), and
  read-only deployment modes. The former Next.js admin app is replaced by a
  static SPA in `apps/console` embedded into release builds.
- Restructured the public site export: `rototo docs --export` writes the
  rototo.dev homepage at the site root, documentation under `/docs/`, and a
  `_redirects` file for the old URLs. Added the Self-Hosting the Console
  documentation page.

## 0.1.0-alpha.3

- Added package layering, catalogs, `rototo init`, and fixture generation.
- Made resolution without `--context` use empty `{}` context.
- Reworked the bundled and public documentation around examples, adoption
  guidance, reference material, and release process.
- Updated the docs site typography and generated syntax highlighting.

## 0.1.0-alpha.1

- Initial release.
