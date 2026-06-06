# CLI Commands Reference

This page lists the current command contracts. Use `rototo <command> --help`
for the exact parser output shipped with your installed binary.

## `rototo init`

```sh
rototo init <WORKSPACE> [--qualifier ID] [--variable ID] [--resource ID] [--context] [--force] [--dry-run]
```

Creates a local workspace or adds templates to an existing workspace.

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

Generates readable fixture TOML files for selected variables and qualifiers.
Fixtures are useful when CI needs to preserve expected runtime behavior for
important contexts.

Selectors:

```text
--variable <ID>  --variables
--qualifier <ID> --qualifiers
```

## `rototo lint`

```sh
rototo lint [WORKSPACE_SOURCE] [selectors]
```

Validates the workspace or selected targets. Without selectors, lint reports
workspace-wide diagnostics. With selectors, lint filters output to selected
variables, resources, qualifiers, lint rules, authorities, or Lua linters.

`--quiet` suppresses the successful `ok:` line, but diagnostics still print.

## `rototo inspect`

```sh
rototo inspect [WORKSPACE_SOURCE] [selectors] [--context CONTEXT]
```

Explains the projected workspace: documents, runtime status, diagnostics,
schemas, resources, variables, qualifiers, lint rules, lint authorities, and
linters.

When context is supplied, selected variables and qualifiers include resolution
traces.

## `rototo show`

```sh
rototo show [WORKSPACE_SOURCE] [selectors]
```

Displays workspace config and lint metadata. Without selectors it prints a
workspace inventory. With `--json`, it returns a structured view for the
selected targets.

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

Evaluates selected variables or qualifiers. At least one variable or qualifier
selector is required.

`--context` is repeatable. When omitted, rototo uses `{}`.

Use `--json` for stable traces. See `reference-resolution-output`.

## `rototo docs`

```sh
rototo docs
rototo docs -p <PAGE_PREFIX>
rototo docs -s <REGEX>
rototo docs --export [OUT_DIR]
```

Lists bundled docs, renders one Markdown page, searches docs with a regular
expression, or exports the static HTML site. `--export` defaults to `site` when
no directory is supplied.

## `rototo lsp`

```sh
rototo lsp
```

Runs the rototo Language Server Protocol server over stdio. Editors and agents
use it for workspace diagnostics, completions, symbols, hovers, and
definitions.

## `rototo completions`

```sh
rototo completions <SHELL>
```

Supported shells:

```text
bash
elvish
fish
powershell
zsh
```
