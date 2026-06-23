# Package Terminology Cut

Rototo now uses `package` for the filesystem boundary that contains
qualifiers, variables, catalogs, schemas, request contexts, and lint rules.
That noun is meant to signal the mental model: a rototo directory tree is a
configuration package that teams build, review, lint, publish, and load from
applications.

This is a clean breaking change. There are no compatibility aliases, fallback
manifest names, deprecated command groups, or SDK type aliases for the previous
product noun.

## Product Surface

- Manifest file: `rototo-package.toml`.
- CLI source flag and positional help: package.
- Remote/local source terminology: package source.
- Auth token flag and environment variable:
  - `--package-token`
  - `ROTOTO_PACKAGE_TOKEN`
- Rust SDK:
  - `Package`
  - `RefreshingPackage`
  - `PackageInspection`
  - `PackageLint`
  - `PackageDiff`
  - `StagedPackage`
  - `LoadedPackageSource`
  - `SourceFingerprint::PackageLayers`
  - `inspect_package`
  - `lint_package`
  - `diff_packages`
  - `stage_package_source`
  - `probe_package_source`
  - `find_package_root`
- Language SDKs expose package names directly:
  - Python: `rototo.Package`, `rototo.RefreshingPackage`,
    `package_token`
  - TypeScript: `Package`, `RefreshingPackage`, `packageToken`
  - Java: `dev.rototo.Package`, `RefreshingPackage`, `packageToken`
  - Go: `Package`, `RefreshingPackage`, `PackageToken`
- Diagnostics use package rule ids and package targets, for example
  `rototo/package-manifest-missing`.
- Console API, store rows, routes, TypeScript UI types, and UI copy use package
  terminology.
- Documentation and generated package READMEs use package terminology and SDK
  examples use `pkg` for local handles in languages where `package` is reserved.

## Boundaries

External build-system and protocol vocabulary remains unchanged when rototo is
only consuming another tool's contract. The product-facing rototo vocabulary
must not reintroduce the previous noun in commands, docs, SDK APIs, tests,
diagnostics, or user-facing console copy.

## Verification

The implementation should pass:

```sh
just fmt
just check
```

Terminology sweeps should leave only external-tool/protocol uses when searching
source and docs.
