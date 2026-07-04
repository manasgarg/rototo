# The CLI

The `rototo` command line is how you create, check, and explore a package by
hand - and how CI does the same thing automatically. This page is the reference
for every command and flag.

Everything follows one shape:

```text
rototo <verb> [package-source] [selectors] [flags]
```

A **verb** says what to do (`lint`, `resolve`, `show`…). The **package source**
says which package to do it to. **Selectors** narrow it down to specific
variables or catalogs. And a few **flags** work everywhere. Let's cover the
things that are shared first, then go command by command.

## The package argument

Most commands take a package source as their first argument. That's the same
flexible string described in full on the [package sources](./package-sources.md)
page - a local folder, a git repo, an HTTPS archive.

And for the everyday commands, you can leave it off entirely. When you do, rototo
looks in the current directory and walks upward until it finds a
`rototo-package.toml`. So when you're standing inside your package, `rototo lint`
just works.

(A couple of commands - `init` and `diff` - only make sense against a local
folder, so their argument is a plain path, not a full source. Noted below where
it matters.)

## Global flags

These three work on every command:

- **`--json`** - emit machine-readable JSON instead of the human-readable view.
  This is the stable interface for scripts and CI.
- **`--quiet`** - suppress the "all good" success output from lint. Failures
  still print.
- **`--package-token <token>`** - a bearer token for private HTTPS archive
  downloads. You can also set `ROTOTO_PACKAGE_TOKEN` in the environment instead,
  which is usually nicer for CI and services.

## Selectors

Several commands - `lint`, `show`, `inspect`, `resolve`, `fixtures` - let you
zoom in on part of the package instead of the whole thing. The pattern is always
the same: a singular flag picks one thing by id (and repeats), a plural flag
picks all of that kind.

```sh
rototo lint --variable checkout_redesign        # just this variable
rototo lint --variable a --variable b           # a couple of them
rototo lint --variables                          # every variable
```

The full set of selectors:

| Singular (repeatable) | Plural (all) | Picks |
| --- | --- | --- |
| `--variable <id>` | `--variables` | variables |
| `--catalog <id>` | `--catalogs` | catalogs |
| `--lint-rule <authority/rule>` | `--lint-rules` | diagnostic rules |
| `--lint-authority <authority>` | `--lint-authorities` | lint authorities |
| `--linter <id>` | `--linters` | your Lua linters |

Not every command takes every selector - `resolve` and `fixtures`, for instance,
only deal in variables, because resolving a catalog or a lint rule doesn't mean
anything. The per-command sections below say which ones apply.

## Context inputs

The commands that evaluate things against runtime facts - `resolve`, `inspect`,
`diff` - take `--context`. You can give it three ways, and they merge left to
right, with later values winning:

```sh
# a path=value pair (dots build nested objects)
--context user.tier=premium

# a raw JSON object
--context '{"user": {"tier": "premium"}}'

# a file of JSON, with a leading @
--context @app-config/model/context/request-samples/premium.json
```

Mixing them is fine: `--context @base.json --context user.tier=premium` loads a
file and then overrides one field. If you pass nothing, the context is `{}`.

---

## init - start a new package or add to one

`init` writes files for you so you don't have to remember the folder layout. The
argument is a local path (not a source).

```sh
rototo init app-config                              # a fresh, empty package
rototo init app-config --variable free_shipping     # + a variable template
rototo init app-config --catalog checkout_redesign  # + a catalog template
rototo init app-config --evaluation-context         # + an evaluation context
rototo init app-config --evaluation-context request # named "request"
```

Each `--variable`, `--catalog`, and `--evaluation-context` takes the id to
create. `--evaluation-context` is special: give it a name, or leave the
name off and it creates `model/context/evaluation.schema.json`.

Useful extras:

- **`--update`** (with `--evaluation-context`) - add newly inferred context paths
  to an existing schema without clobbering what you've already reviewed.
- **`--dry-run`** - print what *would* be written, change nothing.
- **`--force`** - overwrite files this command created before. (Can't combine
  with `--update`.)

## lint - check the package

`lint` is the first gate. It validates the whole rototo model: files parse,
references resolve, schemas compile, values match their types, catalog entries
fit their schemas, custom Lua registers cleanly. Run it constantly while editing,
not just at the end.

```sh
rototo lint                                  # the package you're standing in
rototo lint app-config                       # a specific one
rototo lint app-config --variable checkout   # just one variable
rototo lint app-config --json                # structured output
```

Takes all the selectors. What it reports - the diagnostics, their rule names, and
the `--json` shape - is covered on the [diagnostics](./diagnostics.md) page. The
short version of `--json`:

```json
{
  "package": "/abs/path/app-config",
  "documents": [ { "id": 0, "path": "rototo-package.toml", "kind": "manifest" } ],
  "diagnostics": [ ]
}
```

