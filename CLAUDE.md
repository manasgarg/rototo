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
workspaces, qualifiers, variables, catalogs, schemas, and values. Avoid
reintroducing generic nouns such as package, catalog, item, or document in CLI
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

Do not add `rototo catalog ...`. Qualifier and variable command enums are kept
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

## Console

`rototo console` serves the web console and its JSON API from the same binary
as the CLI. The Rust server lives in `src/console/` and owns all data access:
workspace staging, lint, the semantic model, resolution previews, the GitHub
REST write path (draft branches, file commits, pull requests), an in-process
LSP bridge, and a SQLite store for repos/workspaces/drafts/sessions under the
console data directory. The frontend in `apps/console` is a Vite + React
static SPA with no server runtime; it talks only to `/api/*` and its built
`dist/` bundle is embedded into the binary (staged via `build.rs`, served by
`rust-embed`).

Auth modes are resolved at startup: local (default — no login; ambient GitHub
token from `--workspace-token`/`ROTOTO_WORKSPACE_TOKEN`, a stored device-flow
sign-in, or `gh auth token`), team (`GITHUB_CLIENT_ID` + `GITHUB_CLIENT_SECRET`
turn on the GitHub OAuth web flow with per-user tokens encrypted via
`ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY`), and read-only (`--read-only
--workspace <source>`, no auth, writes rejected). Mutating routes require the
`x-rototo-console` header plus an Origin check; keep that invariant when
adding routes. Console writes go through the GitHub API only — do not add a
generic git write backend without reopening the design.

Wire shapes are serde camelCase and mirrored in
`apps/console/src/lib/types.ts`; the Rust server is the source of truth.
Keep the console feature flag in Cargo.toml: SDK binding crates build with
`default-features = false` so the server stack stays out of their artifacts.

## Lint Expectations

Lint is core behavior, not just smoke testing. It should validate rototo's own
workspace structure and files:

- Workspace manifest exists, parses, and declares `schema_version = 1`.
- Qualifier files parse, declare `schema_version = 1`, contain at least one
  `[[predicate]]`, reference known qualifiers when
  using `qualifier.<id>`, use known predicate operators, and validate bucket and
  operator value shapes.
- Variable files parse, declare `schema_version = 1`, declare exactly one of
  `type`, `schema`, or `catalog`, contain inline values under `[values]` and/or
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

### Language-Specific SDKs

Language-specific SDKs should be thin, idiomatic bindings around the Rust SDK.
Rust remains the semantic authority for workspace loading, lint, source
staging, refresh, qualifier evaluation, variable resolution, context
validation, and error behavior. Do not reimplement rototo semantics in Python,
Node, Go, Java, or other SDKs unless the design is explicitly reopened.

Keep each language SDK's first surface small and runtime-focused:

- load or inspect a workspace source;
- resolve variables and qualifiers with a JSON object context;
- expose refresh for long-running services;
- map Rust errors into the language's normal error type;
- convert JSON values into the language's native JSON-compatible values;
- clean up background refresh tasks or native handles.

Add list, read, trace, diagnostics catalog, fixture, or inspection helpers only
when there is a concrete app or tool need in that language. Prefer adding the
same concept across SDKs intentionally rather than letting one SDK accumulate
incidental convenience APIs.

All language SDKs should preserve the async runtime model. If a binding layer
must cross a sync foreign-function boundary, hide that detail inside the
binding and keep the public SDK operation awaitable, future-based, promise-based,
or otherwise idiomatic for that ecosystem.

Use one shared contract suite for cross-language behavior. Shared cases live as
data, not duplicated language test code. Rust tests should validate the shared
cases against the Rust SDK. Each language SDK should run the same cases through
its own public API and keep language-local tests focused on wrapper behavior:
import/install, option translation, JSON conversion, error mapping, async
lifecycle, refresh shutdown, and packaging. Rust keeps the exhaustive semantic
tests for lint, resolution, schemas, bucket behavior, source loading, and
refresh failure handling.

Package tests should install the SDK as a user would, then run a small smoke
and contract suite from the installed package. Do not rely only on in-tree
imports; native extension loading, wheel metadata, and package exports are part
of the SDK contract.

