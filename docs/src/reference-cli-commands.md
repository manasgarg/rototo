# CLI Commands Reference

The CLI is how people and automation touch a workspace before an application
does. I use it for four jobs: create the files rototo understands, prove the
workspace is valid, explain what the runtime model will do, and export the docs
that describe that contract.

For exact parser output, use `rototo <command> --help` with the binary you are
running.

## `rototo init`

```sh
rototo init <WORKSPACE> [--qualifier ID] [--variable ID] [--resource ID] [--context] [--force] [--dry-run]
```

Use `init` when you want rototo to create the workspace paths and templates it
expects instead of hand-writing the first file from memory. It creates a local
workspace or adds templates to an existing workspace.

| Option | Meaning |
| --- | --- |
| `--qualifier <ID>` | Create `qualifiers/<ID>.toml`. |
| `--variable <ID>` | Create `variables/<ID>.toml`. |
| `--resource <ID>` | Create `resources/<ID>.toml` and a schema template. |
| `--context` | Create or infer `schemas/context.schema.json`. |
| `--force` | Overwrite files the command creates. |
| `--dry-run` | Print planned writes without changing files. |

## `rototo fixtures`

```sh
rototo fixtures <WORKSPACE_SOURCE> --out <DIR> [selectors]
```

Use `fixtures` when a runtime behavior should become a reviewable test case.
It generates readable TOML files for selected variables and qualifiers, so CI
can preserve expected selections for important contexts.

Selectors:

```text
--variable <ID>  --variables
--qualifier <ID> --qualifiers
```

## `rototo lint`

```sh
rototo lint [WORKSPACE_SOURCE] [selectors]
```

Use `lint` before a workspace can be trusted by an app, CI job, or reviewer. It
validates the workspace or selected targets. Without selectors, lint reports
workspace-wide diagnostics. With selectors, lint filters output to selected
variables, resources, qualifiers, lint rules, authorities, or Lua linters.

`--quiet` suppresses the successful `ok:` line, but diagnostics still print.

## `rototo inspect`

```sh
rototo inspect [WORKSPACE_SOURCE] [selectors] [--context CONTEXT]
```

Use `inspect` when you need to understand how rototo sees the workspace after
source loading and layering. It explains the projected workspace: documents,
runtime status, diagnostics, schemas, resources, variables, qualifiers, lint
rules, lint authorities, and linters.

When context is supplied, selected variables and qualifiers include resolution
traces.

## `rototo show`

```sh
rototo show [WORKSPACE_SOURCE] [selectors]
```

Use `show` when you need configured files and catalog metadata, not runtime
evaluation. Without selectors it prints a workspace inventory. With `--json`,
it returns a structured view for the selected targets.

When no workspace source is supplied and only lint catalog selectors are used,
`show` reads the global built-in diagnostic catalog.

Examples:

```sh
rototo show --lint-rules
rototo show examples/basic --variable account-limits
rototo show examples/basic --lint-authority rototo
```

## `rototo resolve`

```sh
rototo resolve [WORKSPACE_SOURCE] [--variable ID | --variables | --qualifier ID | --qualifiers] [--context CONTEXT]
```

Use `resolve` when you want to see the value an application would receive for a
specific context. At least one variable or qualifier selector is required.

`--context` is repeatable. When omitted, rototo uses `{}`.

Use `--json` for stable traces. See `reference-resolution-output`.

## `rototo docs`

```sh
rototo docs
rototo docs -p <PAGE_PREFIX>
rototo docs -s <REGEX>
rototo docs --export [OUT_DIR]
```

Use `docs` when you need the documentation bundled with the current binary. It
lists pages, renders one Markdown page, searches docs with a regular
expression, or exports the static HTML site. `--export` defaults to `site` when
no directory is supplied.

## `rototo lsp`

```sh
rototo lsp
```

Use `lsp` when an editor or agent needs live workspace feedback. It runs the
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
