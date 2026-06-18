# Python SDK Reference

The Python SDK is a thin native wrapper around the Rust SDK. Python code gets
an idiomatic async API while rototo keeps workspace loading, linting, refresh,
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

The rototo release version stays SemVer, for example `0.1.0-alpha.4`. Python
packaging tools may display the equivalent PEP 440 spelling `0.1.0a4` for the
distribution.

## Load A Workspace

```python
import rototo

workspace = await rototo.Workspace.load("examples/basic")
```

`Workspace.load` accepts the same
[source strings](reference-workspace-sources.html) as the CLI. It
[lints](reference-lint-overview.html) the workspace and rejects lint failures
before returning.

## Resolve A Variable

```python
resolution = await workspace.resolve_variable(
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
resolution = await workspace.resolve_qualifier(
    "premium-users",
    {"user": {"tier": "premium"}},
)

print(resolution.value)
```

`QualifierResolution` has `id` and `value`.

## Context Validation

Resolution validates [context](reference-context.html) against
`schemas/context.schema.json` by default. Skip validation for one call when a
tool needs to evaluate partial context:

```python
resolution = await workspace.resolve_variable(
    "premium-message",
    context,
    validate_context=False,
)
```

The context still must be a JSON object.

## Inspect And Lint

```python
workspace = await rototo.Workspace.inspect("examples/basic")
lint = await workspace.lint()
```

Inspection is for tools. A workspace loaded through `inspect` cannot
[resolve variables or qualifiers](reference-sdk-resolution.html) because it
does not compile the runtime model.

## Refreshing Workspace

```python
workspace = await rototo.RefreshingWorkspace.load(
    source,
    period_seconds=30,
)

resolution = await workspace.resolve_variable("premium-message", context)
status = await workspace.status()
await workspace.shutdown()
```

[`RefreshingWorkspace`](reference-sdk-refresh.html) keeps serving the last
successfully loaded workspace when refresh fails. `status` returns a
`RefreshStatus` dataclass with fingerprint, success, attempt, failure, error,
refreshing, and immutable fields.

## Errors

Rust `RototoError` values become `rototo.RototoError` in Python:

```python
try:
    await workspace.resolve_variable("missing", {})
except rototo.RototoError as error:
    print(error)
```

Invalid Python option values, such as an unknown lint mode, raise Python
`ValueError`.

## Public Types

| Type | Purpose |
| --- | --- |
| `Workspace` | Loaded workspace handle. |
| `RefreshingWorkspace` | Refreshing workspace handle for services. |
| `VariableResolution` | Selected variable value. |
| `QualifierResolution` | Qualifier boolean result. |
| `RefreshStatus` | Refresh state snapshot. |
| `RototoError` | Error raised for rototo failures. |
