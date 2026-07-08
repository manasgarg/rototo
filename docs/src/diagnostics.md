# Diagnostics

When you run `rototo lint`, what comes back is a set of **diagnostics** - each
one a specific thing that's wrong, or worth a second look, in your package. This
page explains how to read a diagnostic, how the built-in checks are organized,
and how your own custom checks fit in alongside them.

The thing to hold onto: lint isn't a smoke test that just says "looks fine." It
actually understands rototo's model - that a variable's type matches its values,
that a rule references a real variable, that a catalog entry fits its
schema - and each kind of problem has its own stable name you can point at.

## What a diagnostic looks like

Every diagnostic has the same parts, whether it came from a built-in check or one
you wrote:

- a **rule** - the stable name for *this kind* of problem, like
  `rototo/variable-unknown-value`. This is the part you can search for, suppress,
  or talk about in a review.
- a **severity** - either `error` or `warning`. Errors mean the package isn't
  releasable; warnings are things worth knowing that don't block you.
- a **message** - what specifically went wrong, this time.
- a **help** - a fixed hint on how to fix that kind of problem.
- a **location** - which file, and where in it.

In your terminal that's printed as readable text. With `--json` you get the
structured version, covered at the end of the page.

## Rule names: who's talking

The first part of a rule name tells you where the check came from - its
**authority**.

Built-in rototo checks always start with `rototo/`, followed by a flat,
hyphenated name:

```text
rototo/variable-unknown-value
rototo/variable-reference-cycle
rototo/catalog-entry-schema-mismatch
```

It's always exactly one level - `rototo/something`, never
`rototo/variables/something`. That flatness is on purpose, so a rule name is one
stable string you can grep for.

