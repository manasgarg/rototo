# CLI Commands Reference

The CLI is how people and automation touch a package before an application
does. I use it for four jobs: create the files rototo understands, prove the
package is valid, explain what the runtime model will do, and export the docs
that describe that contract.

For exact parser output, use `rototo <command> --help` with the binary you are
running.

## `rototo init`

```sh
rototo init <PACKAGE> [--qualifier ID] [--variable ID] [--catalog ID] [--context] [--force] [--dry-run]
```

Use `init` when you want rototo to create the package paths and templates it
expects instead of hand-writing the first file from memory. It creates a local
package or adds templates to an existing package.

| Option | Meaning |
| --- | --- |
| `--qualifier <ID>` | Create `qualifiers/<ID>.toml`. |
| `--variable <ID>` | Create `variables/<ID>.toml`. |
| `--catalog <ID>` | Create `catalogs/<ID>.schema.json` and a default entry template. |
| `--context` | Create or infer `request-contexts/request.schema.json`. |
| `--force` | Overwrite files the command creates. |
| `--dry-run` | Print planned writes without changing files. |

## `rototo fixtures`

```sh
rototo fixtures <PACKAGE_SOURCE> --out <DIR> [selectors]
```

Use `fixtures` when a runtime behavior should become a reviewable test case.
It generates readable TOML files for selected
[variables](reference-variables.html) and
[qualifiers](reference-qualifiers.html), so CI can preserve expected selections
for important contexts.

Selectors:

```text
--variable <ID>  --variables
--qualifier <ID> --qualifiers
```

## `rototo lint`

```sh
rototo lint [PACKAGE_SOURCE] [selectors]
```

Use `lint` before a package can be trusted by an app, CI job, or reviewer. It
validates the package or selected targets. Without selectors, lint reports
package-wide diagnostics. With selectors, lint filters output to selected
variables, catalogs, qualifiers, lint rules, authorities, or Lua linters.

`--quiet` suppresses the successful `ok:` line, but diagnostics still print.

## `rototo inspect`

```sh
rototo inspect [PACKAGE_SOURCE] [selectors] [--context CONTEXT]
```

Use `inspect` when you need to understand how rototo sees the package after
source loading and layering. It explains the projected package: documents,
runtime status, diagnostics, catalogs, variables, qualifiers, lint
rules, lint authorities, and linters.

When context is supplied, selected variables and qualifiers include resolution
traces against that object. When no context is supplied, inspect uses stored
sample entries from compatible request contexts.

## `rototo diff`

```sh
rototo diff BEFORE_PACKAGE_SOURCE AFTER_PACKAGE_SOURCE [--context CONTEXT]
```

Use `diff` when you need to understand what changed in rototo terms rather than
as raw TOML or JSON. It compares projected package entities such as variables,
values, resolve rules, qualifiers, catalogs, catalog values, and
request contexts.

When context is supplied, `diff` also reports resolution impact for variables
whose selected value changes between the before and after packages.

## `rototo show`

```sh
rototo show [PACKAGE_SOURCE] [selectors]
```

Use `show` when you need configured files and catalog metadata, not runtime
evaluation. Without selectors it prints a package inventory. With `--json`,
it returns a structured view for the selected targets.

When no package source is supplied and only lint catalog selectors are used,
`show` reads the global built-in diagnostic catalog.

Examples:

```sh
rototo show --lint-rules
rototo show examples/basic --variable account-limits
rototo show examples/basic --lint-authority rototo
```

## `rototo resolve`

```sh
rototo resolve [PACKAGE_SOURCE] [--variable ID | --variables | --qualifier ID | --qualifiers] [--context CONTEXT]
```

Use `resolve` when you want to see the value an application would receive for a
specific context. At least one variable or qualifier selector is required.

`--context` is repeatable. When omitted, rototo resolves selected targets
against stored sample entries from compatible request contexts.

Use `--json` for stable traces. See
[Resolution Output](reference-resolution-output.html).

## `rototo docs`

```sh
rototo docs
rototo docs -p <PAGE_PREFIX>
rototo docs -s <REGEX>
rototo docs --export [OUT_DIR]
rototo docs --package-readme <python|typescript> --out <FILE> [--docs-base-url URL]
```

Use `docs` when you need the documentation bundled with the current binary. It
lists pages, renders one Markdown page, searches docs with a regular
expression, exports the static HTML site, or generates packaged SDK README
content from the SDK reference pages. `--export` defaults to `site` when no
directory is supplied. `--docs-base-url` controls the public docs host used for
internal links in generated package READMEs and defaults to
`https://docs.rototo.dev`.

## `rototo console`

```sh
rototo console
rototo console --bind <ADDR> [--public-url <URL>] [--data-dir <DIR>]
rototo console --package <PACKAGE_SOURCE> --write disabled
rototo console --package <PACKAGE_SOURCE> --write pull-request
rototo console --package <PACKAGE_SOURCE> --write direct-push
```

Use `console` to serve the rototo web console and its JSON API from this
binary. With no OAuth secret configured it runs as a local deployment on
`http://127.0.0.1:7686`: no browser sign-in, using git config identity for
local packages and an ambient GitHub token (`--package-token` /
`ROTOTO_PACKAGE_TOKEN`, a stored device-flow sign-in, or `gh auth token`)
when GitHub credentials are needed. Setting `ROTOTO_GITHUB_CLIENT_ID` and
`ROTOTO_GITHUB_CLIENT_SECRET` switches it to hosted deployment with GitHub
OAuth sign-in; `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY` is then required so
stored tokens are encrypted at rest. `--package` registers one fixed source
at startup, and `--write` controls whether branch edits are disabled, published
as pull requests, or direct-pushed. See
[Self-Hosting the Console](self-hosting-console.md) for deployment shapes.

## `rototo lsp`

```sh
rototo lsp
```

Use `lsp` when an editor or agent needs live package feedback. It runs the
rototo Language Server Protocol server over stdio for diagnostics,
completions, symbols, hovers, and definitions.

## `rototo completions`

```sh
rototo completions <SHELL>
```

Use `completions` to generate shell completion scripts for local authoring.
Supported shells:

```text
bash
elvish
fish
powershell
zsh
```
