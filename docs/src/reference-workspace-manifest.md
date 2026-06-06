# Workspace Manifest Reference

Every rototo workspace is rooted at `rototo-workspace.toml`. The manifest is
the file that tells rototo where the workspace boundary starts, which workspace
format version is in use, and which parent workspaces should be layered in.

This page is the exact manifest contract for schema version 1.

## Minimal Manifest

The smallest valid manifest is:

```toml
schema_version = 1
```

`schema_version` is required. It must be the integer `1`.

If the file is missing, rototo reports `rototo/workspace-manifest-missing`. If
the file cannot be parsed as TOML, rototo reports
`rototo/workspace-manifest-parse-failed`. If the manifest omits
`schema_version`, declares a non-integer value, or declares a version other than
`1`, rototo reports `rototo/workspace-manifest-schema-failed`.

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Workspace manifest format version. The only supported value is `1`. |
| `extends` | No | array of strings | Parent workspace sources projected before this workspace. |

Only the fields above are part of rototo's manifest contract. Treat any other
top-level fields as workspace-specific metadata only if your own tooling
explicitly owns them.

## Workspace Layering

`extends` declares parent workspace sources:

```toml
schema_version = 1
extends = ["../product-config", "git+ssh://git@example.com/customer-config.git#main"]
```

Each `extends` entry must be a nonblank string without surrounding whitespace.
The value is parsed as a workspace source. It can use the same source forms as
ordinary workspace loading, including local paths, `file://`, `git+file://`,
`git+https://`, `git+ssh://`, and supported HTTPS archive sources.

Relative `extends` entries are resolved from the workspace that declares them.
When a workspace source is staged into a temporary directory, relative parent
sources must stay inside the staged workspace boundary.

Layer projection order is parent first, then child. If a later layer contains
the same path as an earlier layer, the later file replaces the earlier file at
that path. The root `rototo-workspace.toml` from parent layers is not copied
into the final projected workspace; the child manifest remains the root
manifest.

Rototo rejects layering graphs that exceed 32 layers or contain a cycle.

For the ownership model behind layering, see `workspace-layering`.

## Valid Manifest Examples

A standalone workspace:

```toml
schema_version = 1
```

A workspace that extends one parent:

```toml
schema_version = 1
extends = ["../base-config"]
```

## Invalid Manifest Examples

`schema_version` must be present:

```toml
extends = ["../base-config"]
```

`schema_version` must be the integer `1`:

```toml
schema_version = "1"
```

`extends` must be an array:

```toml
schema_version = 1
extends = "../base-config"
```

`extends` entries must not be blank or padded:

```toml
schema_version = 1
extends = [" ../base-config "]
```

## What The Manifest Does Not Define

The manifest does not define variables, qualifiers, resources, schemas, or
resource objects. It also does not declare custom Lua lint rule metadata. Those
contracts live in the workspace directories:

```text
qualifiers/
variables/
resources/
schemas/
lint/
```

Custom lint handlers and their rule metadata live in `lint/*.lua`; see
`reference-custom-lua-lint`.

The manifest also does not grant edit permission. Repository permissions,
review rules, CI, and deployment policy decide who may change a workspace.
Rototo reads the manifest after those controls have selected the workspace
source to load.