Your own [custom Lua checks](#your-own-checks-in-lua) use the same shape, but
with an authority that's *yours* - anything except `rototo`, which is reserved:

```text
consumer-experience/checkout-heading-required
payments/max-token-budget
```

Both the authority and the rule name have to be lowercase letters, digits, and
hyphens. That keeps every diagnostic in the system addressable the same way, no
matter who wrote the rule.

One distinction worth being explicit about: rule names are hyphenated, but the
ids *inside* your package (variables, lists, catalogs, entries, evaluation
contexts, samples) are snake_case. Those are two separate namespaces. Package
ids appear in expressions, where a hyphen is the minus operator; rule names
never do, so they keep the kebab convention.

## Seeing the whole catalog

You don't have to memorize the rules - you can ask for the catalog:

```sh
# every built-in rule rototo ships
rototo show --lint-rules

# from inside a package: built-in rules *plus* your custom ones
rototo show --lint-rules

# machine-readable, for tooling
rototo show --lint-rules --json
```

Run it with no package and you get the global built-in catalog. Run it from
inside a package and rototo adds that package's custom rules to the output, so you
see exactly what *this* package will be checked against. The human view is a
table of `rule | entity | severity | title`; the JSON view carries the same
fields plus each rule's `help`.

That command is the source of truth - it can't drift the way a hand-written page
in these docs would.

## How the built-in checks are grouped

There are a few dozen built-in rules, and they line up with the parts of a
package. You don't need them all in your head; this is the map so you know
roughly where a finding is coming from.

- **Package** - the manifest exists, parses, declares `schema_version = 1`, any
  `[[trace]]` policies have a valid `when`, and every rototo-recognized id is
  snake_case. (e.g. `rototo/package-manifest-missing`,
  `rototo/trace-when-invalid-reference`, `rototo/id-not-snake-case`)
- **Governance** - `governance.toml` parses, its blocks are keyed
  `[<kind>.<id>]` with known kinds, operations, and policy keys, allowlists
  aren't empty, delete policies carry no field scope, field names exist in the
  catalog schema, every block names an entity the package declares, and update
  grants scope their fields (a warning when they don't - an unscoped grant
  silently includes fields added later). (e.g.
  `rototo/governance-parse-failed`, `rototo/governance-shape`,
  `rototo/governance-unknown-target`, `rototo/governance-unscoped-update`)
- **Evaluation contexts** - context schemas are valid JSON Schema, don't use
  reserved fields, only use list targets in `x-rototo-ref`, and every sample
  matches its schema, including any list member pins. (e.g.
  `rototo/evaluation-context-sample-schema-mismatch`)
- **Variables** - they parse, declare a `type` and a `[resolve]` default, their
  values match the declared type, catalog-backed variables point at real catalog
  entries, list-backed variables use declared lists and stay inside the member
  set, and rule `when` expressions reference real variables and declared
  context paths with the right types. This group also flags variables whose
  references form a cycle. Allocation-backed variables (`method =
  "allocation"`) name a real allocation and cover its arms exactly. (e.g.
  `rototo/variable-value-type-mismatch`,
  `rototo/variable-rule-unknown-variable`, `rototo/variable-unknown-list`,
  `rototo/variable-reference-cycle`, `rototo/variable-unknown-allocation`,
  `rototo/variable-allocation-shape`)
- **Variable rules** - warnings about rules that can never fire because an
  earlier rule shadows them, or rules that just re-select the default anyway.
  (e.g. `rototo/variable-rule-shadowed`)
- **Layers** - each layer file under `layers/` parses, declares
  `schema_version = 1`, has a valid `unit` and `buckets`, and its allocations
  are well-shaped, with arms across the whole layer claiming disjoint buckets.
  (e.g. `rototo/layer-parse-failed`, `rototo/layer-schema-version`,
  `rototo/layer-shape`, `rototo/layer-bucket-overlap`)
- **Catalogs** - schemas are valid JSON Schema, and any UI widget hints make
  sense for the property they're on. (e.g. `rototo/catalog-schema-invalid`)
- **Catalog entries** - each entry parses, validates against its catalog's
  schema, and any `x-rototo-ref` values point at real catalog entries or list
  members. (e.g. `rototo/catalog-entry-schema-mismatch`,
  `rototo/catalog-entry-unknown-reference`)
- **Lists** - each file under `lists/` parses, declares
  `schema_version = 1`, a scalar `type`, and a non-empty `members` array of
  distinct values matching that type. (e.g. `rototo/list-parse-failed`,
  `rototo/list-schema-version`, `rototo/list-shape`)
- **Custom lint** - your Lua files register cleanly, without conflicting or
  duplicate rule metadata. (e.g. `rototo/custom-lint-registration-invalid`)

A good rule of thumb: if the rule name starts with the thing you just edited, the
finding is about that thing.

One historical note: earlier rototo versions had a separate qualifier entity
with its own `rototo/qualifier-*` rules. Qualifiers were dissolved into
condition variables (plain bool variables), so those rule ids are gone: they
no longer fire and they don't appear in the catalog.

## Errors versus warnings

There are only two severities, and the line between them is simple.

An **error** means the package can't be trusted to run - a value doesn't match
its type, a rule points at a variable that doesn't exist, an entry breaks its
schema. `Package::load` in the SDK rejects a package with lint errors, so these
genuinely block a release.

A **warning** is something you probably want to know but that won't break
anything: a rule that can never fire, a rule that just re-selects the default,
a custom lint file that registered no rules. Warnings are how lint nudges you
toward a cleaner package without standing in your way.

## Your own checks, in Lua

Some rules are specific to your world and rototo can't guess them - "standard-tier
users can't get more than five projects," "a checkout heading can't be empty."
Those live in `lint/*.lua`, and they produce diagnostics that sit right next to
the built-in ones.

A lint file defines a `register` function and registers one or more rules inside
it:

```lua
function register(lint)
  lint:rule({
    id = "consumer-experience/checkout-heading-required",
    title = "Checkout heading is missing",
    help = "Set heading to visible checkout copy.",
    target = "catalog=checkout_redesign:entry=",
    severity = "error",
    handler = "check_heading",
  })
end

function check_heading(package, entry)
  if is_checkout_value(entry.value) and entry.value.heading == "" then
    return {{ message = "checkout value " .. entry.key .. " must include heading", path = "/value/heading" }}
  end
  return {}
end
```

What each field in `lint:rule({...})` does:

- `id` - the rule name, `authority/rule-id` (your authority, not `rototo`). Required.
- `title` - a short summary. Required.
- `help` - how to fix it. Required.
- `handler` - the name of the function rototo calls. Required.
- `target` - what the rule runs against (defaults to `/`, the whole package).
- `severity` - `error` or `warning` (defaults to `error`).

The handler gets the `package` and the current `target`, and returns an array of
problems. Each problem needs a `message`, and can optionally point at the exact
spot with a `path` or `field`. Returning an empty array (or `nil`) means "nothing
wrong here."

A couple of guardrails worth knowing, since they shape what your Lua can do: the
handlers run in a locked-down sandbox - just the `table`, `string`, `utf8`, and
`math` libraries, no file or network access. The limits are concrete: 16 MB of
Lua memory, one million VM instructions per handler run, and a two-second
timeout, and hitting any of them fails that rule loudly instead of hanging
lint. Custom lint is for *inspecting* the package, not reaching outside it.

## The JSON shape

`rototo lint --json` emits each diagnostic as an object. Here's the full shape,
with the fields you'll actually use up top:

```json
{
  "rule": "rototo/variable-value-type-mismatch",
  "severity": "error",
  "message": "value 3 does not match declared type bool",
  "help": "Use a value matching the variable's declared type.",
  "location": {
    "path": "variables/checkout_redesign.toml",
    "range": {
      "start": { "line": 8, "character": 8 },
      "end": { "line": 8, "character": 9 }
    }
  },
  "stage": "value",
  "target": {
    "entity": { "kind": "variable", "id": "checkout_redesign" },
    "field": { "kind": "resolveDefault" }
  },
  "related": [
    { "location": { "path": "...", "range": { } }, "message": "declared here" }
  ]
}
```

The fields:

- **`rule`**, **`severity`**, **`message`**, **`help`** - the same four you see
  in the terminal.
- **`location`** - `path` is the file, relative to the package root, and is
  always there. `range` pins the exact span (zero-indexed `line` and
  `character`), and is present when the diagnostic can point at one.
- **`stage`** - which phase of lint produced it (`discover`, `parse`, `project`,
  `register`, `reference`, `value`, `graph`, or `policy`). Useful for grouping;
  ignorable otherwise.
- **`target`** - what the finding is about: an `entity` (always, tagged by
  `kind`) and an optional `field` within it.
- **`related`** - other locations that help explain the problem, like where a
  conflicting value was first declared.

Severity and stage both serialize lowercase. The `rule` string is identical
whether it's built-in or custom, so one piece of tooling can read findings from
both.
