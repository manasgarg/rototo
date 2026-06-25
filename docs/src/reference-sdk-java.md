# Java SDK Reference

The Java SDK is a thin JNI wrapper around the Rust SDK. Java loading, lint,
refresh, and shutdown use `CompletableFuture`; loaded-package resolution is a
direct in-memory call into the Rust core.

## Install

rototo is currently versioned as an alpha package for Java 11 and newer:

```gradle
implementation("dev.rototo:rototo:0.1.0-alpha.5")
```

For Maven:

```xml
<dependency>
  <groupId>dev.rototo</groupId>
  <artifactId>rototo</artifactId>
  <version>0.1.0-alpha.5</version>
</dependency>
```

The package includes native libraries for the supported Linux, macOS, and
Windows targets. The rototo release version stays SemVer, for example
`0.1.0-alpha.5`.

## Load A Package

```java
import dev.rototo.Package;

try (Package pkg = Package.load("examples/basic").get()) {
    // Resolve variables from this pkg.
}
```

`Package.load` accepts the same source strings as the CLI. It lints the
package and rejects lint failures before returning.

## Resolve A Variable

```java
Map<String, Object> context = Map.of(
    "user",
    Map.of("tier", "premium")
);

VariableResolution resolution = pkg
    .resolveVariable("premium-message", context);

System.out.println(resolution.value());
System.out.println(resolution.source());
```

`VariableResolution` has:

| Method | Meaning |
| --- | --- |
| `id()` | Variable id. |
| `value()` | Selected JSON-compatible value. |
| `source()` | Selected source. |

## Resolve A Qualifier

```java
boolean matches = pkg
    .resolveQualifier("premium-users", context);

System.out.println(matches);
```

Qualifier resolution returns the final boolean result.

## Context Validation

Resolution validates context against a compatible request context schema by
default. Skip validation for one call when a tool needs to evaluate partial
context:

```java
VariableResolution resolution = pkg
    .resolveVariable(
        "premium-message",
        context,
        ResolveOptions.validateContext(false)
    );
```

The context still must be a JSON object represented as a `Map<String, ?>`.

## Inspect And Lint

```java
try (Package pkg = Package.inspect("examples/basic").get()) {
    PackageLint lint = pkg.lint().get();
}
```

Inspection is for tools. A package loaded through `inspect` cannot resolve
variables or qualifiers because it does not compile the runtime model.

## Refreshing Package

```java
RefreshingPackageOptions options = RefreshingPackageOptions.builder()
    .periodSeconds(30.0)
    .build();

RefreshingPackage pkg = RefreshingPackage
    .load(source, options)
    .get();

VariableResolution resolution = pkg
    .resolveVariable("premium-message", context);
RefreshStatus status = pkg.status().get();
pkg.shutdown().get();
```

`RefreshingPackage` keeps serving the last successfully loaded package when
refresh fails. `status` returns a `RefreshStatus` object with fingerprint,
success, attempt, failure, error, refreshing, and immutable fields.

## Errors

Rust `RototoError` values become `RototoException` in Java. Loading, lint, and
refresh methods still report errors through `CompletableFuture`; resolution
methods throw `RototoException` directly:

```java
try {
    pkg.resolveVariable("missing", Map.of());
} catch (RototoException error) {
    System.out.println(error.getMessage());
}
```

## Public Types

| Type | Purpose |
| --- | --- |
| `Package` | Loaded package handle. |
| `RefreshingPackage` | Refreshing package handle for services. |
| `VariableResolution` | Selected variable value. |
| `RefreshStatus` | Refresh state snapshot. |
| `PackageLint` | Lint result for a loaded or inspected pkg. |
| `RototoException` | Error raised for rototo failures. |
