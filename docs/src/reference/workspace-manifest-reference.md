# Workspace Manifest Reference

The workspace manifest defines the workspace boundary. It lives at the workspace
root and is always named:

```text
rototo-workspace.toml
```

The manifest declares the workspace schema version, the allowed environments,
and optionally the runtime context schema.

## Minimal Shape

```toml
schema_version = 1

[environments]
values = ["dev", "stage", "prod"]
```

## `schema_version`

Required. Must be:

```toml
schema_version = 1
```

Unsupported or missing schema versions fail workspace inspection and lint.

## `[environments]`

Required. Declares the named environments that variables may reference.

```toml
[environments]
values = ["dev", "stage", "prod"]
```

Rules:

- `values` must be a non-empty list.
- Every environment name must be a string.
- Environment names must be unique.
- `_` is reserved for the variable fallback environment and must not appear in
  this list.

Variables may define blocks such as `[variable.env.prod]` only for environments
declared here. A variable must also define `[variable.env._]`, but `_` is not a
workspace environment.

## `[context]`

Optional. Declares the JSON Schema used to validate runtime context before
qualifiers and variables are resolved.

```toml
[context]
schema = "schemas/context.schema.json"
```

The schema path must be a relative path inside the workspace. Absolute paths and
paths containing `..` are rejected.

When a context schema is present:

- the schema file must exist;
- the schema file must parse as JSON;
- the schema must be valid JSON Schema;
- resolve context must match the schema before resolution continues;
- qualifier predicate attributes must be declared by the schema unless the
  attribute starts with `qualifier.`.

Context schema validation prevents malformed runtime context from silently
falling through to a default value or selecting a value for the wrong reason.

## Discovery

rototo discovers workspace files from conventional directories:

```text
qualifiers/*.toml
variables/*.toml
schemas/*.json
```

Only direct `.toml` files in `qualifiers/` and `variables/` define qualifier and
variable ids. The file stem is the id.

External variable value files live next to a variable in
`variables/<variable-id>-values/*.toml`; see `variable-reference`.

## Complete Example

```toml
schema_version = 1

[environments]
values = ["dev", "stage", "prod"]

[context]
schema = "schemas/context.schema.json"
```

## Validation

Workspace manifest lint checks:

- `rototo-workspace.toml` exists at the workspace root.
- The manifest parses as TOML.
- `schema_version = 1` is present.
- `[environments].values` is present.
- Environment names are strings.
- Environment names are unique.
- At least one environment is declared.
- `_` is not declared as an environment.
- `[context].schema`, when present, points at a readable valid JSON Schema
  inside the workspace.
