# rototo Public Docs Source

This directory contains the public Markdown documentation bundled into the
rototo CLI and exported for the public documentation site.

The publishable pages live under `docs/src/`:

```text
docs/src/
  index.md
```

The CLI embeds the pages registered in `src/docs.rs`. When adding, moving, or
renaming a page, update that registry and the bundled documentation list in
`docs/src/index.md`.

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
rototo docs -p index
rototo docs -s "workspace source"
rototo docs --export site
```

Use `just docs-preview` when you need to review the rendered site over HTTPS
before merging. The recipe exports the current workspace, deploys it to a
Cloudflare Pages preview branch, and leaves the production `main` deployment to
the GitHub workflow. It requires `CLOUDFLARE_ACCOUNT_ID` and
`CLOUDFLARE_API_TOKEN`; `CLOUDFLARE_PAGES_PROJECT` defaults to `rototo-docs`.

Maintainer-only process documentation belongs in `internal-docs/`.
