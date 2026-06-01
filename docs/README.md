# rototo Public Docs Source

This directory contains the public Markdown documentation bundled into the
rototo CLI and exported for the public documentation site.

The publishable pages live under `docs/src/`:

```text
docs/src/
  index.md
  concepts/
  tutorials/
  how-to/
  examples/
  reference/
  api/
```

The CLI embeds the pages registered in `src/docs.rs`. When adding, moving, or
renaming a page, update that registry and the bundled documentation list in
`docs/src/index.md`.

Use the CLI to inspect the bundled docs:

```sh
rototo docs
rototo docs -p quickstart
rototo docs -s "workspace source"
rototo docs --export site
```

Use `just docs-preview` when you need to review the rendered site over HTTPS
before merging. The recipe exports the current workspace, deploys it to a
Cloudflare Pages preview branch, and leaves the production `main` deployment to
the GitHub workflow. It requires `CLOUDFLARE_ACCOUNT_ID` and
`CLOUDFLARE_API_TOKEN`; `CLOUDFLARE_PAGES_PROJECT` defaults to `rototo-docs`.

Maintainer-only process documentation belongs in `internal-docs/`.
