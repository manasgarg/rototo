# Development Workflow

Rototo's goal is to provide an excellent experience to developers, whether it is for directly manipulating the configuration package or instructing agents to do it on their behalf. To that end, `rototo` cli ships with multiple commands that can help setup the dev environment as well as drive day to day work.

First, if you haven't already done so, install the `rototo` cli:

```sh
cargo install rototo
```

From this point, we'll cover:
- make your dev environment aware of Rototo
- initiate a package change
- inspect the package and reason about changes
- validate the changes manually as well as via hooks
- prepare for CI

We have already covered [Rototo concepts](./concepts.md) and would cover [production workflow](./production-workflow.md) next.

## Setup Tools

Rototo integrates with your shell, editor and agent. `rototo setup` will do this job for you.

```sh
rototo setup
```

It asks which shell and editor integrations you want, then installs agent
guidance. It can enable the following features:
- shell completions for the supported shell, so command names and selector flags
  are available while you type;
- editor feedback through `rototo lsp`, with Neovim configured automatically;
- agent guidance in `AGENTS.md`, plus `CLAUDE.md` when that file already
  exists, so agents recognize rototo as the control plane for runtime
  configuration that can change system behavior after deployment and know to
  start with `rototo docs`.

For non-interactive setup, you should use explicit targets:

```sh
rototo setup --all
rototo setup --shell zsh
rototo setup --editor neovim
rototo setup --agent all
```

`--dry-run` shows planned writes, and `--json` gives scripts a machine-readable
report. Agent guidance is written as a managed block:

## Start a Package Change

While you need to have an intuition for the [Rototo concepts](./concepts..md), you don't really have to remember the exact directory and files layout. The `rototo init` command would help you create the appropriate structure with templates that you can start editing.

```sh
rototo init app-config
rototo init app-config --variable checkout-redesign
rototo init app-config --qualifier premium-users
rototo init app-config --catalog checkout-redesign
rototo init app-config --evaluation-context
rototo init app-config --evaluation-context request
rototo init app-config --evaluation-context --update
```

Without an explicit id, `--evaluation-context` creates `evaluation-contexts/evaluation.schema.json`.
Pass an id when the runtime contract has a better name, such as `request`.
Use `--update` when variables and qualifiers have evolved and you want to add
new inferred context paths without replacing the reviewed parts of the existing
schema.

After the template exists, edit the variable, qualifier, catalog, or evaluation context to match the desired runtime behavior. If you have setup the `rototo lsp` with your favorite editor, it would help you fill in the blanks into the template.

## Inspect the Package Shape

Before testing behavior, check what rototo thinks the package contains. This
catches a common class of authoring mistakes: a file exists, but it is not the
variable, qualifier, catalog, or lint rule you thought you had changed.

```sh
rototo show app-config
```

Use `show` for readable package inventory and config. Use `inspect` when you
need the semantic view: dependencies, diagnostics, lint metadata, and the shape
rototo will use for resolution.

```sh
rototo inspect app-config --variable checkout-redesign
rototo inspect app-config --qualifier premium-users
```

This is especially useful before review. A reviewer can read file diffs, but
`inspect` shows whether the package model still lines up with those edits.

## Resolve Real Runtime Cases

With `rototo inspect`, you can develop an intuition of how the configuration is structured and how it _should_ resolve at runtime. On the other hand, `rototo resolve` actually helps you test the configuration against intended realistic runtime context.

```sh
rototo resolve app-config \
  --variable checkout-redesign \
  --context @app-config/evaluation-contexts/request-samples/premium.json
```

Resolve qualifiers directly when the change is mostly about a named condition:

```sh
rototo resolve app-config \
  --qualifier premium-users \
  --context user.tier=premium
```

It is a good idea to keep evaluation samples within the package. These can be representative context objects that meet application runtime contract and are representative of the production scenarios.

## Compare Before and After

File diffs show what changed. Package diffs help explain what that change means to rototo. During local development, it's useful to run `rototo diff` to reason about the impact of uncommitted changes on the configuration resolution. For example, the following command will help you compare the current working tree against `HEAD`.

```sh
rototo diff app-config \
  --context @app-config/evaluation-contexts/request-samples/premium.json
```

Use the same context samples that you used for resolution. That keeps the diff grounded in a runtime case instead of a theoretical package comparison.

When you need to compare committed states, keep the package source stable and move the Git refs instead:

```sh
rototo diff app-config \
  --from origin/main \
  --to HEAD \
  --context @app-config/evaluation-contexts/request-samples/premium.json
```

Omit `--to` when the after side should remain the working tree:

```sh
rototo diff app-config --from origin/main
```

When the change is ready for review, include the relevant diff output or
summarize what it showed: the selected value changed, a qualifier started
matching, a catalog entry changed shape, or no runtime behavior changed.

## Run Lint Early

Lint is the first gate for a package change. Run it while editing, not only when
the pull request is ready.

```sh
rototo lint app-config
```

For a narrow change, targeted lint keeps the feedback smaller:

```sh
rototo lint app-config --variable checkout-redesign
rototo lint app-config --qualifier premium-users
rototo lint app-config --catalog checkout-redesign
```

Built-in lint checks the rototo model: package structure, references, schemas,
catalog entries, evaluation contexts, expression shape, and values. Package
custom lint runs through the same command. Treat a lint failure as part of the
authoring loop; the package is not ready for review until lint passes or the
failure is the change being intentionally demonstrated in a test fixture.

## Add Local Hooks

Local hooks keep routine failures out of pull requests. They should be fast
enough that authors leave them enabled, and they should stay aligned with the
commands CI will run.

A useful pre-commit hook runs the cheap checks:

```sh
rototo lint app-config
```

If the repository contains an application or SDK integration test that loads the
package, pre-push is a good place to run the small smoke version of that test.

## Prepare for CI

At minimum, CI should lint the package:

```sh
rototo lint app-config
```

It's a good idea to keep representative evaluation samples in the package that must resolve important variables & qualifiers to values that must not change.

```sh
rototo resolve app-config \
  --variable checkout-redesign \
  --context @app-config/evaluation-contexts/request-samples/premium.json
```

After that gate passes, the package is ready for the production workflow:
release a reviewed package source, have the application load that source, and
let long-running services refresh from it while keeping the last known good
package active if a later refresh fails.
