# TypeScript SDK Reference

The TypeScript SDK is a thin N-API wrapper around the Rust SDK. TypeScript code
gets an idiomatic async API with camelCase fields while rototo keeps workspace
loading, linting, refresh, and resolution behavior in the Rust core.

## Install

rototo is currently published as an alpha package for Node.js 20 and newer:

```sh
npm install rototo@alpha
```

The package includes native modules for Linux, macOS, and Windows on the
supported x64 and arm64 targets. The rototo release version stays SemVer, for
example `0.1.0-alpha.5`.

## Load A Workspace

```typescript
import { Workspace } from "rototo";

const workspace = await Workspace.load("examples/basic");
```

`Workspace.load` accepts the same
[source strings](reference-workspace-sources.html) as the CLI. It
[lints](reference-lint-overview.html) the workspace and rejects lint failures
before returning.

## Resolve A Variable

```typescript
const resolution = await workspace.resolveVariable(
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
const matches = await workspace.resolveQualifier(
  "premium-users",
  { user: { tier: "premium" } },
);

console.log(matches);
```

Qualifier resolution returns the final boolean result.

## Context Validation

Resolution validates [context](reference-context.html) against a compatible
request context schema by default. Skip validation for one call when a tool
needs to evaluate partial context:

```typescript
const resolution = await workspace.resolveVariable(
  "premium-message",
  context,
  { validateContext: false },
);
```

The context still must be a JSON object.

## Inspect And Lint

```typescript
const workspace = await Workspace.inspect("examples/basic");
const lint = await workspace.lint();
```

Inspection is for tools. A workspace loaded through `inspect` cannot
[resolve variables or qualifiers](reference-sdk-resolution.html) because it
does not compile the runtime model.

## Refreshing Workspace

```typescript
import { RefreshingWorkspace } from "rototo";

const workspace = await RefreshingWorkspace.load(source, {
  periodSeconds: 30,
});

const resolution = await workspace.resolveVariable("premium-message", context);
const status = await workspace.status();
await workspace.shutdown();
```

[`RefreshingWorkspace`](reference-sdk-refresh.html) keeps serving the last
successfully loaded workspace when refresh fails. `status` returns a
`RefreshStatus` object with fingerprint, success, attempt, failure, error,
refreshing, and immutable fields.

## Errors

Rust `RototoError` values become `RototoError` in TypeScript:

```typescript
import { RototoError } from "rototo";

try {
  await workspace.resolveVariable("missing", {});
} catch (error) {
  if (error instanceof RototoError) {
    console.log(error.message);
  }
}
```

## Public Types

| Type | Purpose |
| --- | --- |
| `Workspace` | Loaded workspace handle. |
| `RefreshingWorkspace` | Refreshing workspace handle for services. |
| `VariableResolution` | Selected variable value. |
| `RefreshStatus` | Refresh state snapshot. |
| `RototoError` | Error raised for rototo failures. |
