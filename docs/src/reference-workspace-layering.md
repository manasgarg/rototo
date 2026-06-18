# Workspace Layering Reference

Workspace layering lets one workspace inherit files from another workspace
without giving every owner write access to one repository. Rototo projects the
layers into one staged workspace, then runs lint and resolution against that
projected result.

Layering is declared with `extends` in `rototo-workspace.toml`.

## Declaration

```toml
schema_version = 1
extends = ["../base-config", "git+ssh://git@example.com/acme/customer.git#main"]
```

Each entry must be a nonblank string without surrounding whitespace. Every
entry is parsed as a [workspace source](reference-workspace-sources.html).

Relative entries are resolved from the workspace that declares them.

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
into the projected workspace root.

The child workspace's `rototo-workspace.toml` remains the manifest of the final
projected workspace.

## Catalog Entry Layers

Catalog value files layer by path:

```text
catalogs/banner-entries/default.toml
catalogs/banner-entries/incident.toml
```

A child can add a new entry by adding a new file. It can replace an inherited
entry by writing the same entry path. It cannot partially merge fields within
an entry file.

The catalog declaration still has to exist in the projected workspace, and
every entry still has to match the catalog schema after projection.

## Safety Limits

Rototo rejects layering graphs that:

- exceed 32 layers;
- contain a cycle;
- use a relative parent source that escapes a staged workspace boundary.

Those checks keep workspace loading finite and keep relative sources scoped to
the projected source tree.

## Lint And Resolution

Lint runs after projection. That means lint sees the same files the
application will resolve from.

This is the important operational property: parent and child changes are not
validated in isolation when the application loads the child source. They are
validated as the final workspace the app will use.

## Fingerprints And Refresh

Layered workspaces use a combined fingerprint built from the source layers. A
refresh can replace the active workspace only after the source graph is probed
and the projected workspace loads and lints successfully.

If any layer is mutable, the combined source is mutable. If every layer is an
immutable pinned git commit, the combined source is immutable.

See [SDK Refresh](reference-sdk-refresh.html) for how this affects
long-running services.