An empty `diagnostics` list means a clean package.

## show - a readable inventory

`show` is the quick "what's in here?" It lists the package's variables,
catalogs, and lint metadata in an easy-to-read form. Reach for it when you want
to confirm a file you edited actually became the variable or catalog you
intended.

```sh
rototo show app-config
rototo show app-config --variables
rototo show --lint-rules            # the diagnostics catalog (see Diagnostics)
```

Takes all the selectors, and supports `--json`.

## inspect - the deep view

`inspect` is `show`'s thorough sibling. Where `show` gives you the inventory,
`inspect` explains how rototo *sees* the package - dependencies, diagnostics,
lint metadata, and the shape it'll use to resolve values. It's especially handy
right before review: a reviewer reads the file diff, and `inspect` shows whether
the model still lines up with those edits.

```sh
rototo inspect app-config --variable checkout_redesign
rototo inspect app-config --variable premium_users
rototo inspect app-config --context user.tier=premium
```

Takes all the selectors, plus `--context` (so you can see how things look against
specific facts), and `--json`.

## resolve - evaluate against real facts

`resolve` is where you test behavior. Hand it a context and it tells you what a
variable actually comes out to. This is also the command CI leans on to protect
the cases that must never drift.

```sh
# a variable, against a sample context
rototo resolve app-config --variable checkout_redesign \
  --context @app-config/model/context/request-samples/premium.json

# a condition variable, against an inline fact
rototo resolve app-config --variable premium_users --context user.tier=premium
```

It only takes the variable selectors (resolving a catalog or rule doesn't mean
anything), plus `--context`.

The human output walks the resolution pathway: each rule prints as
`rule[N] if <condition> -> <value> (matched|skipped)`, then the default, then
the result with its source. For a package composed through `extends`, the pathway
starts with a `resolve from <source>` line naming the package whose `[resolve]`
block is being evaluated - the same provenance the trace carries. A condition
variable reads the same way - its value
just happens to be `true` or `false`. An allocation-backed variable prints its
assignment instead of rules, e.g.
`allocation checkout/cta_copy_test -> bucket 967 -> arm benefit_led`.

The `--json` output is the stable interface for tests. A variable resolution
gives you the chosen `value`, where it came from (`source`), and - handy for
debugging - the default and every rule that was considered:

```json
{
  "package": "app-config",
  "variables": [
    {
      "resolution": {
        "id": "checkout_redesign",
        "value": { "variant": "premium" },
        "source": { "kind": "catalog", "catalog": "checkout_redesign", "value": "premium" }
      },
      "default_value": { "variant": "control" },
      "default_source": { "kind": "catalog", "catalog": "checkout_redesign", "value": "control" },
      "rules": [
        { "index": 0, "condition": "variables[\"premium_users\"]", "value": { }, "matched": true }
      ]
    }
  ]
}
```

The `source.kind` is `literal` for a plain value, `catalog` for a single catalog
entry, or `catalog_list` for a `list<catalog:...>` value.

## diff - what changed, behaviorally

A file diff shows *what* changed in the TOML. `diff` shows what that change
*does* - which variable now picks a different value, which condition variable
started coming out true. The argument is a local path, because it works across
git refs of that checkout.

```sh
# working tree vs HEAD (the default)
rototo diff app-config --context @app-config/model/context/request-samples/premium.json

# compare two committed states
rototo diff app-config --from origin/main --to HEAD --context @.../premium.json

# leave --to off to keep the "after" side as your working tree
rototo diff app-config --from origin/main
```

- **`--from <ref>`** - the "before" side. Defaults to `HEAD`.
- **`--to <ref>`** - the "after" side. Defaults to your current working tree.
- **`--context`** - the facts to report impact against. Use the same samples you
  resolve with, so the diff stays grounded in a real case.

One family of changes gets extra attention: editing an allocation's arm
claims. Not every claim edit carries the same risk, so `diff` classifies the
blast radius instead of just saying "arms changed". Growing an arm into
buckets nothing claimed before only enrolls new units - already-enrolled
units keep the value they had. That reports as `allocation_arms_expanded`.
But moving a claimed bucket to a different arm, or releasing it back to the
default, changes what already-enrolled units receive - that reports as
`allocation_arms_reassigned`, and it's the one a reviewer should stop on.
Both kinds carry the counts in an `impact` line (a `detail` object in
`--json`): `claimed_buckets`, `released_buckets`, and `reassigned_buckets`.
Changing a layer's `unit` or bucket count reports as
`layer_diversion_changed`, which reshuffles every unit's position and is the
biggest hammer of all.

## fixtures - print ready-to-run resolve commands

`fixtures` generates readable examples of how your package resolves - as actual
`rototo resolve` commands you can run or paste into docs and reviews. It's a way
to turn "here's how this behaves" into something concrete.

