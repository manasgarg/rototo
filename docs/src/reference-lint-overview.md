# Lint Reference

Lint is the release gate for a rototo package. It validates the files as a
control plane before an application loads them and changes runtime behavior.

Rototo lint runs in stages so diagnostics can point at the right failure
boundary: discovery, parse, projection, registration, references, values,
graph rules, and policy.

## Command

```sh
rototo lint [PACKAGE_SOURCE]
rototo lint [PACKAGE_SOURCE] --variable account-limits
rototo lint [PACKAGE_SOURCE] --lint-authority rototo
```

Without selectors, lint reports all package diagnostics. With selectors,
lint filters diagnostics to selected targets.

## Exit Behavior

Lint succeeds when there are no error diagnostics. Warning diagnostics are
printed but do not fail the command by themselves.

With `--quiet`, successful lint prints nothing. Diagnostics still print.

With [`--json`](reference-json-output.html), lint returns:

```json
{
  "package": "/path/to/package",
  "documents": [],
  "diagnostics": []
}
```

## Built-In Coverage

Built-in lint validates:

- manifest presence, TOML syntax, `schema_version = 1`, and `extends` shape;
- qualifier syntax, schema version, expressions, helper calls, and
  referenced qualifiers;
- variable syntax, type source, primitive values, resolve defaults, resolve
  rules, catalog references, and value references;
- catalog syntax, schema references, entry schema validation, and
  `x-rototo-catalog-ref` references;
- standalone JSON Schema parsing and compilation;
- request context compatibility for context attributes read by expressions;
- custom Lua lint registration and handler execution;
- graph issues such as unreferenced qualifiers, shadowed rules, and unused
  values.

## Stages

| Stage | What it protects |
| --- | --- |
| `discover` | Package root and known documents. |
| `parse` | TOML, JSON, and Lua file parsing. |
| `project` | File content shape and required fields. |
| `register` | Custom Lua lint registration. |
| `reference` | Links between variables, qualifiers, catalogs, and context paths. |
| `value` | Primitive values, catalog schemas, catalog values, and custom value rules. |
| `graph` | Relationships that are valid syntax but suspicious behavior. |
| `policy` | Custom policy checks. |

Custom lint handlers can run in `project`, `reference`, `value`, `graph`, or
`policy`.

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

Selectors do not change how the package is validated. They change which
diagnostics are reported.

## SDK

The SDK can lint an inspected package handle:

:::sdk-snippet lint-package-sdk
```rust
let pkg = Package::inspect(package_root).await?;
let lint = pkg.lint().await?;
```

```python
pkg = await rototo.Package.inspect(package_root)
lint = await pkg.lint()
```

```typescript
const pkg = await Package.inspect(packageRoot);
const lint = await pkg.lint();
```

```java
try (Package pkg = Package.inspect(packageRoot).get()) {
    PackageLint lint = pkg.lint().get();
}
```

```go
pkg, err := rototo.Inspect(ctx, packageRoot, nil)
if err != nil {
    return err
}
defer pkg.Close()

lint, err := pkg.Lint(ctx)
```
:::

`Package::load` and the equivalent language SDK load calls also run lint by
default and reject packages with error diagnostics.

## Custom Policy

Built-in lint knows rototo's structural contract.
[Custom Lua lint](reference-custom-lua-lint.html) handles local policy: naming
conventions, copy rules, allowed account classes, or domain limits only your
team understands.

See [Custom Lua Lint](reference-custom-lua-lint.html).
