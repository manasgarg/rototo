# Package Format

A rototo package is just a folder of files. There's no database, no hidden
state, no generated blob you have to keep in sync - what you see in the folder is
the whole thing. That's on purpose: config you can read, diff, and review in a
pull request is config you can trust.

This page is the tour of that folder. We'll go file by file, and for each one
I'll show you a real example pulled straight from the `examples/basic` package
that ships with rototo.

## The shape of the folder

Here's the whole layout. Don't worry about memorizing it - `rototo init` makes
all of this for you. This is just so you know what goes where:

```text
my-package/
├── rototo-package.toml              # the manifest - marks this folder as a package
├── variables/                       # the values your app reads
│   ├── premium_users.toml
│   └── checkout_redesign.toml
├── layers/                          # shared bucket lines for rollouts and experiments
│   └── checkout.toml
├── model/                           # contracts: what values must look like
│   ├── catalogs/
│   │   └── checkout_redesign.schema.json
│   ├── enums/
│   │   └── plan_tiers.toml
│   └── context/                     # the shape of the facts your app passes in
│       ├── request.schema.json
│       └── request-samples/
│           └── premium_enterprise.json
├── data/                            # values that satisfy those contracts
│   ├── catalogs/
│   │   └── checkout_redesign/
│   │       └── control.toml
│   └── enums/
│       └── plan_tiers.toml
└── lint/                            # your own custom checks, in Lua
    └── checkout_redesign.lua
```

Notice the split between `model/` and `data/`. `model/` holds contracts: catalog
schemas, enum declarations, evaluation-context schemas. `data/` holds the values
that have to satisfy those contracts: catalog entries and enum members. That
separation matters in review: a change under `model/` changes what's *allowed*,
a change under `data/` changes what's *there*. Variables and lint sit at the top
level because each is its own thing - a variable is both contract and value in
one file, and lint is code.

Two rules tie the folder together.

First, **the file name is the id**. A file at `variables/checkout_redesign.toml`
defines a variable whose id is `checkout_redesign`. A catalog schema at
`model/catalogs/checkout_redesign.schema.json` defines a catalog whose id is
`checkout_redesign`. You never write the id *inside* the file - the filename
already said it. Variable files can also live in subdirectories, and the
subdirectory becomes a namespace: `variables/acme/in_trial.toml` defines the
variable `acme/in_trial`, referenced as `variables["acme/in_trial"]` in
expressions.

Second, **ids are snake_case**. Every id rototo recognizes - variables, enums,
catalogs, catalog entries, evaluation contexts, samples - is lowercase letters,
digits, and underscores, with `/` allowed for namespacing (like
`payments/retry_limit`). The reason is that ids show up in TOML table headers
and in [expressions](./expressions.md), where a hyphen is the minus operator:
`variables.premium-users` would parse as a subtraction. Snake_case keeps
`variables.premium_users` working everywhere. Lint enforces this as an error,
`rototo/id-not-snake-case`. (Diagnostic rule names like
`rototo/id-not-snake-case` itself are a separate namespace and stay
hyphenated.)

## The manifest: `rototo-package.toml`

This is the file that says "this folder is a rototo package." It lives at the
root, and it's deliberately tiny. The only thing it *must* have is a version
marker:

```toml
schema_version = 1
```

That's a complete, valid manifest. The `schema_version` has to be exactly `1` -
it's how rototo knows it's reading a format it understands.

There are two optional things you can add.

The first is `extends`, for when this package builds on top of others - shared
defaults, a common set of condition variables, that kind of thing. You list the
parent packages as [package sources](./package-sources.md):

```toml
schema_version = 1
extends = ["../shared-config", "git+https://github.com/acme/base-config.git#main"]
```

