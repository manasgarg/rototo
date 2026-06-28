# Development Workflow

Runtime configuration changes are production changes. The difference is that
they are expressed as package files instead of application code, and that makes
the development loop a little different. The useful workflow is not just "edit a
TOML file and hope lint catches it." A package author needs fast local feedback,
package-aware editor diagnostics, realistic resolution checks, and instructions
that keep humans and agents working inside the same boundary.

This page starts there. It assumes you have read the concepts page or already
know what packages, variables, qualifiers, catalogs, evaluation contexts, and
lint are. The goal here is narrower: set up an authoring environment that makes
package changes reviewable before they reach CI.

The development loop looks like this:

```text
set up tools -> edit package -> lint -> resolve -> capture fixtures -> review
```

Production concerns such as hosting package archives, application refresh,
promotion, rollback, and observability belong in the production workflow. During
development, the job is to make the intended runtime behavior visible enough
that another engineer, or an agent, can review it with confidence.

## Setup Tools

`rototo setup` configures the local integrations that make that feedback
available while you work: shell completions, editor integration, and agent
guidance.

```sh
rototo setup
```

In an interactive terminal, setup asks which integrations to enable. On a normal
workstation, accept the prompts that match how you edit packages. The resulting
environment gives you:

- shell completions for the supported shell, so command names and selector flags
  are available while you type;
- editor feedback through `rototo lsp`, with Neovim configured automatically and
  VS Code reported as a manual LSP-client step;
- agent guidance in `AGENTS.md` for Codex and `CLAUDE.md` for Claude, so agents
  use package files as the control-plane boundary and run `inspect`, `resolve`,
  and `lint` before finishing.

For non-interactive setup, use explicit targets:

```sh
rototo setup --all
rototo setup --shell zsh
rototo setup --editor neovim
rototo setup --agent codex
```

`--dry-run` shows planned writes, and `--json` gives scripts a machine-readable
report. Agent guidance is written as a managed block:

```md
<!-- BEGIN rototo setup -->
...
<!-- END rototo setup -->
```

Setup can replace that managed block later without taking ownership of the rest
of the file. Project-specific instructions, review rules, and repository
conventions should live outside the managed block.

The command is deliberately local. It prepares your shell, editor, and nearby
agent instruction files. It does not create a package, change application code,
or publish anything.

## Start a Package Change

Use `rototo init` when you create or extend a package. It gives you the current
file shape and keeps the first edit focused on the behavior you want to model,
not on remembering directory names.

```sh
rototo init app-config
rototo init app-config --variable checkout-redesign
rototo init app-config --qualifier premium-users
rototo init app-config --catalog checkout-redesign
rototo init app-config --evaluation-context
```

Those commands create templates. They do not decide the production policy for
the package. After the template exists, edit the variable, qualifier, catalog,
or evaluation context to match the runtime behavior under review. If the shape
of those files is unfamiliar, use the [concepts page](./concepts.md) first.

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

## Resolve Real Runtime Cases

Lint proves that the package is coherent. Resolution proves that the package
selects the value you intended for a realistic runtime input.

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

Prefer sample context files for important cases. Inline `--context path=value`
is useful while experimenting, but a sample under `evaluation-contexts/` gives
the reviewer the same runtime facts you used. It also keeps the package's
assumptions about application input visible next to the rules that depend on
that input.

## Capture Behavior as Fixtures

Once a change resolves the way you expect, capture that behavior as a fixture.
The point is not to test TOML syntax again. The point is to make selected values
and qualifier results visible in review.

```sh
rototo fixtures app-config \
  --variables \
  --qualifiers \
  --out tests/fixtures/rototo
```

Commit fixture changes when they describe intentional behavior. A fixture diff
should answer a concrete review question: which runtime case changed, and what
does rototo now return for it?

Fixtures work best when they are small and named after operational cases the
team recognizes. Avoid generating a large snapshot nobody will read. If a
variable or qualifier is risky enough to deserve a package change, it usually
deserves at least one realistic fixture case.

## Compare Before and After

File diffs show what changed. Package diffs help explain what that change means
to rototo. During local development, the default comparison is the useful one:
the package at `HEAD` against the current working tree.

```sh
rototo diff app-config \
  --context @app-config/evaluation-contexts/request-samples/premium.json
```

Use the same context samples that you used for resolution. That keeps the diff
grounded in a runtime case instead of a theoretical package comparison.

When you need to compare committed states, keep the package source stable and
move the Git refs instead:

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

## Add Local Hooks

Local hooks keep routine failures out of pull requests. They should be fast
enough that authors leave them enabled, and they should stay aligned with the
commands CI will run.

A useful pre-commit hook runs the cheap checks:

```sh
rototo lint app-config
```

A useful pre-push hook can be broader:

```sh
rototo lint app-config
rototo fixtures app-config --variables --qualifiers --out tests/fixtures/rototo
```

If the repository contains an application or SDK integration test that loads the
package, pre-push is a good place to run the small smoke version of that test.
Do not make hooks the final authority. They help authors move faster, but CI is
the release gate.

## Open a Reviewable Change

A reviewable package change explains behavior, not only files. Before opening
the pull request, check that the change has enough evidence for someone else to
reason about it.

The review should make these points clear:

- what runtime behavior is intended to change;
- which variables, qualifiers, catalogs, or evaluation contexts changed;
- whether `rototo lint` passes;
- which realistic contexts were resolved locally;
- whether fixture output changed, and why;
- whether custom lint changed;
- what `rototo diff` showed for the important runtime cases.

That checklist is useful for human reviewers and for agents. It keeps the
discussion centered on the package as the control-plane boundary: reviewed
files, validated semantics, and observable resolution behavior.

## Hand Off to Production

Development is done when the package change is ready for CI to be authoritative:

```text
lint passes
realistic resolves match intent
fixtures are current
review explains behavior impact
```

At that point the package moves into the production workflow. Production covers
the parts this page intentionally leaves out: publishing package sources,
hosting archives, loading the package from an application, refreshing
long-running services, promoting changes, rolling back, and observing which
package version selected which values.
