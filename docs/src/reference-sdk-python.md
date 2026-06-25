# Python SDK Reference

The Python SDK is a thin native wrapper around the Rust SDK. Python code gets
an idiomatic async API while rototo keeps package loading, linting, refresh,
and resolution behavior in the Rust core.

## Install

rototo is currently published as an alpha package for Python 3.10 and newer:

```sh
python -m pip install rototo
```

If your package resolver is constrained to stable releases, allow prereleases
explicitly:

```sh
python -m pip install --pre rototo
```

The rototo release version stays SemVer, for example `0.1.0-alpha.5`. Python
packaging tools may display the equivalent PEP 440 spelling `0.1.0a5` for the
distribution.

## Load A Package

```python
import rototo

pkg = await rototo.Package.load("examples/basic")
```

`Package.load` accepts the same
[source strings](reference-package-sources.html) as the CLI. It
[lints](reference-lint-overview.html) the package and rejects lint failures
before returning.

## Resolve A Variable

```python
resolution = pkg.resolve_variable(
    "premium-message",
    {"user": {"tier": "premium"}},
)

print(resolution.value)
print(resolution.source)
```

`VariableResolution` has:

| Field | Meaning |
| --- | --- |
| `id` | Variable id. |
| `value` | Selected JSON-compatible Python value. |
| `source` | Selected source. |

## Resolve A Qualifier

```python
matches = pkg.resolve_qualifier(
    "premium-users",
    {"user": {"tier": "premium"}},
)

print(matches)
```

Qualifier resolution returns the final boolean result.

## Context Validation

Resolution validates [context](reference-context.html) against a compatible
request context schema by default. Skip validation for one call when a tool
needs to evaluate partial context:

```python
resolution = pkg.resolve_variable(
    "premium-message",
    context,
    validate_context=False,
)
```

The context still must be a JSON object.

## Inspect And Lint

```python
pkg = await rototo.Package.inspect("examples/basic")
lint = await pkg.lint()
```

Inspection is for tools. A package loaded through `inspect` cannot
[resolve variables or qualifiers](reference-sdk-resolution.html) because it
does not compile the runtime model.

## Refreshing Package

```python
pkg = await rototo.RefreshingPackage.load(
    source,
    period_seconds=30,
)

resolution = pkg.resolve_variable("premium-message", context)
status = await pkg.status()
await pkg.shutdown()
```

[`RefreshingPackage`](reference-sdk-refresh.html) keeps serving the last
successfully loaded package when refresh fails. `status` returns a
`RefreshStatus` dataclass with fingerprint, success, attempt, failure, error,
refreshing, and immutable fields.

## Errors

Rust `RototoError` values become `rototo.RototoError` in Python:

```python
try:
    pkg.resolve_variable("missing", {})
except rototo.RototoError as error:
    print(error)
```

Invalid Python option values, such as an unknown lint mode, raise Python
`ValueError`.

## Public Types

| Type | Purpose |
| --- | --- |
| `Package` | Loaded package handle. |
| `RefreshingPackage` | Refreshing package handle for services. |
| `VariableResolution` | Selected variable value. |
| `RefreshStatus` | Refresh state snapshot. |
| `RototoError` | Error raised for rototo failures. |
