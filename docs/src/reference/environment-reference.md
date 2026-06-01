# Environment Reference

An environment is the runtime or deployment lane used when resolving variables.
Common examples are `dev`, `stage`, and `prod`.

## Declaring Environments

Workspace environments are declared in `rototo-workspace.toml`:

```toml
[environments]
values = ["dev", "stage", "prod"]
```

Rules:

- at least one environment is required;
- names must be strings;
- names must be unique;
- `_` is reserved and must not be declared here.

## Variable Environment Blocks

Variables select values through `[env]` blocks:

```toml
[env._]
value = "standard"

[env.dev]
value = "small"

[env.prod]
value = "large"
```

Every variable must define `[env._]`. That block is the fallback when
the requested environment has no variable-specific block.

Named environment blocks must match environments declared in the workspace
manifest.

## Resolution Behavior

When resolving a variable for an environment:

1. rototo checks that the requested environment is declared by the workspace.
2. rototo uses `[env.<environment>]` if it exists.
3. Otherwise, rototo uses `[env._]`.
4. Rules in the selected block are evaluated in order.
5. The first matching rule selects its value.
6. If no rule matches, the block's `value` is selected.

The `_` fallback is per variable. It does not make `_` a valid requested
environment.

## Unknown Environments

Resolving a variable for an environment not declared in the workspace manifest
fails before fallback selection.

This is deliberate. A misspelled environment such as `prd` should not silently
use production or default behavior.

## Example

```toml
schema_version = 1

description = "Maximum number of tokens the summarizer can emit"
type = "int"

[values]
small = 500
standard = 1000
large = 2000

[env._]
value = "standard"

[env.dev]
value = "small"

[env.prod]
value = "large"
```

Resolution results:

```text
dev   -> small    -> 500
prod  -> large    -> 2000
stage -> standard -> 1000, if stage has no block and is declared
prd   -> error, if prd is not declared
```
