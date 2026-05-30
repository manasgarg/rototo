# Diagnostic Reference

Diagnostics explain why rototo could not inspect, lint, load, or resolve a
workspace.

Use the CLI diagnostic catalog as the source of truth:

```sh
rototo diagnostics list
rototo diagnostics get rototo/variable-unknown-type
rototo diagnostics get payments/max-token-budget --workspace ./config
```

Every emitted lint diagnostic has these fields:

- `rule`: stable diagnostic identity in `<authority>/<rule-id>` form.
- `severity`: currently `error`.
- `path`: workspace-relative file path when rototo can attach the diagnostic to a file.
- `message`: concrete failure message.
- `help`: recovery guidance from the built-in rule or declared custom rule.

Built-in rules use the reserved `rototo` authority and flat rule ids, such as
`rototo/qualifier-predicate-unknown-op`. Custom Lua lint rules use a
non-`rototo` authority declared in the variable file.

Use JSON output for scripts and CI annotations:

```sh
rototo diagnostics list --json
rototo diagnostics get rototo/variable-unknown-type --json
```
