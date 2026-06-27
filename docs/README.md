# rototo Public Docs Source

This directory contains the public Markdown documentation bundled into the
rototo CLI and exported for the public documentation site.

The publishable pages live under `docs/src/`:

```text
docs/src/
  motivation.md
  concepts.md
```

The CLI embeds the pages registered in `src/docs.rs`. When adding, moving, or
renaming a page, update that registry and `DOC_NAV_SECTIONS` in the same file.
The consistency tests require every Markdown file under `docs/src/` to be
registered and listed in navigation exactly once.

The current source tree is intentionally small while the public docs are being
rewritten. SDK package README generation is temporarily ignored until the new
SDK reference source exists again.

## Writing Voice

Write public docs in the senior-engineer voice defined in `AGENTS.md`: practical,
experienced, warm, and precise. The docs should feel like an engineer sharing
the production pattern they trust, not a feature catalog.

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
