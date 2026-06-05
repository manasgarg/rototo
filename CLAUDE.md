# rototo Agent Guide

`rototo` is a Rust project. It is a git-backed configuration control plane:
configuration lives as reviewable workspace files in a git repository, rototo
validates those files, and apps can later source typed configuration values from
that workspace at runtime. Treat git as the source of truth and the workspace as
the control-plane boundary; avoid designs that assume an external database,
remote service, or generated state unless explicitly introduced.

## Project Principles

Keep `rototo` minimal. Add only what is necessary for the current checkpoint,
prefer the smallest working slice, and avoid scaffolding crates, abstractions,
features, or compatibility layers before they have real behavior to hold.

Backward compatibility is not required for the foreseeable future. Prefer clean,
small interfaces over compatibility shims, especially for SDK and CLI API
changes made before a public stability commitment.

Rototo should use async APIs for any operation that may block: filesystem I/O,
network fetches, subprocesses, archive extraction, Lua lint execution, and
workspace loading or resolution. Do not add new sync public APIs around blocking
work; use async functions and `spawn_blocking` for sync-only libraries.

Use rototo's domain vocabulary directly. Current first-class concepts are
workspaces, qualifiers, variables, resources, schemas, and values. Avoid
reintroducing generic nouns such as package, resource, item, or document in CLI
commands, public SDK types, docs, tests, or diagnostics unless the product model
explicitly changes.

## Current Shape

The workspace format is rooted at `rototo-workspace.toml`:

- `qualifiers/*.toml`: named qualifier definitions. The file stem is the
  qualifier id.
- `variables/*.toml`: named variable definitions. The file stem is the variable
  id.
- `schemas/*.json`: JSON Schemas referenced by schema-backed variables.

The example workspace at `examples/basic` is intentionally broad and should stay
lint-clean. It covers primitive variables, schema-backed nested values,
default values, override rules, qualifier composition, and bucket predicates.

There is no package model right now. Do not add package scaffolding unless the
design is reopened.

## CLI

The CLI intentionally makes qualifiers and variables first-class:

```text
rototo workspace inspect [workspace] [--workspace <workspace>]
rototo workspace lint [workspace] [--workspace <workspace>]
rototo qualifier list|get|lint|resolve|resolve-all ...
rototo variable list|get|lint|resolve|resolve-all ...
rototo diagnostics list|get ... [--workspace <workspace>]
rototo completions <shell>
```

Workspace arguments are sources, not only local paths. They can be local paths,
`file://`, `git+file://`, `git+https://`, `git+ssh://`, or `https://` archive
URLs. Plain `http://` workspace sources are intentionally unsupported. Git
sources support `#ref:subdir`; archive URLs support `#:subdir`. Bearer auth for
HTTPS archive sources comes from `--workspace-token` or
`ROTOTO_WORKSPACE_TOKEN`.

Do not add `rototo resource ...`. Qualifier and variable command enums are kept
separate in `src/main.rs` even when they currently share verbs, so each noun can
evolve independently.

Global flags are supported at every level:

- `--json`
- `--quiet`, `-q`

Resolution takes repeatable `--context` inputs in the CLI: raw JSON object,
`@path/to/context.json`, or `path=value`, merged left to right. Qualifiers are
ANDed predicates. A predicate can read context paths such as `user.tier` or
another qualifier via `qualifier.<id>`. Variables resolve by taking the first
matching rule value, otherwise the default value.

```sh
rototo qualifier resolve premium-users --workspace examples/basic \
  --context user.tier=premium

rototo variable resolve checkout-redesign --workspace examples/basic \
  --context @examples/basic/contexts/premium-enterprise.json
```

## Lint Expectations

Lint is core behavior, not just smoke testing. It should validate rototo's own
workspace structure and files:

- Workspace manifest exists, parses, and declares `schema_version = 1`.
- Qualifier files parse, declare `schema_version = 1`, contain at least one
  `[[predicate]]`, reference known qualifiers when
  using `qualifier.<id>`, use known predicate operators, and validate bucket and
  operator value shapes.
- Variable files parse, declare `schema_version = 1`, declare exactly one of
  `type`, `schema`, or `resource`, contain inline values under `[values]` and/or
  external values under a sibling `<variable-id>-values/*.toml` directory,
  declare `[resolve]`, reference known values, and reference known qualifiers
  from rules.
- Primitive variable values match `bool`, `int`, `number`, `string`, or `list`.
- Schema-backed variable values validate against their JSON Schema.
- Workspaces can declare custom rules in `rototo-workspace.toml` under
  `[[lint.rule]]` with `id`, `title`, and `help`. Lua files under `lint/*.lua`
  define `register(lint)` and register handlers with stage, entity, optional
  field, declared rule, and handler name. Handlers return diagnostics with
  `message`; the registration owns the rule id. `rototo` is reserved for
  built-in diagnostics.
- Standalone `schemas/*.json` files parse and compile as JSON Schema.

Diagnostics use one stable `rule` identity. Built-in rototo rules use
`rototo/<rule-id>` with a flat rule id, for example
`rototo/variable-unknown-value`; they must not use nested `rototo/*/<rule-id>`
paths. Lua/custom lint rules use
`<authority>/<rule-id>` with a non-`rototo` authority, for example
`payments/max-token-budget`. The diagnostics catalog lists built-in rules
globally and adds declared custom rules for a workspace-scoped catalog.

