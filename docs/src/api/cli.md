# rototo CLI reference

## Global Flags

- `--json`: emit machine-readable JSON where supported.
- `--quiet`: suppress success output from lint commands.
- `--workspace-token`: bearer token for `https://` workspace archive sources.
  Can also be set with `ROTOTO_WORKSPACE_TOKEN`.
- `--version`, `-V`: print the rototo version.
- `--help`, `-h`: print command help.

## Workspace Commands

Workspace commands take the workspace source as their first positional
argument. If it is omitted, rototo walks up from the current directory until it
finds `rototo-workspace.toml`.

- `rototo lint [workspace] [selectors]`
- `rototo inspect [workspace] [selectors]`
- `rototo show [workspace] [selectors]`
- `rototo resolve [workspace] [selectors] --context <context>...`

Workspace inputs can be local paths, `file://` URIs, `git+file://`,
`git+https://`, `git+ssh://`, or `https://` archive URLs. Plain `http://`
sources are rejected. Git sources support `#ref:subdir`; archive URLs support
`#:subdir`.

## Selectors

- `--variable <id>`: select one variable. Repeatable.
- `--variables`: select all variables.
- `--resource <id>`: select one resource. Repeatable for `lint`, `inspect`,
  and `show`.
- `--resources`: select all resources for `lint`, `inspect`, and `show`.
- `--qualifier <id>`: select one qualifier. Repeatable.
- `--qualifiers`: select all qualifiers.
- `--lint-rule <authority/rule>`: select one diagnostic rule. Repeatable.
- `--lint-rules`: select all diagnostic rules.
- `--lint-authority <authority>`: select one lint authority. Repeatable.
- `--lint-authorities`: select all lint authorities.
- `--linter <id>`: select one workspace Lua linter. Repeatable.
- `--linters`: select all workspace Lua linters.

When no selector is provided, `lint`, `inspect`, and `show` operate at
workspace level. `resolve` requires at least one variable or qualifier selector;
resources are selected through variables during resolution.

## Resolution Context

Resolution commands accept repeatable `--context` inputs. Each value can be a
JSON object, `@path/to/context.json`, or `path=value`; later inputs override
earlier ones. Qualifiers are resolved against that context. Variables also
require `--env <environment>`.

## Examples

```sh
rototo resolve ./workspace --variable llm-agent-config --env prod \
  --context '{"user":{"tier":"premium"}}'

rototo resolve ./workspace --variables --env prod \
  --context @context.json

rototo resolve ./workspace --qualifier enterprise-accounts \
  --context account.plan=enterprise --context account.seats=250

rototo lint git+https://github.com/acme/config.git#main:rototo

ROTOTO_WORKSPACE_TOKEN=secret rototo inspect \
  https://example.com/rototo-workspace.tar.gz#:workspace
```

## Documentation Commands

- `rototo docs`: list bundled documentation pages in sidebar order.
- `rototo docs -p <page-prefix>`: render a bundled page.
- `rototo docs -s <regex>`: search bundled pages with a regular expression.
- `rototo docs --export [out-dir]`: export bundled pages as a static HTML site.

Internal documentation links rendered by the CLI are printed as
`rototo docs -p <page-id>` references.

## Diagnostic Catalog

Diagnostics are shown through the lint-rule selectors:

- `rototo show --lint-rules`
- `rototo show --lint-rule rototo/variable-unknown-type`
- `rototo show ./config --lint-rule payments/max-token-budget`

Without a workspace source, rototo lists built-in diagnostic rules. With a
workspace source, the catalog also includes custom rules declared by that
workspace.

## Utility Commands

- `rototo lsp`
- `rototo completions bash`
- `rototo completions elvish`
- `rototo completions fish`
- `rototo completions power-shell`
- `rototo completions zsh`

Use `rototo <command> --help` for command syntax. Use `rototo docs` for
concepts, references, and search.

## Exit Policy

- `0`: command succeeded.
- `1`: lint found diagnostics, the command was used incorrectly, or rototo
  could not complete the request.
