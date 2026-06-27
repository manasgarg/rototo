<!-- Generated from docs/src/reference-sdk-typescript.md by `rototo docs --package-readme typescript --out sdks/typescript/README.md`. Do not edit directly. -->

# rototo TypeScript SDK

The TypeScript SDK is a thin N-API wrapper around the Rust SDK. TypeScript code
gets an idiomatic async API with camelCase fields while rototo keeps package
loading, linting, refresh, and resolution behavior in the Rust core.

## Install

rototo is currently published as an alpha package for Node.js 20 and newer:

```sh
npm install rototo@alpha
```

The package includes native modules for Linux, macOS, and Windows on the
supported x64 and arm64 targets. The rototo release version stays SemVer, for
example `0.1.0-alpha.5`.

## Load A Package

```typescript
import { Package } from "rototo";

const pkg = await Package.load("examples/basic");
```

`Package.load` accepts the same
[source strings](https://docs.rototo.dev/reference-package-sources.html) as the CLI. It
[lints](https://docs.rototo.dev/reference-lint-overview.html) the package and rejects lint failures
before returning.

## Resolve A Variable

```typescript
const resolution = pkg.resolveVariable(
  "premium-message",
  { user: { tier: "premium" } },
);

console.log(resolution.value);
console.log(resolution.source);
```

`VariableResolution` has:

| Field | Meaning |
| --- | --- |
| `id` | Variable id. |
| `value` | Selected JSON-compatible value. |
| `source` | Selected source. |

## Resolve A Qualifier

```typescript
const matches = pkg.resolveQualifier(
  "premium-users",
  { user: { tier: "premium" } },
);

console.log(matches);
```

Qualifier resolution returns the final boolean result.

## Context Validation

Resolution validates [context](https://docs.rototo.dev/reference-context.html) against a compatible
evaluation context schema by default. Skip validation for one call when a tool
needs to evaluate partial context:

```typescript
const resolution = pkg.resolveVariable(
  "premium-message",
  context,
  { validateContext: false },
);
```

The context still must be a JSON object.

## Inspect And Lint

```typescript
const pkg = await Package.inspect("examples/basic");
const lint = await pkg.lint();
```

Inspection is for tools. A package loaded through `inspect` cannot
[resolve variables or qualifiers](https://docs.rototo.dev/reference-sdk-resolution.html) because it
does not compile the runtime model.

## Refreshing Package

```typescript
import { RefreshingPackage } from "rototo";

const pkg = await RefreshingPackage.load(source, {
  periodSeconds: 30,
});

const resolution = pkg.resolveVariable("premium-message", context);
const status = await pkg.status();
await pkg.shutdown();
```

[`RefreshingPackage`](https://docs.rototo.dev/reference-sdk-refresh.html) keeps serving the last
successfully loaded package when refresh fails. `status` returns a
`RefreshStatus` object with fingerprint, success, attempt, failure, error,
refreshing, and immutable fields.

## Errors

Rust `RototoError` values become `RototoError` in TypeScript:

```typescript
import { RototoError } from "rototo";

try {
  pkg.resolveVariable("missing", {});
} catch (error) {
  if (error instanceof RototoError) {
    console.log(error.message);
  }
}
```

## Public Types

| Type | Purpose |
| --- | --- |
| `Package` | Loaded package handle. |
| `RefreshingPackage` | Refreshing package handle for services. |
| `VariableResolution` | Selected variable value. |
| `RefreshStatus` | Refresh state snapshot. |
| `RototoError` | Error raised for rototo failures. |
