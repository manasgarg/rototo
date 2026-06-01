# Diagnostic Reference

Diagnostics explain why rototo could not inspect, lint, load, or resolve a
workspace.

Use the CLI diagnostic catalog as the source of truth:

```sh
rototo show --lint-rules
rototo show --lint-rule rototo/variable-unknown-type
rototo show ./config --lint-rule payments/max-token-budget
```

Every emitted lint diagnostic has these fields:

- `rule`: stable diagnostic identity in `<authority>/<rule-id>` form.
- `severity`: `error` or `warning`. Errors fail lint; warnings are reported but
  do not make lint fail.
- `stage`: lint pipeline stage that produced the diagnostic.
- `entity`: workspace, manifest, qualifier, variable, or schema owner.
- `message`: concrete failure message.
- `help`: recovery guidance from the built-in rule or declared custom rule.
- `location`: workspace-relative path and optional zero-based line/character range.
- `related`: secondary locations when a rule needs them.

Built-in rules use the reserved `rototo` authority and flat rule ids, such as
`rototo/qualifier-predicate-unknown-op`. Custom Lua lint rules use a
non-`rototo` authority declared in the workspace manifest.

Precise locations use LSP-style line and character ranges. Document-only and
workspace-root diagnostics include a path without a range.

Use JSON output for scripts and CI annotations:

```sh
rototo show --lint-rules --json
rototo show --lint-rule rototo/variable-unknown-type --json
```