Use one canonical rototo release version: SemVer, for example
`0.1.0-alpha.3`. Rust crates, git tags, docs, and SDK runtime version exports
should use that canonical version. Package registries may require or display an
ecosystem-native normalized equivalent, such as Python/PyPI's `0.1.0a3`; do
not switch the canonical version to a registry-specific spelling. Language SDKs
should expose the canonical rototo version at runtime when the ecosystem has a
version field such as `__version__`.

Documentation should share prose and switch code snippets by language. Use
inline SDK snippet groups in Markdown rather than separate duplicated pages for
the same concept:

````text
:::sdk-snippet load-workspace
```rust
...
```

```python
...
```
:::
````

The docs renderer should show the selected SDK language consistently across the
site. Shared SDK pages explain semantics such as loading, resolution, and
refresh. Per-language reference pages specify exact install commands, imports,
types, options, return values, and error behavior.

Do not hand-maintain multiple package READMEs with duplicated prose. Author the
language-specific package README content once in the docs source, generate the
packaged README from that source, commit the generated file only when the
ecosystem requires it for packaging, and add a freshness check so package
README content cannot drift from the docs.

Release all SDKs from the same tag. A `v<version>` tag should publish every
supported ecosystem package for that version after all release artifacts have
been built and checked. Use `just release-prep <version>` before tagging to
update the canonical version surfaces, refresh generated package READMEs, and
run the local release gate. Use `just release-check <version>` in CI before any
publish step so tag names, manifests, and generated package content cannot
drift.

Java SDK releases publish `dev.rototo:rototo` to Maven Central through the
Central Portal Maven plugin. The published JAR should be built by Maven and
include generated native-library catalogs for every supported platform, plus
the sources JAR, javadoc JAR, POM metadata required by Central, GPG signatures,
and Central-generated checksums. Do not hand-build the Maven Central artifact in
the release workflow. Release automation expects Central Portal token secrets
named `CENTRAL_USERNAME` and `CENTRAL_PASSWORD`, and GPG secrets named
`MAVEN_GPG_PRIVATE_KEY` and `MAVEN_GPG_PASSPHRASE`.

The Go SDK lives under the root Go module as
`github.com/manasgarg/rototo/sdks/go`, so the same root `v<version>` tag is the
Go module version. The first Go SDK is a cgo binding over the Rust SDK. Local
tests build the `rototo-go` cdylib and set `ROTOTO_GO_NATIVE_PATH`; future
packaging may add platform-native assets, but Go wrapper tests should continue
to run the shared SDK contract through the public Go API.

## Commands

Use `just` as the project command surface:

```sh
just setup
just fmt
just lint
just test
just console-build
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

`just setup` installs the pinned local tools, console dependencies, and local
pre-commit and pre-push hooks.

## Documentation Guidance

Write rototo documentation for engineers and agents who need to understand,
operate, and modify runtime configuration safely.

### Documentation Voice

Write rototo docs in the voice of a senior engineer sharing work with other
engineers. The voice should feel practical, experienced, and warm without
becoming casual or promotional.

The default stance is:

- authored, not anonymous;
- precise, not sterile;
- confident from experience, not from hype;
- empathetic about production risk;
- excited by working systems, not by feature claims.

Use first person sparingly, when it communicates engineering judgment or
experience. Do not use it for every instruction. A page should still be about
the reader's operational problem, not the author.

Good examples:

> I like starting with one value because it keeps the whole system honest.

> That starts with a qualifier. The qualifier gives the runtime condition a name
> before we wire it into a variable or turn its context path into a schema
> contract.

> The useful part is that none of this changes the core shape.

> I am using `RefreshingWorkspace` even in the first app because refresh is part
> of the runtime model.

Avoid empty hype or feature-catalog phrasing:

> rototo makes configuration easy and seamless.

> This powerful feature lets you manage config effortlessly.

> Users can quickly create segments and target cohorts.

Good rototo docs should explain:

1. what production failure mode we are avoiding;
2. why the next concept exists;
3. what small working slice to build;
4. how that slice grows into production practice.

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
- `getting-started`: provide a short first success with one small local example and
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
