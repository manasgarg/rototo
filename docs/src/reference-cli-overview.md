# CLI Overview Reference

The rototo CLI is where engineers, agents, CI, and release tooling work with
packages. It does not replace the SDK in an application process; it gives
those tools a consistent way to inspect, lint, and resolve the same package
an app will load.

## Command Groups

```text
rototo <command> [options]
```

Package commands:

| Command | Purpose |
| --- | --- |
| `init` | Create package and entity templates. |
| `fixtures` | Generate readable runtime behavior fixtures. |
| `lint` | Validate a package or selected targets. |
| `inspect` | Explain how rototo sees package data. |
| `show` | Display package config, variables, qualifiers, catalogs, and lint metadata. |
| `resolve` | Evaluate variables or qualifiers with runtime context. |

Utility commands:

| Command | Purpose |
| --- | --- |
| `docs` | Read or export bundled documentation. |
| `lsp` | Run the language server over stdio. |
| `completions` | Generate shell completion scripts. |

See [CLI Commands](reference-cli-commands.html) for exact command forms and
option placement.

## Global Options

| Option | Meaning |
| --- | --- |
| `--json` | Emit machine-readable JSON when the command supports it. |
| `--quiet`, `-q` | Suppress successful lint output. Diagnostics are still printed. |
| `--package-token <token>` | Bearer token for HTTPS archive package downloads. |
| `-V`, `--version` | Print CLI version. |
| `-h`, `--help` | Print help. |

`--package-token` can also be supplied with `ROTOTO_PACKAGE_TOKEN`.

Global options are accepted at every command level.

## Package Source Argument

Most package commands accept an optional `PACKAGE_SOURCE`:

```sh
rototo lint examples/basic
rototo show git+https://github.com/acme/config.git#main --variables
```

When omitted, rototo searches upward from the current directory for
`rototo-package.toml`.

See [Package Sources](reference-package-sources.html) for supported source
forms.

## Selectors

`lint`, `inspect`, and `show` share selectors:

```text
--variable <ID>        --variables
--catalog <ID>        --catalogs
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

See [Resolve Context](reference-context.html).

## Exit Codes

Successful commands return exit code `0`.

`lint` returns a non-zero exit code when selected lint output contains an error
diagnostic. Warnings do not fail lint by themselves.

Parse errors, unknown selectors, unsupported package sources, and resolution
errors return non-zero exit codes.

## Choosing A Command

Use `show` when you need the configured files or
[diagnostic catalog](reference-diagnostics.html).

Use `inspect` when you need dependencies, consumers, runtime availability, or a
trace attached to package structure.

Use `resolve` when you need the exact runtime value for a context.

Use `lint` in pre-commit, CI, and release checks.
