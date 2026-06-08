# Java SDK Reference

The Java SDK is a thin JNI wrapper around the Rust SDK. Java code gets an
idiomatic `CompletableFuture` API while rototo keeps workspace loading,
linting, refresh, and resolution behavior in the Rust core.

## Install

rototo is currently versioned as an alpha package for Java 11 and newer:

```gradle
implementation("dev.rototo:rototo:0.1.0-alpha.4")
```

For Maven:

```xml
<dependency>
  <groupId>dev.rototo</groupId>
  <artifactId>rototo</artifactId>
  <version>0.1.0-alpha.4</version>
</dependency>
```

The package includes native libraries for the supported Linux, macOS, and
Windows targets. The rototo release version stays SemVer, for example
`0.1.0-alpha.4`.

## Load A Workspace

```java
import dev.rototo.Workspace;

try (Workspace workspace = Workspace.load("examples/basic").get()) {
    // Resolve variables from this workspace.
}
```

`Workspace.load` accepts the same source strings as the CLI. It lints the
workspace and rejects lint failures before returning.

## Resolve A Variable

```java
Map<String, Object> context = Map.of(
    "user",
    Map.of("tier", "premium")
);

VariableResolution resolution = workspace
    .resolveVariable("premium-message", context)
    .get();

System.out.println(resolution.valueKey());
System.out.println(resolution.value());
```

`VariableResolution` has:

| Method | Meaning |
| --- | --- |
| `id()` | Variable id. |
| `valueKey()` | Selected value key. |
| `value()` | Selected JSON-compatible value. |

## Resolve A Qualifier

```java
QualifierResolution resolution = workspace
    .resolveQualifier("premium-users", context)
    .get();

System.out.println(resolution.value());
```

`QualifierResolution` has `id()` and `value()`.

## Context Validation

Resolution validates context against `schemas/context.schema.json` by default.
Skip validation for one call when a tool needs to evaluate partial context:

```java
VariableResolution resolution = workspace
    .resolveVariable(
        "premium-message",
        context,
        ResolveOptions.validateContext(false)
    )
    .get();
```

The context still must be a JSON object represented as a `Map<String, ?>`.

## Inspect And Lint

```java
try (Workspace workspace = Workspace.inspect("examples/basic").get()) {
    WorkspaceLint lint = workspace.lint().get();
}
```

Inspection is for tools. A workspace loaded through `inspect` cannot resolve
variables or qualifiers because it does not compile the runtime model.

## Refreshing Workspace

```java
RefreshingWorkspaceOptions options = RefreshingWorkspaceOptions.builder()
    .periodSeconds(30.0)
    .build();

RefreshingWorkspace workspace = RefreshingWorkspace
    .load(source, options)
    .get();

VariableResolution resolution = workspace
    .resolveVariable("premium-message", context)
    .get();
RefreshStatus status = workspace.status().get();
workspace.shutdown().get();
```

`RefreshingWorkspace` keeps serving the last successfully loaded workspace when
refresh fails. `status` returns a `RefreshStatus` object with fingerprint,
success, attempt, failure, error, refreshing, and immutable fields.

## Errors

Rust `RototoError` values become `RototoException` in Java. Because public SDK
methods return `CompletableFuture`, errors are available as the future cause:

```java
try {
    workspace.resolveVariable("missing", Map.of()).get();
} catch (ExecutionException error) {
    if (error.getCause() instanceof RototoException rototoError) {
        System.out.println(rototoError.getMessage());
    }
}
```

## Public Types

| Type | Purpose |
| --- | --- |
| `Workspace` | Loaded workspace handle. |
| `RefreshingWorkspace` | Refreshing workspace handle for services. |
| `VariableResolution` | Selected variable value. |
| `QualifierResolution` | Qualifier boolean result. |
| `RefreshStatus` | Refresh state snapshot. |
| `WorkspaceLint` | Lint result for a loaded or inspected workspace. |
| `RototoException` | Error raised for rototo failures. |
