# Changelog

All notable changes to rototo are documented here.

## Unreleased

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
