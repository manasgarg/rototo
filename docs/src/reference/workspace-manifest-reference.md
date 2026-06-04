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

Variables may define blocks such as `[env.prod]` only for environments
declared here. A variable must also define `[env._]`, but `_` is not a
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

Context schema validation prevents malformed runtime context from reaching
qualifier and variable rule evaluation.

## `extends`

Optional. Layers this workspace on top of a parent workspace so that shared
configuration can live in one place and more-derived workspaces add to or
replace it. This is the mechanism teams use to keep an organization-wide base
workspace separate from their own.

```toml
schema_version = 1
extends = "../base"

[environments]
values = ["dev", "prod", "canary"]
```

`extends` accepts the same source forms as a workspace argument: a local path,
`file://`, `git+file://`, `git+https://`, `git+ssh://`, or an `https://` archive
URL. Relative local paths resolve against the declaring workspace's own
directory. A workspace declares at most one parent; the parent may itself
`extends` another workspace, forming a chain.

rototo composes the chain into a single effective workspace before lint and
resolution run:

- File-level entities are overlaid by their workspace-relative path. A
  more-derived workspace **adds** an entity by introducing a new file, or
  **replaces** a parent entity by providing a file with the same path
  (`variables/checkout-discount.toml` replaces the parent's file of the same
  name wholesale). Layering never removes a parent entity.
- `[environments]` uses child-overrides semantics: the most-derived
  `[environments].values` is authoritative for the composed workspace.
- `[context]` is inherited from the parent unless the more-derived workspace
  declares its own.
- Custom `[[lint.rule]]` declarations are additive across the chain,
  deduplicated by `id`, with the more-derived declaration winning.

Composition fails (before lint) when the chain forms a cycle, a parent cannot
be loaded, a layer declares a `schema_version` other than `1`, or `extends` is
not a string.

`rototo inspect` reports the resolved layer chain, base first, so you can see
which workspaces contributed to the composed result. The SDK exposes the same
provenance through `Workspace::layers()`.

## Discovery

rototo discovers workspace files from conventional directories:

```text
qualifiers/*.toml
variables/*.toml
resources/*.toml
resources/<resource-id>-objects/*.toml
schemas/*.json
```

Only direct `.toml` files in `qualifiers/`, `variables/`, and `resources/`
define ids. Resource object files live under the matching
`resources/<resource-id>-objects/` directory; the object file stem is the object
id.

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
- `extends`, when present, is a string and resolves to a loadable parent
  workspace, the chain has no cycle, and every layer declares
  `schema_version = 1`. Lint runs against the composed workspace.
