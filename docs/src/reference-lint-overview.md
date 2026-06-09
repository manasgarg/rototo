# Lint Reference

Lint is the release gate for a rototo workspace. It validates the files as a
control plane before an application loads them and changes runtime behavior.

Rototo lint runs in stages so diagnostics can point at the right failure
boundary: discovery, parse, projection, registration, references, values,
graph rules, and policy.

## Command

```sh
rototo lint [WORKSPACE_SOURCE]
rototo lint [WORKSPACE_SOURCE] --variable account-limits
rototo lint [WORKSPACE_SOURCE] --lint-authority rototo
```

Without selectors, lint reports all workspace diagnostics. With selectors,
lint filters diagnostics to selected targets.

## Exit Behavior

Lint succeeds when there are no error diagnostics. Warning diagnostics are
printed but do not fail the command by themselves.

With `--quiet`, successful lint prints nothing. Diagnostics still print.

With [`--json`](reference-json-output.html), lint returns:

```json
{
  "workspace": "/path/to/workspace",
  "documents": [],
  "diagnostics": []
}
```

## Built-In Coverage

Built-in lint validates:

- manifest presence, TOML syntax, `schema_version = 1`, and `extends` shape;
- qualifier syntax, schema version, predicates, operators, bucket ranges, and
  referenced qualifiers;
- variable syntax, type source, primitive values, resolve defaults, resolve
  rules, resource references, and value references;
- resource syntax, schema references, object schema validation, and
  `x-rototo-resource` references;
- standalone JSON Schema parsing and compilation;
- context schema path declarations and predicate type compatibility;
- custom Lua lint registration and handler execution;
- graph issues such as unreferenced qualifiers, shadowed rules, and unused
  values.

## Stages

| Stage | What it protects |
| --- | --- |
| `discover` | Workspace root and known documents. |
| `parse` | TOML, JSON, and Lua file parsing. |
| `project` | File content shape and required fields. |
| `register` | Custom Lua lint registration. |
| `reference` | Links between variables, qualifiers, resources, schemas, and context paths. |
| `value` | Primitive values, schemas, resource objects, and custom value rules. |
| `graph` | Relationships that are valid syntax but suspicious behavior. |
| `policy` | Custom policy checks. |

Custom lint handlers can run in `project`, `reference`, `value`, `graph`, or
`policy`.

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

Selectors do not change how the workspace is validated. They change which
diagnostics are reported.

## SDK

The SDK exposes:

```rust
rototo::lint_workspace(workspace_root).await?;
rototo::lint_variable(workspace_root, "account-limits").await?;
rototo::lint_qualifier(workspace_root, "paid-account").await?;
rototo::lint_resource(workspace_root, "account-limit-profile").await?;
```

`Workspace::load` also runs lint by default and rejects workspaces with error
diagnostics.

## Custom Policy

Built-in lint knows rototo's structural contract.
[Custom Lua lint](reference-custom-lua-lint.html) handles local policy: naming
conventions, copy rules, allowed account classes, or domain limits only your
team understands.

See [Custom Lua Lint](reference-custom-lua-lint.html).
