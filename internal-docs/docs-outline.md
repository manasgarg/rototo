# Public documentation outline

This outline describes the intended public documentation structure. Build it one
page at a time and keep `docs/src` focused on user-facing documentation that can
be bundled into the CLI and published to the docs site.

## Concepts

Explanation-oriented pages. These should help readers understand what rototo is,
why the model exists, and how the pieces fit together.

- `concepts/overview.md`
- `concepts/mental-model.md`
- `concepts/resolution-model.md`
- `concepts/workspace-model.md`
- `concepts/safety-and-validation.md`

## Tutorials

Linear, guided learning paths for first-time users.

- `tutorials/quickstart.md`
- `tutorials/build-a-workspace.md`
- `tutorials/embed-with-rust-sdk.md`

## How-to guides

Task-oriented guides for readers who already know the basic model.

- `how-to/define-a-qualifier.md`
- `how-to/define-a-variable.md`
- `how-to/use-context-schema.md`
- `how-to/use-custom-lint.md`
- `how-to/use-directory-backed-values.md`
- `how-to/use-remote-workspaces.md`
- `how-to/export-and-serve-docs.md`

## Reference

Exhaustive, precise documentation for every public contract and behavior.

- `reference/workspace-manifest.md`
- `reference/qualifier-file.md`
- `reference/variable-file.md`
- `reference/predicates.md`
- `reference/context.md`
- `reference/environments.md`
- `reference/value-types.md`
- `reference/source-uris.md`
- `reference/cli.md`
- `reference/sdk.md`
- `reference/diagnostics.md`
- `reference/json-output.md`

## Examples

Concrete usage patterns that map rototo onto real application concerns.

- `examples/feature-flags.md`
- `examples/runtime-config.md`
- `examples/tenant-config.md`
- `examples/llm-config.md`

## Agent guide

A compact orientation page optimized for coding agents.

- `agent-guide.md`

The agent guide should include core invariants, workspace layout, file schemas,
common commands, resolution behavior, diagnostics workflow, and examples of safe
edits.