Each entry follows the exact same source grammar you'd type on the command line.
Relative paths are resolved against this package, so a package and its parents
can travel together. (When you build a distributable archive with `rototo
package`, the `extends` list gets flattened in and stripped out - the archive is
already self-contained, so there's nothing left to point at.) How the layers
actually combine - which files replace and which compose - gets its own
section, [Extending: how packages combine](#extending-how-packages-combine),
right after this one.

The second is `[[trace]]`, which turns on resolution tracing for specific cases
without redeploying your app. You can have as many as you like:

```toml
[[trace]]
when = 'env.resolving.variable == "checkout_redesign" && context.user.id == "user-123"'
```

The `when` is an [expression](./expressions.md) - same language as everywhere
else. We cover what tracing is for in [Using Rototo](./adoption.md); here, just
know it's a manifest thing.

## Extending: how packages combine

When a package extends others, rototo flattens them into one file tree before
anything else happens - lint, resolution, `rototo package`, all of it sees the
flattened result. Parents flatten in `extends` order, the child comes last.
Flattening is deterministic: the packages are processed in a sorted, fixed order,
so the same inputs always produce the same flattened package.

One premise before the mechanics: modifying a base is permission-gated.
Adding new ids is always free, but every update or delete of something a
base declared needs that base to grant it in `governance.toml` - deny by
default is unconditional, and a base without the file grants nothing. The
composition rules below describe what the operations *do*; the
[governance section](#governance-governancetoml) describes who may perform
them. A base whose overlays are its own team typically opens itself with one
broad `[defaults]` grant.

The composition rules below describe the child landing on its bases - the
overlay relationship, where one package was authored to change another. Two
bases in the same `extends` list are a different relationship: siblings.
Neither was authored as an overlay of the other, so nothing about them may
silently merge. Each base is flattened on its own first, then the results
union: the composed package has every variable, catalog, enum, evaluation
context, and layer from every base, and the bases have to be disjoint. Two
bases touching the same entity - the same variable id, the same enum, context,
or layer - fails the load ("package extends bases conflict on ..."). Catalogs
are the one place siblings may share: two bases can each add their own entries
to a catalog they both inherit from a common ancestor, because entry files
compose additively. The disjointness line just moves down a level - two bases
providing the same entry, one base updating or deleting an entry another base
provides, or two bases carrying different versions of the catalog schema, all
still fail the load. If two bases genuinely share more than that, make one
extend the other or move the shared piece into one package. Diamond ancestry
stays fine everywhere: two bases extending a common ancestor both carry its
files, and byte-identical restatements of the same file never conflict.

Governance stays per-base: each base's `governance.toml` governs its own
entities against the overlays that land after it, so the extending package
needs each base's grants for what it changes in that base, and a base with no
`governance.toml` stays ungoverned as usual.

The default rule is the simple one: **a file replaces the file at the same path
in the base packages, whole.** That's still what happens for `model/` schemas
(catalog, enum, and context), `layers/`, and `lint/` files. But
four file shapes compose structurally instead, because whole-file replacement
can't say the things an overlay needs to say: it can't disable one base catalog
entry, can't change one field of an entry, can't add one enum member, and it
forces you to copy an entire variable file just to change its resolution.

One bookkeeping note: while flattening, rototo records which package owns each
variable's `[resolve]` block in a small provenance sidecar, and resolution
traces read it back as the trace's `provenance` field. You never edit that
file; it's how `rototo resolve` can print `resolve from <source>` for a
composed package.

### Variables update through a marker

An overlay never restates a base variable's file. It updates one with a
marker, the same shape catalog entries use:

- `variables/<id>.update.toml` updates the base variable. The marker may
  carry only `[resolve]` and `description` - the fields an overlay is allowed
  to change. Each key it declares replaces the base's key whole; there is no
  key-level merge *inside* `[resolve]`, the topmost package's resolve block
  wins whole. The marker itself never lands in the flattened package.
- A plain `variables/<id>.toml` is always an add. If the base already
  declares the id, the load fails and points at the marker: "variable `<id>`
  is declared in the base packages; update it with
  `variables/<id>.update.toml` instead of restating the file". That keeps a
  reviewer able to tell an add from an update by the file name alone.

The marker may not carry `type` or `schema_version`, even restated with the
base's exact values. The type is the contract applications were written
against; repeating it in an overlay would force every reviewer to check it
still agrees, and would silently pin the base's intent. An update file
carries only what it changes.

There is no `variables/<id>.deleted.toml`. An overlay never removes a base
variable - every consumer resolving the id would break - it can only give
the id different behavior. Removing a variable is the base's decision.

An orphan marker (no base variable to update) fails the load, and one package
providing both `<id>.toml` and `<id>.update.toml` is contradicting itself and
fails too. The one exception to the no-restatement rule is a byte-identical
copy of the base's file, which composes as a no-op - that is how diamond
ancestry looks when two bases share an ancestor.

An overlay can also add variables of its own, and subdirectories keep them out
of the base's way: `variables/acme/in_trial.toml` defines the namespaced
variable `acme/in_trial`, referenced as `variables["acme/in_trial"]`.

### Catalog entries: union, deletes, and updates

Catalog entries compose as a set. The active entries of a catalog are the
entries the base packages provide, plus the entries this package provides, minus
the entries this package deletes, with field updates applied. Two file shapes
drive the minus and the update, both keyed by path next to the entries they act
on:

- `data/catalogs/<catalog>/<entry>.deleted.toml` removes the entry a base
  package provided from this package's view. The base file is untouched; the
  marker file itself never appears in the flattened package. By convention it
  contains `deleted = true` and an optional `reason = "..."`.
- `data/catalogs/<catalog>/<entry>.update.toml` updates fields of the entry
  below: tables merge recursively, scalars and arrays replace, and any field
  the update doesn't mention is inherited.

Both shapes have to point at something real. A deleted marker with no entry in
the base packages fails the load ("deleted marker has no catalog entry to
remove in the base packages"), and an orphaned update marker fails the same way. A
single package that both provides `<entry>.toml` and deletes or updates that
same entry also fails the load - it's contradicting itself.

Deleting an entry someone depends on is deliberately loud. If a base variable
still names the deleted entry, lint catches it as
`rototo/variable-unknown-value`, and the overlay has to override that
variable's resolution too. Removing data quietly out from under a variable is
exactly the drift this is designed to surface.

### Enum members: union and delete

An overlay's `data/enums/<id>.toml` doesn't replace the base's member file - the
member sets compose. `members` declares what the package adds, and `deleted`
names the base members it removes:

```toml
# overlay's data/enums/plan_tiers.toml
members = ["acme_enterprise"]
deleted = ["legacy_bronze"]
```

The composed enum has the base's members plus `acme_enterprise`, minus
`legacy_bronze`. The `deleted` key is consumed during flattening and never
appears in the flattened package.

Deletes follow the same rules as catalog entry deletes. Every deleted value
has to name a member some base package actually provides ("deleted enum member
is not in the base packages" fails the load), a single package may not both add
and delete the same value, and deleting every member fails the load - an
empty enum is not a thing. In a package with no `extends`, a `deleted` key has
nothing to remove from and lint flags it (`rototo/enum-members-shape`).

Deleting a member someone depends on is loud in the same way entry deletes
are: if a base variable default, rule value, catalog entry, or context sample
still uses the deleted member, lint on the flattened package catches it, and
the overlay has to override that usage too.

## Governance: `governance.toml`

`extends` composition lets an overlay change things. For one team splitting a
package across files, that's the point. For a tenant overlay, it's exactly
wrong: the app ships a contract, and a tenant should only move *within* it.
Governance is the file that makes layering safe - a dial on every capability
that each successive overlay can only turn further down.

The file is a single `governance.toml` at the package root. It holds one block
per governed entity, keyed `[<kind>.<id>]`, where `kind` is one of `catalog`,
`enum`, `variable`, `evaluation_context`, or `layer`:

```toml
[catalog.plans]
allowed_operations = ["add", "update", "delete"]

[catalog.plans.update_policy]
allowed_fields = ["monthly_price", "limits"]
denied_entries = ["free"]

[catalog.plans.delete_policy]
allowed_entries = ["*"]
denied_entries = ["free"]

[variable.active_plan]
allowed_operations = ["update"]
```

Read that as a contract: the overlay may add plan entries, may update
`monthly_price` and `limits` on any plan except `free`, may delete any plan
except `free`, and may update `active_plan`'s resolution. Everything else it
might try on a base-declared entity is denied.

### Deny by default, unconditionally

Governance denies by default whether or not the file exists. A base with no
`governance.toml` grants nothing: an overlay can add new ids next to it, but
any update or delete of something the base declared fails the load with
`governance denies <op> on <kind>.<id>`. The file is not a switch that turns
governance on - it is simply where the grants live, and no file means no
grants.

For a base whose overlays are its own team, spelling out every grant would be
noise. That is what the top-level `[defaults]` block is for:

```toml
[defaults]
allowed_operations = ["add", "update", "delete"]
```

`[defaults]` grants across every base-declared entity that a per-entity block
doesn't speak for itself. Per-entity blocks refine below it, and deny wins
from either level: a `[defaults]` grant of `delete` plus a
`[catalog.plans] denied_operations = ["delete"]` means everything but plans
entries can be deleted. `[defaults]` can carry `update_policy` and
`delete_policy` tables too, which apply where the entity's own block has
none.

The contract governs what the base declared, nothing more. New ids mint
freely: a tenant's own namespaced variables, its own catalogs and enums, its
own layers. Whether those minted ids are *well named* is a lint concern
(`rototo/id-not-snake-case` and friends), not a permission.

### The three operations

Each operation names one on-disk shape the overlay can produce:

| Operation | What the overlay does on disk |
| --- | --- |
| `add` | a new `<entry>.toml` in a governed catalog, or a member file under `data/enums/` for a declared enum that had none in the base |
| `update` | an `<entry>.update.toml` over a base catalog entry, a `data/enums/<id>.toml` that unions members into the base's set, a `variables/<id>.update.toml` over a base variable, or a replacement of a base layer file under `layers/` |
| `delete` | an `<entry>.deleted.toml` disabling a base catalog entry |

Grants go in `allowed_operations`; `denied_operations` subtracts from them and
wins. The retired operation names `constrain` and `override` are not accepted
and will not be reused. An operation absent from `allowed_operations` is denied - that's the
default-closed part.

Some shapes are deliberately *not* operations. Replacing a whole base catalog
entry file is rejected toward the structural shapes: "governance does not
model replacing catalog entry `<entry>` wholesale; use `<entry>.update.toml` to
update fields or `<entry>.deleted.toml` to disable it". Replacing a lint file
the base owns is rejected outright.

Base schema files are in that group too, and it's worth saying why. Under a
governed base, an overlay can never change what the base declared under
`model/`: not a catalog schema, not an enum declaration, not an evaluation
context schema or its samples. There is no grant for it. The reason is that a
schema edit can widen a contract just as easily as narrow it, and no one can
tell the difference by looking at the grant. When an overlay genuinely needs a
tighter contract than its base ships, it writes a custom lint rule under
`lint/`: the base's schema stays the shared floor, and the overlay's lint rule
is the overlay's own, reviewable, tightening on top of it.

### Scoping update and delete

Only `update` and `delete` carry a scope, through the optional
`update_policy` and `delete_policy` tables. Each takes up to four lists:

- `allowed_entries` / `denied_entries` - which entry ids the operation may
  touch.
- `allowed_fields` / `denied_fields` - which top-level fields an update may
  change. Field lists on `delete_policy` are a lint error; a delete has no
  field scope.

List items are literal ids or `*` globs (`*` matches any run of characters,
everything else is literal - `acme_*`, `*_hero`). The resolution rules:

- An **allowlist restricts when present.** No `allowed_entries` means every
  entry passes the allow side. An *empty* allowlist is a lint error - "listed
  nothing" reads two ways, so name targets or drop the list.
- A **denylist subtracts and wins absolutely.** An id matching a denied
  pattern fails, whatever the allowlist says.
- An `update` must pass both dimensions: the field passes the field lists
  *and* the entry passes the entry lists.

Field names are a fixed set, so a field pattern that matches nothing the
catalog schema declares is a lint error. Entry lists are not checked that way,
because they may name entries an overlay adds later.

### The ceiling: grants only narrow

Governance stacks. An overlay's own `governance.toml` is its grant to *its*
overlays, and it must fit inside the ceiling it inherited: every operation
it allows, and every policy pattern it lists, has to be something its base
granted it. A wider grant is rejected at compose time, not silently
clamped - `governance grant exceeds the inherited ceiling: ...` - so the
author sees it and either drops the rule or asks the base to widen.

That's the dial model made concrete: each successive overlay can keep the
dial where it is or turn it further down, never back up.

### Lint on the file itself

Four rules cover the contract file:

- `rototo/governance-parse-failed` - the TOML doesn't parse.
- `rototo/governance-shape` - structural problems: an unknown kind or key, an
  operation name outside the four, a dead `update_policy`/`delete_policy` for
  an operation the block doesn't allow, an empty allowlist, field scopes on
  `delete_policy`, or a field name the catalog schema doesn't declare.
- `rototo/governance-unknown-target` - a `[<kind>.<id>]` block names an entity
  the package doesn't declare.
- `rototo/governance-unscoped-update` - a warning: an `update` grant without
  `allowed_fields` silently includes every field someone adds to the schema
  later. List the fields the overlay may change.

## Variables: the values your app actually reads

A **variable** is the thing your application asks for at runtime. It has a type,
a default, and an optional list of rules that override the default when some
condition holds.

The simplest kind is a plain on/off flag. Here's `user_is_admin.toml`:

```toml
schema_version = 1
description = "Whether the user should receive admin UI affordances"
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'variables["admin_users"]'
value = true
```

Read it top to bottom and it tells a story: this is a boolean; by default it's
`false`; but for admin users, it's `true`. When your app resolves this variable,
rototo checks the rules in order, takes the `value` of the first rule whose
`when` matches, and falls back to `default` if none do.

The fields:

- `schema_version` - always `1`.
- `description` - optional, recommended.
- `type` - required. What kind of value this is (next section).
- `[resolve]` - required. Holds an optional `method`, the `default`, and the
  rules.
- `method` - optional, `"rules"`, `"query"`, or `"allocation"`. Absent means
  `"rules"`: the first matching rule's value wins, else the default. `"query"`
  swaps the rules for a catalog query; the fields for that live in
  [Catalog queries](#catalog-queries-picking-entries-from-data) below.
  `"allocation"` swaps them for an arm assignment from a layer; see
  [Layers](#layers-shared-assignment-for-rollouts-and-experiments) below.
- `default` - required under the rules method.
- `[[resolve.rule]]` - zero or more. Each has a `when` condition and a `value`.

Both the default and every rule value have to match the declared `type` - rototo
checks that for you, so a `bool` variable can't accidentally default to a string.

Some old syntax is gone: a top-level `schema` field and a `[values]` section are
both rejected. Declare a `type` and put your literal values directly under
`[resolve]`.

## Condition variables: naming a runtime condition

That `variables["admin_users"]` in the rule above deserves a closer look. "Is
this a premium user?" "Is this request coming from Europe?" Conditions like
these tend to show up in more than one variable, and repeating the same
expression everywhere is how definitions drift apart.

The fix is to give the condition a name - and in rototo, a named condition is
just a bool variable. By convention we call it a **condition variable**: type
`bool`, default `false`, and a rule that flips it to `true` when the condition
holds. Here's `eu_users.toml`:

```toml
schema_version = 1
description = "Users whose country is in the European operating region"
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = '(context.request.country in ["DE","FR","ES","IT","NL","SE"])'
value = true
```

Any other variable's rule can now read that condition by name, with the
`variables["<id>"]` root. Conditions can lean on each other, too. This one is
true only when two *other* condition variables are both true:

```toml
schema_version = 1
description = "Premium users who are also in the beta rollout bucket"
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = '(variables["premium_users"]) && (variables["beta_rollout_bucket"])'
value = true
```

There's nothing special about a condition variable to rototo - it resolves like
any other bool, and your app can resolve it directly if it wants the yes/no
answer itself. The convention is for readers: a bool named `eu_users` with a
`false` default reads as "the condition this package uses to mean an EU user."
How `variables[...]` references work (lazy, memoized, cycles rejected) is
covered in the [expressions reference](./expressions.md).

(Older packages had a separate `qualifiers/` folder for this. Qualifiers were
dissolved into condition variables; a `qualifiers/` directory is no longer part
of the format.)

## Variable types

The `type` field decides what shape a value can take. The built-in types are:

| Type | What it is |
| --- | --- |
| `bool` | true / false |
| `int` | a whole number |
| `number` | a number with a fractional part |
| `string` | text |
| `list` | a plain list of values |
| `catalog:<id>` | one entry from a catalog (see below) |
| `enum:<id>` | one member of a named enum (see below) |
| `list<...>` | a list of a specific item type |

The `list<...>` form lets you say what's *in* the list. The item can be a
primitive, a catalog reference, or an enum - `list<string>`, `list<int>`,
`list<catalog:payment_methods>`, `list<enum:plan_tiers>`. What you can't do is
nest lists inside lists: `list<list<string>>` is rejected. One level deep is
the limit.

Here's a plain list variable, `payment_methods.toml`:

```toml
schema_version = 1
description = "Payment methods enabled at checkout"
type = "list"

[resolve]
default = ["card", "paypal"]

[[resolve.rule]]
when = 'variables["mobile_users"]'
value = ["card", "apple_pay", "google_pay"]
```

## Catalogs: values with a real shape

Sometimes a value isn't a single number or string - it's a structured object
with several fields, and you've got a few named versions of it. A checkout page
layout, say: each variant has a heading, a subheading, an image, some body copy.
That's what a **catalog** is for. It's a set of named entries, all sharing one
schema.

A catalog comes in two parts, and they live on the two sides of the
model/data split. First, the schema, at `model/catalogs/<id>.schema.json` - an
ordinary JSON Schema describing what every entry must look like. Here's
`checkout_redesign.schema.json`:

```json
{
  "description": "Checkout page content and layout entries",
  "type": "object",
  "required": ["variant", "heading", "subheading", "image_url", "content"],
  "properties": {
    "variant": { "type": "string" },
    "heading": { "type": "string" },
    "subheading": { "type": "string" },
    "image_url": { "type": "string" },
    "content": { "type": "string" }
  },
  "additionalProperties": false
}
```

Second, the entries, each a TOML file under `data/catalogs/<id>/`. The
filename is the entry's id. Here's `data/catalogs/checkout_redesign/control.toml`:

```toml
variant = "control"
heading = "Complete your purchase"
subheading = "You're almost done"
image_url = "/images/checkout/control.png"
content = "Secure checkout in seconds."
```

rototo converts that TOML to JSON and checks it against the schema, so an entry
that's missing a field or has a typo gets caught at lint time, not in production.

To use a catalog, give a variable the type `catalog:<id>` and let its values be
entry ids:

```toml
schema_version = 1
description = "Checkout page content and layout variant"
type = "catalog:checkout_redesign"

[resolve]
default = "control"

[[resolve.rule]]
when = 'variables["premium_users"]'
value = "premium"
```

The variable resolves to an entry id like `"control"`, and rototo hands your app
the full structured entry behind it.

## Catalog queries: picking entries from data

Sometimes which entry applies is a data question, not a rule question. "Every
enabled payment method." "The pricing plan whose tier matches the account."
Writing one `when`/`value` rule per entry just duplicates what the entries
already say, and it goes stale the moment someone adds an entry. Setting
`method = "query"` turns the resolve block into a small pipeline over one
catalog's entries instead:

```toml
schema_version = 1
type = "list<catalog:llm_parameters>"

[resolve]
method = "query"
from = "llm_parameters"
filter = "entry.enabled == true"
```

The query keys sit flat on `[resolve]`:

- `from` - required, a string. The id of the catalog to read entries from. It
  must be the same catalog named in the variable's `type`, and it must exist
  (`rototo/variable-unknown-catalog` if it doesn't).
- `filter` - optional, a boolean [expression](./expressions.md) run once per
  entry. `entry` is the entry under consideration, and `context`,
  `variables[...]`, and `env.now` are available exactly as in a rule's `when`.
  Entries where it comes out true stay; with no `filter`, every entry stays.
- `sort` - optional, an expression evaluated once per entry that produces that
  entry's sort key. The keys have to be mutually comparable - all numbers or
  all strings. Mixed kinds are a resolution error.
- `order` - optional, `"asc"` (the default) or `"desc"`. Requires `sort`.
- `limit` - optional, a positive integer. After sorting, keeps at most that
  many entries.
- `default` - optional. The usual resolve default, used when the query matches
  nothing.

What the query produces depends on the variable's type:

- `type = "list<catalog:<id>>"` - the value is every matching entry, after
  `sort` and `limit`. No matches means the `default` if you declared one,
  otherwise an empty list.
- `type = "catalog:<id>"` - the value is one entry. With a `sort`, the top
  entry wins. Without one, the `filter` has to match exactly one entry;
  matching several is a resolution error (add a `sort` or narrow the filter).
  No matches means the `default` if you declared one, otherwise an error.

Methods don't mix. `method = "query"` must not declare
`[[resolve.rule]]` tables, and the query keys are rejected under the rules
method. Lint enforces all of this shape as `rototo/variable-query-shape`.

## Layers: shared assignment for rollouts and experiments

Rolling a change out to 20% of users, or A/B testing two versions of the
checkout copy, needs a deterministic answer to "which variant does this user
get?" - the same answer on every request, and the same answer for every
variable the experiment drives. If the layout, the copy, and the CTA each
hashed the user independently, one user could see the new layout with the old
copy. That's why the assignment is a shared, named thing in the package - an
**allocation** inside a **layer** - and not a per-variable setting.

The mental model: a layer hashes each unit (say, `context.user.id`) to a
stable position on a line of buckets, and that position never moves. An
allocation claims a set of those buckets and divides them among **arms**.
Allocations in one layer claim disjoint buckets, so a unit sits in at most one
of them. Different layers are independent lines - it's safe for the same user
to be in an experiment in two different layers, because each layer's
allocations drive different variables. A rollout and an experiment are the
same shape used differently: a rollout is one arm growing its bucket range, an
experiment is two or more arms splitting one.

### The layer file

Each layer is one TOML file under `layers/`, and as usual the file stem is the
layer's id (snake_case). Here's `layers/checkout.toml` from `examples/basic`:

```toml
schema_version = 1
description = "Checkout page experiments, diverted by user id"
unit = "context.user.id"
buckets = 1000

[[allocation]]
id = "cta_copy_test"
status = "running"
eligibility = '!variables["enterprise_accounts"]'

[[allocation.arm]]
name = "control"
buckets = "0-499"

[[allocation.arm]]
name = "benefit_led"
buckets = "500-999"
```

The layer-level fields:

- `schema_version` - always `1` (`rototo/layer-schema-version` if it isn't).
- `description` - optional, recommended.
- `unit` - required. A CEL [expression](./expressions.md) over `context` that
  produces the value to hash - usually a stable id like the user id. It reads
  `context` only: no `variables`, no `entry`.
- `buckets` - required. A positive integer, the length of the line. Buckets
  are numbered `0` to `buckets - 1`.

Then each `[[allocation]]`:

- `id` - required. Unique across **all** layers, not just this one, because a
  variable names its allocation without a layer qualifier.
- `status` - optional, one of `"draft"`, `"running"`, or `"concluded"`.
  Defaults to `"running"`. Only a running allocation assigns arms; while an
  allocation is draft or concluded, every unit resolves to the variable's
  default. That makes concluding an experiment a package edit, not a scramble.
- `eligibility` - optional. A boolean expression deciding who is enrolled at
  all. It can read `context` and `variables[...]` - so a condition variable
  like `enterprise_accounts` can keep a whole class of accounts out of an
  experiment - but not `entry`.

And each `[[allocation.arm]]`:

- `name` - required, snake_case, unique within the allocation.
- `buckets` - required. An inclusive range string like `"0-499"`, or a single
  bucket like `"7"`.

Arms across all allocations in a layer must claim disjoint buckets -
`rototo/layer-bucket-overlap` if two claims collide. Buckets nobody claims are
fine; a unit landing there is simply in no allocation. A file that doesn't
parse is `rototo/layer-parse-failed`, and anything else structurally off
(a missing `unit`, a zero `buckets`, a malformed range, a duplicate arm name)
is `rototo/layer-shape`.

### The variable side: `method = "allocation"`

A variable joins an allocation by declaring the third resolve method. Here's
`variables/checkout_cta_copy.toml`:

```toml
schema_version = 1
description = "Call-to-action copy on the checkout button"
type = "string"

[resolve]
method = "allocation"
allocation = "cta_copy_test"
default = "Place order"

[[resolve.assign]]
arm = "control"
value = "Place order"

[[resolve.assign]]
arm = "benefit_led"
value = "Place order, arrives in 2 days"
```

- `allocation` - required, the id of exactly one allocation. Since allocation
  ids are globally unique, this also pins the variable to exactly one layer.
  An id no layer declares is `rototo/variable-unknown-allocation`.
- `default` - **required**. Any unit that doesn't get an arm - ineligible, in
  an unclaimed bucket, or in a draft or concluded allocation - resolves to it.
- `[[resolve.assign]]` - exactly one per arm of the allocation. Each has the
  `arm` name and the `value` that arm assigns. A missing arm, a stray arm the
  allocation doesn't declare, or a duplicate is `rototo/variable-allocation-shape`.

Assign values are type-checked against the variable's declared `type`, exactly
like rule values. And as with queries, methods don't mix: `method =
"allocation"` must not declare `[[resolve.rule]]` tables or query keys.

At resolve time, rototo evaluates the layer's `unit` expression against the
caller's context, hashes the result (FNV-1a, salted with the layer id, so it's
deterministic and stable across rototo releases), and lands on a bucket. If the
allocation is running and the unit passes `eligibility`, the arm claiming that
bucket assigns its value. The resolution trace records the layer, the
allocation, enrollment, the bucket, and the arm, and `rototo resolve` prints
the assignment in the pathway:

```text
allocation checkout/cta_copy_test -> bucket 967 -> arm benefit_led
```

One boundary worth stating: rototo's job here is assignment - deterministic,
reproducible, recorded in the trace. Exposure logging ("unit U saw arm A")
belongs to the consuming app or SDK, and shipping the winning arm is a package
edit, not runtime state.

## Enums: closed sets of scalar values

A lot of configuration values aren't free-form. A plan tier is one of `free`,
`team`, or `business` - never anything else. If someone types `"buisness"` in a
rule value, you want that package to be unreleasable, not quietly shipping a
tier no code path handles.

You could declare the field as a plain `string` and hope review catches the
typo. Or you could build a catalog - but a catalog is for structured objects
with several fields, and it's heavy machinery for "one of these five strings."
An **enum** is the lightweight middle: it names a closed set of scalar values,
and lint checks every use against that set.

Like a catalog, an enum has a contract half and a values half. The declaration
lives at `model/enums/<id>.toml` and says what kind of scalar the members are:

```toml
schema_version = 1
description = "Account plan tiers"
type = "string"
```

The `type` is one of `string`, `int`, `number`, or `bool`. The members live at
`data/enums/<id>.toml`, under the same id:

```toml
members = ["free", "team", "business"]
```

The list has to be non-empty, free of duplicates, and every member has to match
the declared type. Both halves have to exist - a declaration with no members and
members with no declaration are each lint errors.

To use an enum, give a variable the type `enum:<id>`:

```toml
schema_version = 1
description = "The plan tier this account resolves to"
type = "enum:plan_tiers"

[resolve]
default = "free"

[[resolve.rule]]
when = 'context.account.paid == true'
value = "team"
```

Now every default and every rule value is checked against the member set. A
typo'd `"buisness"` fails lint, and so does an `enum:<id>` type that names an
enum the package doesn't declare. Enums also work inside schemas, through
`x-rototo-ref` - that's next.

## Schema references: `x-rototo-ref`

Catalog entries and context facts sometimes need to point at things the package
already defines. A page entry names its hero banner; a notification policy
names its email template. If that's just a plain string field, a renamed or
deleted target breaks silently. The `x-rototo-ref` annotation makes the link
explicit, so lint can verify it and resolution can follow it.

You put it on a field inside a JSON Schema, with a kind-prefixed target. The
target says what the field's values must be.

**`"x-rototo-ref": "catalog:<id>"`** pins a string field to the entry ids of
another catalog:

```json
{
  "type": "object",
  "required": ["hero", "title"],
  "properties": {
    "hero": { "type": "string", "x-rototo-ref": "catalog:hero_banner" },
    "title": { "type": "string" }
  }
}
```

An entry that sets `hero = "home"` now has to point at a real entry in the
`hero_banner` catalog, and lint fails if it doesn't
(`rototo/catalog-entry-unknown-reference`). At resolve time the reference is
**hydrated**: your app gets the full `hero_banner` entry in place of the id, not
the string. A value can also reach inside the target with
`"<entry>#<json-pointer>"`, like `"home#/cta"`, to pull one field out.

The target can be an array, `["catalog:email_template", "catalog:sms_template"]`,
when a field may point into any of several catalogs. Lint then checks the value
against all of them and flags an entry id that exists in more than one - an
ambiguous reference is an error, not a guess.

**`"x-rototo-ref": true`** is the object form, for when the *value* names the
catalog. The field is an object with `catalog` and `entry` keys, plus an
optional `pointer`:

```json
{
  "type": "object",
  "required": ["catalog", "entry"],
  "properties": {
    "catalog": { "type": "string" },
    "entry": { "type": "string" },
    "pointer": { "type": "string", "format": "json-pointer" }
  },
  "x-rototo-ref": true
}
```

**`"x-rototo-ref": "enum:<id>"`** pins a field to an enum's member set:

```json
{
  "type": "object",
  "required": ["tier"],
  "properties": {
    "tier": { "type": "string", "x-rototo-ref": "enum:plan_tiers" }
  }
}
```

Enum targets don't hydrate - the member already *is* the value - but they do
get checked: catalog entry values and evaluation-context sample values both
have to be members of the enum.

Where each target is allowed: catalog schemas can use catalog targets, enum
targets, and the object form. Evaluation-context schemas accept only enum
targets - context facts are caller data, so pointing them at catalog entries
doesn't mean anything, but pinning a fact like `account.tier` to a closed set
does.

## Evaluation contexts: the facts your app passes in

When your app asks rototo to resolve a variable, it passes in a bundle of facts
about the current request - who the user is, what country they're in, what's in
their cart. That bundle is the **evaluation context**, and an evaluation-context
schema pins down its shape so the package and the app can't quietly disagree
about it.

The schema lives at `model/context/<id>.schema.json` - again, plain JSON
Schema. (The concept keeps its full name, evaluation context; only the path is
short.) Here's a trimmed `request.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Evaluation context contract for request runtime inputs.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "user": {
      "type": "object",
      "properties": { "id": { "type": ["string", "integer"] }, "tier": { "type": "string" } }
    },
    "account": {
      "type": "object",
      "properties": { "plan": { "type": "string" }, "seats": { "type": "integer" } }
    },
    "request": {
      "type": "object",
      "properties": { "country": { "type": "string" } }
    }
  }
}
```

This is what lets lint catch drift: if a rule reads `context.user.tier` but
your schema never mentions it, that's a problem you want to hear about before a
release, not during one.

Alongside the schema you can keep sample contexts, in
`model/context/<id>-samples/`. Each is a JSON file - the filename is the
sample's id - that has to validate against the schema, including any
`x-rototo-ref` enum pins the schema declares. Here's
`premium_enterprise.json`:

```json
{
  "user": { "id": "user-123", "tier": "premium", "role": "admin" },
  "account": { "plan": "enterprise", "seats": 250 },
  "cart": { "total_usd": 300 },
  "device": { "platform": "web" },
  "request": { "country": "DE" }
}
```

Samples earn their keep three ways: they're realistic inputs you can feed to
`rototo resolve` while testing, they give lint real data to check rule coverage
against, and they make handy examples in docs and reviews.

## Custom lint: your own rules, in Lua

rototo's built-in [lint](./diagnostics.md) checks the structural stuff - that
files parse, references resolve, values match their types. But some rules are
specific to *your* domain and rototo can't guess them. "Users on the standard
tier must never get more than five projects." "A checkout heading can't be
empty." Those go in `lint/`, written in Lua.

A lint file defines a `register` function, and inside it you register one or more
rules. Here's `checkout_redesign.lua`:

```lua
function register(lint)
  lint:rule({
    id = "consumer-experience/checkout-heading-required",
    title = "Checkout heading is missing",
    help = "Set heading to visible checkout copy.",
    target = "/catalogs/checkout_redesign/entries",
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

A rule registration has five parts:

- `id` - the rule's identity, written `authority/rule-id`. The authority is
  yours to name (here, `consumer-experience`) - anything except `rototo`, which
  is reserved for built-in rules.
- `title` - a short summary of what went wrong.
- `help` - how to fix it.
- `target` - what the rule looks at (here, the entries of a catalog).
- `handler` - the name of the function rototo calls for each target.

There's also an optional `severity` (`"error"` or `"warning"`, default
`"error"`), and `target` itself defaults to `/`, the whole package.

The handler returns a list of problems. Each problem just needs a `message`;
returning an empty list `{}` means "all good." A problem can also carry a
`path` (a pointer into the target's value, used to anchor the diagnostic to
the exact line) and a `field`. The [diagnostics
reference](./diagnostics.md) covers how these show up next to the built-in ones.

### Target addresses

`target` is a logical address, not a file path - it survived the model/data
layout change unchanged, and it's how one rule fans out. A plural address runs
the handler once per instance; a singular one pins a single target:

- `/` - the whole package, once.
- `/variables`, `/variables/<id>` - every variable, or one.
- `/variables/<id>/values`, `/variables/<id>/values/<key>` - a variable's
  declared values.
- `/variables/<id>/rules`, `/variables/<id>/rules/<index>` - a variable's
  resolve rules, by position.
- `/catalogs`, `/catalogs/<id>` - every catalog, or one.
- `/catalogs/<id>/entries`, `/catalogs/<id>/entries/<key>` - a catalog's
  entries. This is the workhorse: one handler, run once per entry.
- `/evaluation-contexts`, `/evaluation-contexts/<id>` - context schemas.
- `/evaluation-contexts/<id>/samples`,
  `/evaluation-contexts/<id>/samples/<key>` - a context's samples.

### What the handler receives

The handler's first argument is always the whole `package`: `root`,
`manifest`, and maps of `variables`, `catalogs`, and `evaluation_contexts` by
id, so a rule about one entry can still cross-check anything else. The second
argument is the target instance, and its shape follows the address. Every
shape carries a `kind` field naming itself:

- a **catalog entry** is `{ kind, catalog, key, path, value }` - `value` is
  the entry's data, which is what most rules inspect;
- a **variable** is `{ kind, id, path, description, declaration, values,
  resolve, toml }`;
- a **value** is `{ kind, variable, key, value, origin }`;
- a **rule** is `{ kind, variable, index, when, value }`;
- a **catalog** is `{ kind, id, path, json, entries }` - `json` is the schema;
- an **evaluation context** is `{ kind, id, path, json, samples }`;
- a **sample** is `{ kind, evaluation_context, key, path, value }`;
- the **package** target (`/`) gets `{ kind, root, manifest, extends }` -
  everything else is already on the first argument.

## How it all gets distributed

When you're ready to ship a package to production, `rototo package` bundles this
whole folder into a single `.tar.gz` file. That archive is **deterministic**:
the same package always produces the exact same bytes - entries are sorted,
timestamps and permissions are fixed, ownership is zeroed, compression is
pinned. Because the contents fully determine the bytes, the archive is named by
its own SHA-256 digest, like `sha256:0f4c...b91.tar.gz`.

That determinism is what makes a release trustworthy: the same commit always
gives the same digest, so a digest is a precise, reproducible name for "exactly
this config." How you serve and load that archive is the [package
sources](./package-sources.md) story.
