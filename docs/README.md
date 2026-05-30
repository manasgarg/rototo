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
rototo docs list
rototo docs show quickstart
rototo docs export --out site
rototo docs serve
```

Maintainer-only process documentation belongs in `internal-docs/`.
