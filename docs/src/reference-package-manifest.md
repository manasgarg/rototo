# Package Manifest Reference

Every rototo package is rooted at `rototo-package.toml`. The manifest is
the file that tells rototo where the package boundary starts, which package
format version is in use, and which parent packages should be layered in.

That makes the manifest small on purpose. It defines the package boundary and
layering, not the application policy inside the package. The contract below
is the exact manifest shape for schema version 1.

## Minimal Manifest

The smallest valid manifest says only which package format rototo should
expect:

```toml
schema_version = 1
```

`schema_version` is required. It must be the integer `1`.

If the file is missing, rototo reports `rototo/package-manifest-missing`. If
the file cannot be parsed as TOML, rototo reports
`rototo/package-manifest-parse-failed`. If the manifest omits
`schema_version`, declares a non-integer value, or declares a version other than
`1`, rototo reports `rototo/package-manifest-schema-failed`.

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Package manifest format version. The only supported value is `1`. |
| `extends` | No | array of strings | Parent package sources projected before this package. |

Only the fields above are part of rototo's manifest contract. Treat any other
top-level fields as package-specific metadata only if your own tooling
explicitly owns them.

## Package Layering

`extends` declares parent package sources. Use it when one administrative
owner needs to build on another owner's package without copying every file:

```toml
schema_version = 1
extends = ["../product-config", "git+ssh://git@example.com/customer-config.git#main"]
```

Each `extends` entry must be a nonblank string without surrounding whitespace.
The value is parsed as a [package source](reference-package-sources.html).
It can use the same source forms as ordinary package loading, including local
paths, `file://`, `git+file://`, `git+https://`, `git+ssh://`, and supported
HTTPS archive sources.

Relative `extends` entries are resolved from the package that declares them.
When a package source is staged into a temporary directory, relative parent
sources must stay inside the staged package boundary.

Layer projection order is parent first, then child. If a later layer contains
the same path as an earlier layer, the later file replaces the earlier file at
that path. The root `rototo-package.toml` from parent layers is not copied
into the final projected package; the child manifest remains the root
manifest.

Rototo rejects layering graphs that exceed 32 layers or contain a cycle.

For the ownership model behind layering, see
[Package Layering](package-layering.html).

## Valid Manifest Examples

A standalone package has one boundary and no parent layers:

```toml
schema_version = 1
```

A package that extends one parent projects the parent first, then applies the
child files:

```toml
schema_version = 1
extends = ["../base-config"]
```

## Invalid Manifest Examples

These examples are invalid because they break the boundary contract rototo
needs before it can inspect the package.

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

The manifest does not define variables, qualifiers, catalogs, request contexts,
or catalog values. It also does not declare custom Lua lint rule metadata. Those
contracts live in the package directories:

```text
qualifiers/
variables/
catalogs/
request-contexts/
lint/
```

Custom lint handlers and their rule metadata live in `lint/*.lua`; see
[Custom Lua Lint](reference-custom-lua-lint.html).

The manifest also does not grant edit permission. Repository permissions,
review rules, CI, and deployment policy decide who may change a package.
Rototo reads the manifest after those controls have selected the package
source to load.
