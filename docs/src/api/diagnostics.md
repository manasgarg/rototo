# Diagnostic Reference

Diagnostics explain why rototo could not inspect, lint, load, or resolve a
workspace.

Use the CLI diagnostic catalog as the source of truth:

```sh
rototo diagnostics list
rototo diagnostics get rototo/workspace-toml-file-parse-failed
rototo diagnostics get rototo/variable-custom-lint-failed
```

Every emitted lint diagnostic has these fields:

- `code`: stable identifier emitted by lint commands.
- `rule`: exact lint rule when the diagnostic comes from a lint rule.
- `source`: `kernel`, `schema`, or `custom`.
- `kind`: `qualifier` or `variable` when the diagnostic belongs to one.
- `title`: short diagnostic name.
- `help`: recovery guidance.

Use JSON output for scripts and CI annotations:

```sh
rototo diagnostics list --json
rototo diagnostics get rototo/workspace-toml-file-parse-failed --json
```
