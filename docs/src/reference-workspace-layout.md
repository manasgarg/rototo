# Workspace Layout Reference

A rototo workspace is a filesystem boundary. Before rototo can lint,
inspect, or resolve anything, it needs to know which files belong to that
boundary and what each file means.

The boundary starts at `rototo-workspace.toml`. The directories beside that
manifest are discovered by name, and ids come from filenames. That keeps the
workspace reviewable: moving, renaming, adding, or deleting configuration is
visible as ordinary repository work.

## Root

Every workspace root must contain:

```text
rototo-workspace.toml
```

The manifest must declare `schema_version = 1`. See
[Workspace Manifest](reference-workspace-manifest.html) for the manifest
contract.

When a CLI command omits the workspace source, rototo walks upward from the
current directory until it finds `rototo-workspace.toml`.

## Discovered Paths

Rototo discovers these workspace paths:

| Path | File type | Id source | Meaning |
| --- | --- | --- | --- |
| `qualifiers/*.toml` | TOML | File stem | Named runtime conditions. |
| `variables/*.toml` | TOML | File stem | Named values resolved by applications. |
| `catalogs/*.toml` | TOML | File stem | Schemas for structured catalog entries. |
| `catalogs/<catalog-id>-entries/*.toml` | TOML | File stem | Entries selectable by `catalog:<catalog-id>` variables. |
| `schemas/*.json` | JSON | File stem | JSON Schemas used for context and catalog validation. |
| `lint/*.lua` | Lua | File stem | Custom lint handlers. |

Only files with the listed extensions are discovered. Other files may live in
the repository for humans or local tooling, but rototo does not treat them as
workspace documents.

Directories are optional. A workspace can start with only `variables/` and add
`qualifiers/`, `catalogs/`, `schemas/`, or `lint/` later.

## Ids

The file stem is the id:

```text
variables/account-limits.toml       -> variable://account-limits
qualifiers/paid-account.toml        -> qualifier://paid-account
catalogs/banner.toml               -> catalog://banner
catalogs/banner-entries/hidden.toml -> catalog entry key hidden
```

Ids are references in other files, CLI selectors, SDK calls, diagnostics, and
resolution traces. Rename files deliberately, because a rename changes the
public id.

## Catalog Entries

Catalog entries are discovered only under a directory named for an existing
catalog:

```text
catalogs/
  banner.toml
  banner-entries/
    hidden.toml
    incident.toml
```

Here `banner.toml` declares the catalog and its schema. The two entry files
define entry keys `hidden` and `incident`.

If the catalog file does not exist, rototo does not treat the matching
`*-entries/` directory as an independent catalog family.

## Special Schema Path

[`schemas/context.schema.json`](reference-context.html) has a reserved meaning.
When present, it is the schema for the runtime context an application passes
during resolution.

Other JSON Schemas under `schemas/` validate catalog entries when referenced
from a catalog file.

## Document Kinds

JSON output reports discovered documents with a `kind` value:

```text
manifest
qualifier
variable
catalog
catalog_entry
schema
custom_lint
```

These kinds appear in lint output, inspect output, and editor integrations.
They are part of the machine-readable contract.

## What Layout Does Not Do

Workspace layout does not grant ownership. Repository permissions, review
rules, and CI decide who can change files. Rototo reads the files after a
workspace source has already been chosen.

Layout also does not define deployment. Applications load a
[workspace source](reference-workspace-sources.html), which may be a local path,
git source, or HTTPS archive.
