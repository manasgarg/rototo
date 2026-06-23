# Go SDK Reference

The Go SDK is a thin cgo wrapper around the Rust SDK. Go code gets a small
context-aware API while rototo keeps workspace loading, linting, refresh, and
resolution behavior in the Rust core.

## Install

rototo is currently versioned as an alpha Go package:

```sh
go get github.com/manasgarg/rototo/sdks/go@v0.1.0-alpha.4
```

Import the package with an explicit local name:

```go
import rototo "github.com/manasgarg/rototo/sdks/go"
```

The first Go SDK release loads the Rust native library through cgo. When
running from source, build the library and point the SDK at it:

```sh
cargo build --locked --package rototo-go
export ROTOTO_GO_NATIVE_PATH="$PWD/target/debug/librototo_go.so"
```

Use `librototo_go.dylib` on macOS and `rototo_go.dll` on Windows. The rototo
release version stays SemVer, for example `0.1.0-alpha.4`.

## Load A Workspace

```go
package main

import (
    "context"

    rototo "github.com/manasgarg/rototo/sdks/go"
)

func main() error {
    workspace, err := rototo.Load(context.Background(), "examples/basic", nil)
    if err != nil {
        return err
    }
    defer workspace.Close()

    return nil
}
```

`Load` accepts the same source strings as the CLI. It lints the workspace and
rejects lint failures before returning.

## Resolve A Variable

```go
resolveContext := map[string]any{
    "user": map[string]any{
        "tier": "premium",
    },
}

resolution, err := workspace.ResolveVariable(
    context.Background(),
    "premium-message",
    resolveContext,
    nil,
)
if err != nil {
    return err
}

fmt.Println(resolution.Value)
fmt.Println(resolution.Source)
```

`VariableResolution` has:

| Field | Meaning |
| --- | --- |
| `ID` | Variable id. |
| `Value` | Selected JSON-compatible value. |
| `Source` | Selected source. |

## Resolve A Qualifier

```go
matches, err := workspace.ResolveQualifier(
    context.Background(),
    "premium-users",
    resolveContext,
    nil,
)
if err != nil {
    return err
}

fmt.Println(matches)
```

Qualifier resolution returns the final boolean result.

## Context Validation

Resolution validates context against a compatible request context schema by
default. Skip validation for one call when a tool needs to evaluate partial
context:

```go
resolution, err := workspace.ResolveVariable(
    context.Background(),
    "premium-message",
    resolveContext,
    &rototo.ResolveOptions{SkipContextValidation: true},
)
```

The context still must be a JSON object represented as `map[string]any`.

## Inspect And Lint

```go
workspace, err := rototo.Inspect(context.Background(), "examples/basic", nil)
if err != nil {
    return err
}
defer workspace.Close()

lint, err := workspace.Lint(context.Background())
```

Inspection is for tools. A workspace loaded through `Inspect` cannot resolve
variables or qualifiers because it does not compile the runtime model.

## Refreshing Workspace

```go
periodSeconds := 30.0

workspace, err := rototo.LoadRefreshing(
    context.Background(),
    source,
    &rototo.RefreshingWorkspaceOptions{
        PeriodSeconds: &periodSeconds,
    },
)
if err != nil {
    return err
}
defer workspace.Close(context.Background())

resolution, err := workspace.ResolveVariable(
    context.Background(),
    "premium-message",
    resolveContext,
    nil,
)
status, err := workspace.Status(context.Background())
err = workspace.Shutdown(context.Background())
```

`RefreshingWorkspace` keeps serving the last successfully loaded workspace when
refresh fails. `Status` returns a `RefreshStatus` struct with fingerprint,
success, attempt, failure, error, refreshing, and immutable fields.

## Errors

Rust `RototoError` values become `*rototo.Error` in Go:

```go
_, err := workspace.ResolveVariable(context.Background(), "missing", nil, nil)
if err != nil {
    var rototoError *rototo.Error
    if errors.As(err, &rototoError) {
        fmt.Println(rototoError.Message)
    }
}
```

Invalid Go-side inputs, such as a context that cannot marshal as JSON, return
standard Go errors.

## Public Types

| Type | Purpose |
| --- | --- |
| `Workspace` | Loaded workspace handle. |
| `RefreshingWorkspace` | Refreshing workspace handle for services. |
| `LoadOptions` | Workspace load options. |
| `RefreshingWorkspaceOptions` | Refreshing workspace load options. |
| `ResolveOptions` | Per-call resolution options. |
| `VariableResolution` | Selected variable value. |
| `RefreshStatus` | Refresh state snapshot. |
| `WorkspaceLint` | Lint result for a loaded or inspected workspace. |
| `Error` | Error raised for rototo failures. |
