# Changelog

All notable changes to rototo are documented here.

## 0.1.0-alpha.6

- Added the rototo console as its own product (`@rototo/console`, in
  `apps/console-server` and `apps/console-web`): change sets over the GitHub
  API, surfaces with a floor renderer and the table and flags experiences,
  the three-delta review, OIDC sign-in with enrollment and grants, and a
  GitHub App write path for stakeholders without GitHub accounts. The
  rototo binary carries no console; there is no `rototo console` command.
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
