# CLI Overview Reference

The rototo CLI is the operator and authoring surface for workspaces. It does
not replace the SDK in an application process; it gives engineers, agents, CI,
and release tooling a consistent way to inspect, lint, and resolve the same
workspace an app will load.

## Command Groups

```text
rototo <command> [options]
```

Workspace commands:

| Command | Purpose |
| --- | --- |
| `init` | Create workspace and entity templates. |
| `fixtures` | Generate readable runtime behavior fixtures. |
| `lint` | Validate a workspace or selected targets. |
| `inspect` | Explain how rototo sees workspace data. |
| `show` | Display workspace config, variables, qualifiers, resources, and lint metadata. |
| `resolve` | Evaluate variables or qualifiers with runtime context. |

Utility commands:

| Command | Purpose |
| --- | --- |
| `docs` | Read or export bundled documentation. |
| `lsp` | Run the language server over stdio. |
| `completions` | Generate shell completion scripts. |

## Global Options

| Option | Meaning |
| --- | --- |
| `--json` | Emit machine-readable JSON when the command supports it. |
| `--quiet`, `-q` | Suppress successful lint output. Diagnostics are still printed. |
| `--workspace-token <token>` | Bearer token for HTTPS archive workspace downloads. |
| `-V`, `--version` | Print CLI version. |
| `-h`, `--help` | Print help. |

`--workspace-token` can also be supplied with `ROTOTO_WORKSPACE_TOKEN`.

Global options are accepted at every command level.

## Workspace Source Argument

Most workspace commands accept an optional `WORKSPACE_SOURCE`:

```sh
rototo lint examples/basic
rototo show git+https://github.com/acme/config.git#main --variables
```

When omitted, rototo searches upward from the current directory for
`rototo-workspace.toml`.

See `reference-workspace-sources` for supported source forms.

## Selectors

`lint`, `inspect`, and `show` share selectors:

```text
--variable <ID>        --variables
--resource <ID>        --resources
--qualifier <ID>       --qualifiers
--lint-rule <ID>       --lint-rules
--lint-authority <ID>  --lint-authorities
--linter <ID>          --linters
```

`resolve` only accepts resolvable targets:

```text
--variable <ID>        --variables
--qualifier <ID>       --qualifiers
```

Selectors can be repeated. Plural selectors select all targets of that kind.

## Context Inputs

`inspect` and `resolve` accept repeatable `--context` inputs. The forms are
JSON object, `@file`, or `path=value`.

For `resolve`, missing context defaults to `{}`. For `inspect`, traces are
included only when context is supplied.

See `reference-context`.

## Exit Codes

Successful commands return exit code `0`.

`lint` returns a non-zero exit code when selected lint output contains an error
diagnostic. Warnings do not fail lint by themselves.

Parse errors, unknown selectors, unsupported workspace sources, and resolution
errors return non-zero exit codes.

## Choosing A Command

Use `show` when you need the configured files or diagnostic catalog.

Use `inspect` when you need dependencies, consumers, runtime availability, or a
trace attached to workspace structure.

Use `resolve` when you need the exact runtime value for a context.

Use `lint` in pre-commit, CI, and release checks.
