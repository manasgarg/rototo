# rototo CLI reference

## Global Flags

- `--json`: emit machine-readable JSON where supported.
- `--quiet`, `-q`: suppress success output from lint commands.
- `--workspace-token`: bearer token for `https://` workspace archive sources.
  Can also be set with `ROTOTO_WORKSPACE_TOKEN`.
- `--version`: print the rototo version.
- `--help`: print command help.

## Workspace Commands

- `rototo workspace inspect [workspace] [--workspace <workspace>]`
- `rototo workspace lint [workspace] [--workspace <workspace>]`

When `workspace` is omitted, rototo walks up from the current directory until
it finds `rototo-workspace.toml`.
The positional workspace argument is kept for compatibility; `--workspace` is
preferred for consistency with qualifier, variable, and diagnostics commands.
Workspace inputs can be local paths, `file://` URIs, `git+file://`,
`git+https://`, `git+ssh://`, or `https://` archive URLs. Plain `http://`
sources are rejected. Git sources support `#ref:subdir`; archive URLs support
`#:subdir`.

## Qualifier Commands

- `rototo qualifier list [--workspace <workspace>]`
- `rototo qualifier get <id> [--workspace <workspace>]`
- `rototo qualifier lint <id> [--workspace <workspace>]`
- `rototo qualifier resolve <id> --context <context> [--workspace <workspace>]`
- `rototo qualifier resolve-all --context <context> [--workspace <workspace>]`

## Variable Commands

- `rototo variable list [--workspace <workspace>]`
- `rototo variable get <id> [--workspace <workspace>]`
- `rototo variable lint <id> [--workspace <workspace>]`
- `rototo variable resolve <id> --env <env> --context <context> [--workspace <workspace>]`
- `rototo variable resolve-all --env <env> --context <context> [--workspace <workspace>]`

Resolution commands accept repeatable `--context` inputs. Each value can be a
JSON object, `@path/to/context.json`, or `path=value`; later inputs override
earlier ones. Qualifiers are resolved against that context. Variables resolve by
environment, applying matching rules before the environment's fallback value.

## Examples

```sh
rototo variable resolve llm-agent-config --workspace ./workspace --env prod \
  --context '{"user":{"tier":"premium"}}'

rototo variable resolve-all --workspace ./workspace --env prod \
  --context @context.json

rototo qualifier resolve enterprise-accounts --workspace ./workspace \
  --context account.plan=enterprise --context account.seats=250

rototo workspace lint \
  --workspace git+https://github.com/acme/config.git#main:rototo

ROTOTO_WORKSPACE_TOKEN=secret rototo workspace inspect \
  --workspace https://example.com/rototo-workspace.tar.gz#:workspace
```

## Documentation Commands

- `rototo docs list`
- `rototo docs show [page] [--format markdown|html]`
- `rototo docs export --out <directory>`
- `rototo docs serve [--addr <address>]`

## Diagnostic Commands

- `rototo diagnostics list [--workspace <workspace>]`
- `rototo diagnostics get <rule> [--workspace <workspace>]`

Diagnostics are global by default. `--workspace` is optional and only scopes the
catalog subject and includes custom lint rules declared by that workspace.

## Shell Completions

- `rototo completions bash`
- `rototo completions elvish`
- `rototo completions fish`
- `rototo completions power-shell`
- `rototo completions zsh`

## Exit Policy

- `0`: command succeeded.
- `1`: lint found diagnostics, the command was used incorrectly, or rototo
  could not complete the request.
