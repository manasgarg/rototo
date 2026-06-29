# Diagnostics

When you run `rototo lint`, what comes back is a list of **diagnostics** - each
one a specific thing that's wrong, or worth a second look, in your package. This
page explains how to read a diagnostic, how the built-in checks are organized,
and how your own custom checks fit in alongside them.

The thing to hold onto: lint isn't a smoke test that just says "looks fine." It
actually understands rototo's model - that a variable's type matches its values,
that a qualifier references a real qualifier, that a catalog entry fits its
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
rototo/qualifier-when-missing
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

## Seeing the whole catalog

You don't have to memorize the rules - you can ask for the list:

```sh
# every built-in rule rototo ships
rototo show --lint-rules

# from inside a package: built-in rules *plus* your custom ones
rototo show --lint-rules

# machine-readable, for tooling
rototo show --lint-rules --json
```

Run it with no package and you get the global built-in catalog. Run it from
inside a package and rototo adds that package's custom rules to the list, so you
see exactly what *this* package will be checked against. The human view is a
table of `rule | entity | severity | title`; the JSON view carries the same
fields plus each rule's `help`.

That command is the source of truth - it can't drift the way a hand-written list
in these docs would.

## How the built-in checks are grouped

There are a few dozen built-in rules, and they line up with the parts of a
package. You don't need them all in your head; this is the map so you know
roughly where a finding is coming from.

- **Package** - the manifest exists, parses, declares `schema_version = 1`, and
  any `[[trace]]` policies have a valid `when`. (e.g.
  `rototo/package-manifest-missing`, `rototo/trace-when-invalid-reference`)
- **Evaluation contexts** - context schemas are valid JSON Schema, don't use
  reserved fields, and every sample matches its schema. (e.g.
  `rototo/evaluation-context-sample-schema-mismatch`)
- **Qualifiers** - they parse, declare a version, have a `when`, and that `when`
  only references real qualifiers and declared context paths with the right
  types. This group also flags qualifiers that form a cycle, and warns about ones
  nothing uses. (e.g. `rototo/qualifier-when-unknown-qualifier`,
  `rototo/qualifier-cycle`, `rototo/qualifier-unreferenced`)
- **Variables** - they parse, declare a `type` and a `[resolve]` default, their
  values match the declared type, catalog-backed variables point at real catalog
  entries, and rule `when` expressions reference real qualifiers and context
  paths. (e.g. `rototo/variable-value-type-mismatch`,
  `rototo/variable-unknown-value`, `rototo/variable-resolve-missing-default`)
- **Variable rules** - warnings about rules that can never fire because an
  earlier rule shadows them, or rules that just re-select the default anyway.
  (e.g. `rototo/variable-rule-shadowed`)
- **Catalogs** - schemas are valid JSON Schema, and any UI widget hints make
  sense for the property they're on. (e.g. `rototo/catalog-schema-invalid`)
- **Catalog entries** - each entry parses and validates against its catalog's
  schema. (e.g. `rototo/catalog-entry-schema-mismatch`)
- **Custom lint** - your Lua files register cleanly, without conflicting or
  duplicate rule metadata. (e.g. `rototo/custom-lint-registration-invalid`)

A good rule of thumb: if the rule name starts with the thing you just edited, the
finding is about that thing.

## Errors versus warnings

There are only two severities, and the line between them is simple.

An **error** means the package can't be trusted to run - a value doesn't match
its type, a qualifier points at one that doesn't exist, an entry breaks its
schema. `Package::load` in the SDK rejects a package with lint errors, so these
genuinely block a release.

A **warning** is something you probably want to know but that won't break
anything: a qualifier nothing references, a rule that can never fire, a custom
lint file that registered no rules. Warnings are how lint nudges you toward a
cleaner package without standing in your way.

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
    target = "/catalogs/checkout-redesign/entries",
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

The handler gets the `package` and the current `target`, and returns a list of
problems. Each problem needs a `message`, and can optionally point at the exact
spot with a `path` or `field`. Returning an empty list (or `nil`) means "nothing
wrong here."

A couple of guardrails worth knowing, since they shape what your Lua can do: the
handlers run in a locked-down sandbox - just the `table`, `string`, `utf8`, and
`math` libraries, no file or network access - with limits on memory, work, and a
two-second timeout per handler. Custom lint is for *inspecting* the package, not
reaching outside it.

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
    "path": "variables/checkout-redesign.toml",
    "range": {
      "start": { "line": 8, "character": 8 },
      "end": { "line": 8, "character": 9 }
    }
  },
  "stage": "value",
  "target": {
    "entity": { "kind": "variable", "id": "checkout-redesign" },
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
