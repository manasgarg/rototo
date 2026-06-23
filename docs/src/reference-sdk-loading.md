# SDK Loading Reference

Applications should not parse package files directly. They should load a
[package source](reference-package-sources.html) with the SDK, let rototo
lint it, and [resolve named variables](reference-sdk-resolution.html) from the
loaded package.

The loading API is the boundary that decides whether the app receives a valid
control plane. Resolution and [refresh](reference-sdk-refresh.html) have their
own pages.

For install commands, imports, and exact language-specific types, see the
[Rust](reference-sdk-rust.html), [Python](reference-sdk-python.html),
[TypeScript](reference-sdk-typescript.html),
[Java](reference-sdk-java.html), and [Go](reference-sdk-go.html) SDK
references.

## Load A Package

:::sdk-snippet load-package
```rust
use rototo::Package;

let pkg = Package::load("git+https://github.com/acme/config.git#main").await?;
```

```python
import rototo

pkg = await rototo.Package.load(
    "git+https://github.com/acme/config.git#main",
)
```

```typescript
import { Package } from "rototo";

const pkg = await Package.load(
  "git+https://github.com/acme/config.git#main",
);
```

```java
import dev.rototo.Package;

Package pkg = Package
    .load("git+https://github.com/acme/config.git#main")
    .get();
```

```go
import (
    "context"

    rototo "github.com/manasgarg/rototo/sdks/go"
)

pkg, err := rototo.Load(
    context.Background(),
    "git+https://github.com/acme/config.git#main",
    nil,
)
if err != nil {
    return err
}
defer pkg.Close()
```
:::

Loading stages the source, inspects the package, runs
[lint](reference-lint-overview.html), and rejects lint failures. It accepts the
same [source forms](reference-package-sources.html) as the CLI.

Use this for services that load configuration once at startup.

## Inspect A Package

:::sdk-snippet inspect-package
```rust
let pkg = Package::inspect("examples/basic").await?;
```

```python
pkg = await rototo.Package.inspect("examples/basic")
```

```typescript
const pkg = await Package.inspect("examples/basic");
```

```java
Package pkg = Package.inspect("examples/basic").get();
```

```go
pkg, err := rototo.Inspect(context.Background(), "examples/basic", nil)
if err != nil {
    return err
}
defer pkg.Close()
```
:::

Inspection stages and inspects a package without requiring a lint-clean
runtime. It is the lower-level loader for tools that need to inspect broken
packages, editor state, or staged diagnostics.

Most application code should load a runtime package instead.

## Load Options

:::sdk-snippet load-options
```rust
use rototo::{LoadOptions, LintMode, SourceAuth};

let options = LoadOptions::new()
    .with_lint(LintMode::Deny)
    .with_source_auth(SourceAuth::Bearer(token));

let pkg = Package::load_with_options(source, options).await?;
```

```python
pkg = await rototo.Package.load(
    source,
    lint="deny",
    package_token=token,
)
```

```typescript
const pkg = await Package.load(source, {
  lint: "deny",
  packageToken: token,
});
```

```java
LoadOptions options = LoadOptions.builder()
    .lint(LintMode.DENY)
    .packageToken(token)
    .build();

Package pkg = Package.load(source, options).get();
```

```go
pkg, err := rototo.Load(ctx, source, &rototo.LoadOptions{
    Lint:           rototo.LintDeny,
    PackageToken: token,
})
if err != nil {
    return err
}
```
:::

Lint deny is the default. It rejects lint failures during load.

Lint skip is available for tools that need to stage or inspect a package
without enforcing lint. Do not use it as the default in application runtime
paths.

## Package Metadata

:::sdk-snippet package-metadata
```rust
let root = pkg.root();
let inspection = pkg.inspection();
let context_schema = pkg.context_schema();
let fingerprint = pkg.source_fingerprint();
let immutable = pkg.immutable_source();
let layers = pkg.source_layers();
```

```python
root = pkg.root
```

```typescript
const root = pkg.root;
```

```java
String root = pkg.root();
```

```go
root, err := pkg.Root()
```
:::

The Rust SDK currently exposes the full loaded source metadata. The first
Python, TypeScript, Java, and Go SDK releases expose the staged root path and
keep the runtime path small; more inspection metadata can be added when
language-specific tools need it.

## Temporary Staging

Remote sources are staged into temporary directories owned by the package
handle. Keep the package value alive for as long as the app needs to resolve
from it.

Do not retain paths into the staged root after dropping the package.

## Context Schema

When the loaded package contains request context schemas,
[resolution](reference-sdk-resolution.html) validates context against a
compatible schema by default.

See [Resolve Context](reference-context.html) and
[SDK Resolution](reference-sdk-resolution.html).