```sh
rototo fixtures app-config --variables
rototo fixtures app-config --variable checkout_redesign
rototo fixtures app-config --variables --context-form json
```

Takes the variable selectors. The one extra flag:

- **`--context-form path|json`** - how the printed context looks. `path` (the
  default) breaks it into `--context a.b=value` arguments; `json` emits a single
  `--context '<json>'`.

## package - build a distributable archive

`package` bundles the package into a deterministic, content-addressed `.tar.gz`
for production distribution - the artifact you upload to an object store. (Same
idea as the [package sources](./package-sources.md) and
[format](./package-format.md) pages describe.)

```sh
rototo package app-config                     # writes into the current directory
rototo package app-config -o ./dist           # ...or somewhere else
rototo package app-config --unpacked ./flat   # plain directory, not an archive
```

- **`--output <dir>`** / **`-o`** - where to write the archive. Defaults to `.`.
- **`--unpacked <dir>`** - write the flattened projection as a plain directory
  instead of an archive. Same pipeline as the archive: `extends` parents are
  merged in, update and deleted markers are consumed, the manifest drops its
  `extends` key, and lint has to pass. This is the easiest way to see exactly
  what an overlay composes to. The target directory must be empty or absent -
  `package` refuses to write over existing files. Mutually exclusive with
  `--output`.

The archive file is named by its own SHA-256 digest, so the same package always
produces the same file name and bytes.

## docs - read the bundled documentation

rototo ships its own docs inside the binary - these very pages - so you (and your
agent) can read them offline.

```sh
rototo docs                       # list the pages
rototo docs -p concepts           # render a page by id prefix
rototo docs -s "refresh"          # search pages with a regex
rototo docs --export ./site       # export the whole thing as static HTML
```

- **`--page <prefix>`** / **`-p`** - render the page whose id starts with this.
- **`--search <regex>`** / **`-s`** - search across pages.
- **`--export [dir]`** - write the docs as a static HTML site (defaults to
  `./site`).

Three more flags exist for the release tooling, which generates each SDK's
package README from these docs so registry pages can't drift from the source:

- **`--package-readme <sdk>`** - generate the packaged README for one SDK:
  `python`, `typescript`, `java`, or `go`.
- **`--out <file>`** - where to write it, e.g. `sdks/python/README.md`.
- **`--docs-base-url <url>`** - the base URL to use for internal docs links in
  the generated README, since relative links don't resolve on a registry page.

A freshness test in CI regenerates these and fails if the committed files
differ, so if you edit the root README, rerun the generation for all four SDKs.

## setup - wire up your shell, editor, and agent

`setup` connects rototo to the tools around it: shell completions, editor
feedback through the language server, and guidance for your coding agent.

```sh
rototo setup                  # interactive - asks what to connect
rototo setup --all            # connect everything it can
rototo setup --shell zsh
rototo setup --editor neovim
rototo setup --agent claude
```

- **`--all`** - set up every supported integration.
- **`--shell <shell>`** - `auto`, `bash`, `fish`, `zsh`, `elvish`, `powershell`,
  or `none`.
- **`--editor <editor>`** - `all`, `neovim`, or `none`.
- **`--agent <agent>`** - `all`, `claude`, `codex`, or `none`. Agent guidance is
  written into a clearly marked, managed block.
- **`--print`** - print the generated content instead of writing files.
- **`--dry-run`** - show planned changes without touching the filesystem.
- **`--force`** - overwrite rototo-owned generated files that already exist.

## console - the web UI

`console` serves the rototo console - a web UI plus a JSON API - from the same
binary, over a package.

```sh
rototo console --package app-config
rototo console --package app-config --bind 127.0.0.1:8080
```

The main flags:

- **`--package <source>`** - the package to open at startup.
- **`--bind <addr>`** - the address to listen on.
- **`--data-dir <dir>`** - where console state lives (also
  `ROTOTO_CONSOLE_DATA_DIR`).
- **`--public-url <url>`** - the public origin, for running behind a reverse
  proxy (also `ROTOTO_CONSOLE_PUBLIC_URL`).
- **`--state ephemeral|persistent`** - whether console state (repos, drafts,
  sessions) survives a restart. Defaults to `ephemeral` for a local folder
  package and `persistent` otherwise.
- **`--deployment local|hosted`** - single-user local mode versus a hosted
  deployment. Defaults to `local` when `--package` is given, `hosted`
  otherwise.
- **`--write disabled|pull-request|direct-push`** - how branch edits leave the
  console: not at all, as GitHub pull requests, or pushed directly. Defaults
  to `direct-push` for a local fixed package and `pull-request` otherwise.

## lsp - the language server

`lsp` runs the rototo language server over stdin/stdout. You don't usually run
this by hand - `rototo setup` points your editor at it. It's what gives you
inline feedback and help while editing package files.

```sh
rototo lsp
```

No arguments.