The failure fixture at `tests/fixtures/workspaces/lint-failures` is a compact
coverage workspace for expected lint failures. Extend it when adding new lint
rules.

## SDK

The Rust SDK mirrors the first-class model. Prefer explicit APIs such as:

- `inspect_workspace`
- `lint_workspace`, `lint_qualifier`, `lint_variable`
- `list_qualifiers`, `list_variables`
- `read_qualifier`, `read_qualifiers`, `read_variable`, `read_variables`
- `resolve_qualifier`, `resolve_qualifiers`
- `resolve_variable`, `resolve_variables`

All SDK APIs that touch workspace files, source loading, lint, or resolution are
async. `Workspace::load(source).await` accepts the same source forms as the CLI,
lints the loaded workspace, and rejects lint failures.
`Workspace::inspect(source).await` is the lower-level loader for tools that need
staged workspace data without running lint. Both APIs own any temporary staged
checkout/archive extraction needed by remote sources.

Returned config types are `QualifierConfig` and `VariableConfig`. Avoid adding a
generic public "read by kind" API unless there is a concrete app-facing need.
SDK resolution APIs take a JSON object context directly; the CLI-only
convenience forms for `--context` are parsed in `src/main.rs`.

## Commands

Use `just` as the project command surface:

```sh
just setup
just fmt
just lint
just test
just check
```

Run `just check` before reporting that a code change is complete. CI runs the
same command.

Commit logical chunks as work progresses. If a commit hook rejects an
intentionally invalid fixture used to test lint failures, it is acceptable to
commit with `--no-verify` after `just check` passes and the reason is clear.

## Setup

Rust is pinned by `rust-toolchain.toml`. Python is pinned by `.tool-versions`
through `mise` and is used for `pre-commit`.

`just setup-min` installs the local pre-commit and pre-push hooks.

## Documentation Guidance

Write rototo documentation for engineers and agents who need to understand,
operate, and modify runtime configuration safely.

Do not write documentation as a feature catalog. Write each page as a guided
argument:

1. Start from a concrete operational problem the reader can recognize.
2. Explain why the problem matters.
3. Introduce the smallest rototo concept that addresses it.
4. Show how that concept composes with the next concept.
5. End with what the reader can now do or understand.

Always explain motivation before syntax. Before introducing a file, field, CLI
command, SDK type, or abstraction, explain why it exists and what failure mode
it prevents.

Use causal transitions between sections. Each section should tell the reader why
the next section exists. Avoid abrupt jumps between independent feature
descriptions.

Prefer concrete nouns and examples over abstract claims. Avoid marketing
language such as "easy", "simple", "powerful", or "seamless" unless the
sentence explains exactly why.

Use this conceptual ordering unless the page has a narrower purpose:

1. Runtime configuration is a production control problem.
2. Rototo stores that control plane as a Git-versioned workspace.
3. Applications load a workspace source rather than embedding config values.
4. Applications resolve named variables using runtime context.
5. Context schemas validate request-time facts supplied by the app.
6. Qualifiers turn runtime facts into named reusable conditions.
7. Variables select configured values using defaults and qualifier rules.
8. Value schemas validate the selected value before the application consumes it.
9. Linting and tests make the workspace releasable.
10. Long-running services refresh the workspace and keep last-known-good state.
11. Observability explains what was selected, from which workspace version, and
    why.

Treat refresh as part of the core runtime model, not as an incidental SDK
feature. Make clear that:

- configuration is deployed separately from the application binary;
- the application is deployed with a workspace source URI;
- the SDK loads the workspace at startup;
- long-running services can periodically refresh from the same source;
- successful refreshes affect future resolutions;
- failed refreshes keep the last successfully loaded workspace active;
- immutable commit refs are reproducible but do not produce new refresh results.

Keep page roles distinct:

- `why-rototo`: motivate the problem, current failure modes, rototo's model, and
  runtime architecture.
- `quickstart`: provide a short first success with one small local example and
  enough mental model to make it land.
- `production-workflow`: show a realistic Git-backed workflow with schemas,
  qualifiers, variables, tests, app loading, refresh, and observability.
- Concepts pages: define vocabulary, relationships, resolution flow,
  guarantees, and boundaries without becoming tutorials.
- Reference pages: specify exact file formats, commands, SDK APIs, diagnostics,
  and JSON output.

When drafting a concepts page, do not begin with a glossary. Start from the
reader's question, for example:

> When my application asks for configuration at runtime, what exactly is rototo
> evaluating?

Then explain the flow: application asks for a variable with runtime context from
a workspace version; rototo validates context, evaluates qualifiers, checks
rules, selects a value, validates the value, and returns the result with enough
explanation to debug or observe it.

Use engineering prose:

- direct;
- precise;
- causal;
- low on adjectives;
- explicit about tradeoffs;
- clear about boundaries.

Avoid:

- duplicate explanations across nearby sections;
- syntax before motivation;
- long lists of rototo nouns without transitions;
- toy examples that are not relatable to the page's goal;
- unexplained abstractions;
- implying config is fixed at deployment time;
- implying applications must redeploy to receive reviewed config changes.
- audience/experimentation vocabulary such as "segment" or "cohort"; prefer
  "runtime condition", "named condition", "account class", or "bucket" as
  appropriate.
