# Package Layout Reference

A rototo package is a filesystem boundary. Before rototo can lint,
inspect, or resolve anything, it needs to know which files belong to that
boundary and what each file means.

The boundary starts at `rototo-package.toml`. The directories beside that
manifest are discovered by name, and ids come from filenames. That keeps the
package reviewable: moving, renaming, adding, or deleting configuration is
visible as ordinary repository work.

## Root

Every package root must contain:

```text
rototo-package.toml
```

The manifest must declare `schema_version = 1`. See
[Package Manifest](reference-package-manifest.html) for the manifest
contract.

When a CLI command omits the package source, rototo walks upward from the
current directory until it finds `rototo-package.toml`.

## Discovered Paths

Rototo discovers these package paths:

| Path | File type | Id source | Meaning |
| --- | --- | --- | --- |
| `qualifiers/*.toml` | TOML | File stem | Named runtime conditions. |
| `variables/*.toml` | TOML | File stem | Named values resolved by applications. |
| `catalogs/*.schema.json` | JSON | Name before `.schema.json` | Schemas for structured catalog values. |
| `catalogs/<catalog-id>-entries/*.toml` | TOML | File stem | Values selectable by `catalog:<catalog-id>` variables. |
| `request-contexts/*.schema.json` | JSON | Name before `.schema.json` | Request context schemas. |
| `request-contexts/<context-id>-entries/*.json` | JSON | File stem | Stored request context samples. |
| `lint/*.lua` | Lua | File stem | Custom lint handlers. |

Only files with the listed extensions are discovered. Other files may live in
the repository for humans or local tooling, but rototo does not treat them as
package documents.

Directories are optional. A package can start with only `variables/` and add
`qualifiers/`, `catalogs/`, `request-contexts/`, or `lint/`
later.

## Ids

The file stem is the id:

```text
variables/account-limits.toml       -> variable://account-limits
qualifiers/paid-account.toml        -> qualifier://paid-account
catalogs/banner.schema.json        -> catalog://banner
catalogs/banner-entries/hidden.toml -> catalog value name hidden
request-contexts/request.schema.json -> request_context://request
request-contexts/request-entries/premium.json -> request context entry premium
```

Ids are references in other files, CLI selectors, SDK calls, diagnostics, and
resolution traces. Rename files deliberately, because a rename changes the
public id.

## Catalog Values

Catalog values are discovered only under a directory named for an existing
catalog:

```text
catalogs/
  banner.schema.json
  banner-entries/
    hidden.toml
    incident.toml
```

Here `banner.schema.json` declares the catalog and its schema. The two value
files define value names `hidden` and `incident`.

If the catalog file does not exist, rototo does not treat the matching
`*-entries/` directory as an independent catalog family.

## Request Contexts

[`request-contexts/<id>.schema.json`](reference-context.html) files declare
the runtime context shapes a package supports. Qualifiers are compatible with
the request contexts that can satisfy their expression context references. Variables
inherit request context compatibility from the qualifiers in their resolve
rules.

Stored samples live under `request-contexts/<id>-entries/*.json`. They are
validated against the matching request context schema and are used by CLI and
console previews.

## Document Kinds

JSON output reports discovered documents with a `kind` value:

```text
manifest
qualifier
variable
catalog
catalog_entry
request_context
request_context_entry
schema
custom_lint
```

These kinds appear in lint output, inspect output, and editor integrations.
They are part of the machine-readable contract.

## What Layout Does Not Do

Package layout does not grant ownership. Repository permissions, review
rules, and CI decide who can change files. Rototo reads the files after a
package source has already been chosen.

Layout also does not define deployment. Applications load a
[package source](reference-package-sources.html), which may be a local path,
git source, or HTTPS archive.
