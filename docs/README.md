# rototo Public Docs Source

This directory contains the public Markdown documentation bundled into the
rototo CLI and exported for the public documentation site.

The publishable pages live under `docs/src/`:

```text
docs/src/
  motivation.md
  quickstart.md
  concepts.md
  adoption.md
  cli.md
  package-format.md
  package-sources.md
  expressions.md
  diagnostics.md
  sdk.md
```

The CLI embeds the pages registered in `src/docs.rs`. When adding, moving, or
renaming a page, update that registry and `DOC_NAV_SECTIONS` in the same file.
The consistency tests require every Markdown file under `docs/src/` to be
registered and listed in navigation exactly once.

Each language SDK's `README.md` is generated from the root `README.md` (the Rust
SDK's README): shared prose and the CLI walkthrough are copied verbatim, and the
title plus the per-language runtime example are swapped in. Regenerate with
`rototo docs --package-readme <sdk> --out sdks/<sdk>/README.md`. The
`package_readmes_are_generated_from_rust_readme` test keeps the committed files
in sync, so edit the root `README.md` (and the quickstart snippet it reuses),
not the generated SDK files.

## Writing Voice

Write every public doc in plain, everyday language, the way one engineer would
explain it to another over coffee. No jargon, no dense paragraphs, no spec-speak.
This applies to reference pages too: they still have to be exact, but "exact" is
not the same as "dry". `docs/src/package-sources.md` is the reference example of
this tone; match it. Never use em-dashes (`—`); use a comma, colon, parentheses,
or a spaced hyphen instead. The full guidance lives in `AGENTS.md`.

Before finishing a docs change, check that the page:

- starts from an operational problem;
- explains why each concept exists before showing syntax;
- gives reference contracts a production reason before listing fields,
  commands, or JSON shapes;
- adds a short causal transition before long command or file-edit sequences;
- uses first person only when it communicates engineering judgment;
- avoids marketing adjectives and ambiguous rollout vocabulary;
- keeps examples runnable against the current CLI and SDK.

The exported site's design system lives under `docs/theme/`:

```text
docs/theme/
  rototo-docs.css        design tokens and page styling (paper/ink/clay palette)
  favicon.svg
  rototo-wordmark.svg
```

`rototo-docs.css` is the docs-site application of the rototo design system
(warm paper surfaces, clay accent, Manrope/Hanken Grotesk/JetBrains Mono type).
The exporter embeds these files via `include_str!` and writes them to the
site's `assets/` directory. Page HTML structure (topbar, side navigation,
breadcrumb, previous/next links) and code syntax highlighting are generated in
`src/docs.rs`. Brand fonts load from the Google Fonts CDN at view time.

Use the CLI to inspect the bundled docs:

```sh
rototo docs
rototo docs -p motivation
rototo docs -s "configuration"
rototo docs --export site
```

Use `just docs-preview` when you need to review the rendered site over HTTPS
before merging. The recipe exports the current package, deploys it to a
Cloudflare Pages preview branch, and leaves the production `main` deployment to
the GitHub workflow. It requires `CLOUDFLARE_ACCOUNT_ID` and
`CLOUDFLARE_API_TOKEN`; `CLOUDFLARE_PAGES_PROJECT` defaults to `rototo-docs`.

Maintainer-only process documentation belongs in `internal-docs/`.
