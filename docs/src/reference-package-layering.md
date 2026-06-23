# Package Layering Reference

Package layering lets one package inherit files from another package
without giving every owner write access to one repository. Rototo projects the
layers into one staged package, then runs lint and resolution against that
projected result.

Layering is declared with `extends` in `rototo-package.toml`.

## Declaration

```toml
schema_version = 1
extends = ["../base-config", "git+ssh://git@example.com/acme/customer.git#main"]
```

Each entry must be a nonblank string without surrounding whitespace. Every
entry is parsed as a [package source](reference-package-sources.html).

Relative entries are resolved from the package that declares them.

## Projection Order

Projection is parent first, child last:

```text
base-config
customer-config
team-config
```

If `team-config` extends `customer-config`, and `customer-config` extends
`base-config`, rototo copies files in this order:

1. `base-config`
2. `customer-config`
3. `team-config`

Later files replace earlier files at the same path.

This replacement is file-level. If a child layer writes
`variables/account-limits.toml`, it owns the whole variable file at that path.
It does not merge individual TOML fields with the parent file.

## Manifest Handling

Parent manifests are used to discover the parent graph. They are not copied
into the projected package root.

The child package's `rototo-package.toml` remains the manifest of the final
projected package.

## Catalog Entry Layers

Catalog value files layer by path:

```text
catalogs/banner-entries/default.toml
catalogs/banner-entries/incident.toml
```

A child can add a new entry by adding a new file. It can replace an inherited
entry by writing the same entry path. It cannot partially merge fields within
an entry file.

The catalog declaration still has to exist in the projected package, and
every entry still has to match the catalog schema after projection.

## Safety Limits

Rototo rejects layering graphs that:

- exceed 32 layers;
- contain a cycle;
- use a relative parent source that escapes a staged package boundary.

Those checks keep package loading finite and keep relative sources scoped to
the projected source tree.

## Lint And Resolution

Lint runs after projection. That means lint sees the same files the
application will resolve from.

This is the important operational property: parent and child changes are not
validated in isolation when the application loads the child source. They are
validated as the final package the app will use.

## Fingerprints And Refresh

Layered packages use a combined fingerprint built from the source layers. A
refresh can replace the active package only after the source graph is probed
and the projected package loads and lints successfully.

If any layer is mutable, the combined source is mutable. If every layer is an
immutable pinned git commit, the combined source is immutable.

See [SDK Refresh](reference-sdk-refresh.html) for how this affects
long-running services.
