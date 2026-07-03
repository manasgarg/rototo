# Tenant-specific configuration: design and prior art

Status: design research and requirements framing. Not implemented.

## What we are trying to model

Today rototo models a single global configuration well: one package, authored by
one team, defines qualifiers, variables, catalogs, schemas, and values, and apps
resolve typed values from it at runtime.

We now want to model **tenant-specific configuration**. The shape is a two-party
authoring split:

- **App developers** release a base package that defines everything the app
  depends on: all variables the app reads (with their types), the catalog
  schemas the app supports, the evaluation-context schemas, the qualifier
  definitions, and sensible default values and rules.
- **Each tenant** customizes within the bounds the app developer allows. A tenant
  can add their own catalog entries, enable or disable provided catalog entries,
  and choose rules for variables. A tenant may also define their own qualifiers
  to use in their rules.

The base package is the contract. The tenant overlay is a thin, governed
customization on top of it. At runtime the app resolves values for a specific
tenant against the composed result of base plus that tenant's overlay.

## Data model and constraints (the foundational frame)

Everything below is an instance of one frame, so it is worth stating first.

### Two data models, one nested inside the other

rototo has two data models, and the same "declaration plus the set of values it
allows" pattern operates at both. The confusion in reasoning about types comes
from seeing one pattern twice and trying to make it one thing. It is one pattern,
applied at two levels, by two owners.

- **rototo's data model (the meta-model).** The fixed vocabulary of primitives:
  package, qualifier, variable, evaluation-context, catalog, and later others such
  as experiment layers. rototo defines these and constrains them. This is the
  package format grammar, enforced by built-in lint (`src/lint/project/*`), not by
  authored JSON Schema. Adding a new primitive is rototo extending its own
  meta-model, not something an app author does.

- **the application's data model (the domain model).** The specific instances an
  app author writes using those primitives: a `premium-users` qualifier, a
  `checkout-redesign` variable, an `orders` catalog shape, a request context
  shape. The app author defines these and constrains them with type, shape,
  references, and possible values.

So "is JSON Schema what validates my entities?" has a precise answer: the entity
grammar is rototo constraining its meta-model (built-in lint), while JSON Schema
is the app constraining its domain model (catalog and context shapes). Two levels,
two owners, sometimes different notations.

### The anchor: it is a small typed relational model

The domain model maps onto the database model every engineer already knows:

| rototo | database |
| --- | --- |
| catalog | table |
| catalog entry | row |
| catalog schema | the table's column types |
| reference (`catalog:<id>`, entry ids) | foreign key |
| `query` / `list<catalog:...>` | SELECT / join |
| variable | computed column or view that picks a value |
| qualifier | WHERE predicate over the request |
| evaluation context | the input row (the request's typed facts) |

The two-level structure has a database parallel too: a database has your tables
(the domain model) and the system catalog or `information_schema` that describes
your tables (the meta-model). Same engine, two levels. rototo's meta-model is its
`information_schema`; your catalogs are your tables.

Note on joins: today a `query` ranges over a single catalog's entries. True
cross-catalog foreign keys and joins are the natural next step this frame
predicts but that is not yet first-class. The relational framing is useful
precisely because it names where the next data-model extension goes.

Note on direction: references point from derived to basic. A variable (a view)
may reference variables, catalogs, and enums. A catalog entry (data) may reference
catalogs and enums, never a variable, because a foreign key is a set-membership
check and a variable is not a set of values, it is a computation. If a data field
seems to want to point at a variable, either the referent is really data (model it
as a catalog or enum) or the pointer belongs in a variable's resolution, in the
logic layer, not in the data. A catalog field may still hold an authored
expression (for example an audience's `condition`), which is declarative data the
engine evaluates, not a reference to a variable.

### Primitives are typed answers to runtime questions

The primitives are distinguished by which runtime question they answer and what
kind of typed thing they are:

| Primitive | Question | What it is | Value |
| --- | --- | --- | --- |
| Qualifier | what is true? | a named predicate over the request | `bool` |
| Variable | what is the value? | a named selected value | scalar / object / array / reference |
| Catalog | what objects exist? | a typed set of rows | objects conforming to a shape |
| Evaluation context | what facts came in? | the input row's type | object |
| Layer | which arm is this unit in? | a bucketed allocation over a diversion | first-class primitive (see Experimentation) |

A new primitive is rototo extending the meta-model. The frame says that is where
it belongs: it is added as vocabulary, not bolted onto the domain model.

This table is the current shape. The section "Variables, resolvables, and
resolution models" below reshapes it: qualifier and variable collapse into a
single `resolvable` primitive, so the durable core is catalog, variable, and
evaluation context.

### Constraints and values, unified

A constraint names an allowed set of values. The kinds of constraint are just
different ways to bound that set:

- **type**: the value's family (which scalar kind),
- **shape**: the structure of an object or array,
- **reference**: membership in a named set (the foreign key or enum by
  declaration),
- **possible values**: an explicit enumeration.

Values (scalar, object, array) are what fill the set. This holds at both levels:
rototo bounds "what a valid variable file is," the app bounds "what a valid order
is," using the same four constraint kinds. Only the author changes.

### Three nested authorities

Tenancy is a third owner that nests cleanly inside the app's domain model:

```
rototo   owns the META-MODEL   (primitives + grammar)       nobody else changes it
  |__ app owns the DOMAIN MODEL  (which quals/vars/catalogs)  expressed within rototo's model
        |__ tenant owns an OVERLAY  (rows, rules, namespaced conditions)  within the app's model, may only narrow or add
```

Each level is authored inside the one above and may not violate it. That
"authored within, may only narrow" relationship is the same meet and narrowing
lattice used for composition, applied here to authorship rather than to values.
The tenant contract, the composition algebra, and this type and constraint model
are the same structure seen from different sides.

## Variables, resolvables, and resolution models

The primitives table above is the current shape. This section reshapes it: the
`catalog:schema` relationship has a twin for variables, and following it collapses
qualifiers and variables into one primitive.

### A variable is a type plus a resolution model

A catalog is one generic container parameterized by a schema, which is what lets
it capture any data model. A variable should be the same: one generic container
parameterized by a resolution model, which is what lets it capture any resolution
model. The schema analog for variables is the resolution model.

So a variable decomposes into two orthogonal parts:

- **type**: what the answer is (scalar, object, array, reference), from the
  constraint and type system.
- **resolution model**: how the answer is computed (`rules`, `query`, or
  `allocation`).

They connect through the constraint engine: the type validates the resolution
model's output. The resolution model is typed by the variable's type, so this adds
no new validation story, it reuses the existing one.

### Resolution model is a small, curated vocabulary

There are three resolution models, distinguished by how they pick a value. The rule
that keeps this from becoming a programming language: **resolution selects an
authored value, it never computes one.** Expressions appear only in guards,
predicates, and keys (which decide *which* value); the value itself is always
authored data, a literal, an enum member, or a reference to a catalog entry.

| Model | Candidate space | How it picks | Output |
| --- | --- | --- | --- |
| `rules` | authored guarded rules | first rule whose `when` is true, else `default` | one value, any type |
| `query` | a catalog's entries | a predicate over entries, then reduce | one entry or a set |
| `allocation` | a layer's arms | the unit's bucket assignment (see Experimentation) | one value, any type |

Everything that read like a separate model in earlier drafts is a *parameter of
`query`*, not a new name:

| Earlier name | Is really |
| --- | --- |
| `filter` | `query` on a `list<catalog>` variable (returns the set) |
| single select | `query` on a `catalog` variable (returns the top entry) |
| `ranked` | `query` with a `sort` key, plus `default` |
| `most-specific` | `query` with a `sort` key that ranks by specificity |
| `effective-dated` | `query` whose `filter` tests `env.now` against a window |
| `project` | `query` with a `map` expression |
| top-N | `query` with `limit` |

The resolution is named by a `method` field in a flat `[resolve]` block (not a
sub-table, and not `model`). `query`'s knobs are CEL expressions for filter, sort,
and map, plus a few scalars:

- `from`: the catalog.
- `filter`: an optional CEL predicate over `entry` plus `context`/`env`/`variables`.
- `sort` and `order`: an optional CEL sort key and `asc`/`desc`.
- `map`: an optional CEL projection of each entry (the projected type must match the
  variable's type).
- `limit`: an optional cap on how many entries are returned.
- `default`: the value when a single-valued query matches nothing.

```toml
[resolve]
method = "query"
from = "offers"
filter = 'entry.category in ["seasonal", "evergreen"] && matches_audience(entry.audiences)'
sort = 'entry.priority'
order = "desc"
default = "welcome_hero"
```

Whether a query returns one entry or a set comes from the variable's type, not a
keyword: a `catalog:x` variable takes the top entry after `filter` and `sort`, and a
`list<catalog:x>` variable takes the whole filtered, sorted, mapped, and limited set.
`rules` carries `default` plus `[[resolve.rule]]` entries (`when`, `value`);
`allocation` carries an `allocation` id, `default`, and `[[resolve.assign]]` entries
(`arm`, `value`). `most-specific` and single-match uniqueness are no longer named
parameters: most-specific is a `sort` key the author writes, and uniqueness is a lint
or runtime check that a single-valued query's `filter` matches at most one entry.

`bucket` and `weighted` are gone as resolution models. Bucketing is not a way a
variable resolves; it is a first-class assignment primitive (a layer), and
`allocation` is the resolution model that reads it. See "Experimentation" below.

Two guardrails keep this from reopening settled questions:

- **Declarative, engine-interpreted, not code.** Each model's parameters are CEL and
  literals, which preserves the invariant (resolution never runs package code), the
  resolution trace, and multi-SDK portability.
- **Type constrains the model.** A `list<catalog>` variable resolves with `query`
  (returning the set); a `catalog` variable uses `query` (the top entry), `rules`, or
  `allocation`; a scalar uses `rules` or `allocation`. "Which model is legal for which
  type" is a dependent constraint, the exact self-hosting feature the engine already
  needs.

### Qualifiers dissolve into variables

A qualifier is just a `bool`-typed variable whose resolution model is a predicate.
Its apparent separateness is a role (named, reused as a building block, referenced
via `env.qualifier[...]`), not a mechanism. So everything is a **resolvable**: a
named, typed thing with a resolution model. Qualifiers were the `bool`-typed
resolvables used as building blocks; variables are the ones the app reads.

Taking this to its conclusion: drop the qualifier concept entirely and make every
variable referenceable in expressions as `variables["id"]`. `env.qualifier["id"]`
was only ever a special-cased reference to one kind of variable.

This closes a structural gap for free. `env.qualifier["id"]` could reference only a
boolean resolvable; `variables["id"]` references any variable of any type, so a
rule's guard can read another variable's value, not just a boolean condition. That
is cross-variable dependency, one of the open structural gaps, filled by the same
move that removes the qualifier primitive.

### The base-tables-plus-views model

This completes the "resolution as querying a small database" frame. The resolution
environment is:

| Table | What it is |
| --- | --- |
| `context` | the input row (request facts) |
| `catalogs` | the data tables (rows) |
| `env` | rototo-provided values (`now`, `tenant`) |
| `variables` | derived views, computed by resolution models, referenceable by other views |

Qualifiers were the boolean views. Now all variables are views, and any view can
read other views. `variables["id"]` is "read this view". The system is a database
with base tables and views.

### Costs and what to preserve

- **Cycle detection generalizes.** Qualifiers already have cycle detection and
  per-resolution memoization. Now that any variable can reference any other, that
  machinery spans the whole variable graph. Same mechanism, widened; cycles are a
  structural property caught at lint.
- **Type-checking improves.** `env.qualifier[...]` was always `bool`.
  `variables[...]` is typed by each variable's declared type, so using a
  `list<catalog>` variable where a bool is expected becomes a lint error.
- **Recover the lost intent as an attribute, not a primitive.** The qualifier
  concept signaled "reusable condition" and, more importantly, "internal helper,
  not an app-facing value." Once every variable is a referenceable view, app-facing
  outputs and internal helpers blur, risking dependency spaghetti. Add a
  visibility or intent marker (exported/app-facing versus internal, or a
  `condition` role) and optionally gate referenceability on it, so the dependency
  graph stays disciplined. Keep the role, drop the mechanism.

### The resulting primitive core

- **catalog**: the object value space (tables plus schema),
- **enum**: the scalar value space (a named set of allowed scalars); see below,
- **variable** (resolvable): the resolution model (type plus resolution model),
  referenceable in expressions as `variables.<id>`,
- **evaluation context**: the input shape,
- **layer**: the assignment space (bucketed allocations that drive variables); see
  Experimentation below.

Qualifier is gone. The three runtime questions collapse cleanly: "what is true?"
and "what is the value?" are both variables (a bool one versus any other), while
"what objects exist?" stays a catalog. The durable core is catalog, enum, and
variable over the input context, with the layer as a fourth primitive for
assignment; an extensible resolution-model vocabulary lives inside variables and an
extensible schema inside catalogs.

## Experimentation: layers and allocations

Feature rollouts and A/B experiments are the same mechanism, so rototo models them
as one primitive rather than as a resolution-model bolt-on. It must be a primitive,
not a per-variable resolution parameter, for one reason: consistency. An experiment
drives many variables at once (the layout, the copy, the CTA), and every one must
read the *same* assignment for a given unit. Only a shared, named entity can
guarantee that.

### The mental model: slice a line of units

Hash each unit (say `context.user.id`) into a stable position on a line of `buckets`
(0 to N). That position never changes for a unit. An **allocation** claims a set of
buckets and divides them among **arms**; each arm assigns values to variables. A
**layer** is the line: allocations in one layer claim disjoint buckets, so a unit is
in at most one of them (mutual exclusion). Different layers are independent lines (a
unit can be in one allocation per layer), and they are safe to overlap because each
layer owns a disjoint set of variables.

That is the two-partition design every overlapping-experiment system converges on
(Google's layers, Statsig, Optimizely): **traffic** is partitioned within a layer
(disjoint buckets give mutual exclusion), and **variables** are partitioned across
layers (disjoint ownership makes overlap conflict-free).

### The entities

- **layer**: a diversion (`unit`, `buckets` count) plus a set of allocations. It owns
  a disjoint set of variables, derived from which variables reference its
  allocations. One file per layer under `layers/`.
- **allocation**: claims buckets, grouped into arms; optional `eligibility` (a CEL
  gate on who is enrolled) and `status` (draft/running/concluded). There is no
  rollout-versus-experiment `kind`: a plain rollout and a measured experiment are the
  same shape used differently, and the distinction drives no engine behavior (a
  control arm and exposure logging are analysis conventions and consumer concerns,
  not resolution).
- **arm**: a named slice of an allocation's buckets.

### How a variable consumes it

An allocation-driven variable resolves entirely from the arm assignment, with a
`default` for units in no arm (ineligible, or unclaimed buckets):

```toml
# layers/checkout.toml
unit = 'context.user.id'
buckets = 100

[[allocation]]
id = "cta_copy"
status = "running"
eligibility = 'context.account.plan_tier != "enterprise"'

[[allocation.arm]]
name = "control"
buckets = "0-49"

[[allocation.arm]]
name = "benefit_led"
buckets = "50-99"
```

```toml
# variables/checkout_cta_copy.toml
type = "string"

[resolve]
method = "allocation"
allocation = "cta_copy"
default = "click here"

[[resolve.assign]]
arm = "control"
value = "click here"

[[resolve.assign]]
arm = "benefit_led"
value = "save time, check out faster"
```

A variable names exactly one allocation, so it belongs to exactly one layer; the
variable-to-one-layer partition enforces itself with no separate ownership list.
This is the v1 shape: an allocation-driven variable's non-experiment value is the
flat `default`. Running an experiment *on top of* rich base resolution (normally
resolve by market, and also A/B test it) is the deferred "experiment as an override
above `rules`/`query`" fork.

### The lint

- Within a layer, arms across all allocations claim disjoint buckets inside
  `[0, buckets)`; unclaimed buckets mean "in no allocation" and resolve to `default`.
- A variable names exactly one allocation and covers exactly that allocation's arms
  (no missing arm, no stray arm).
- Every `assign` value and the `default` satisfy the variable's type.
- Arm names are unique within an allocation.

### Boundaries

Assignment is rototo's job: it computes the deterministic, reproducible arm for a
unit and records it in the resolution trace. Exposure logging (emitting "unit U saw
arm A") is the consuming SDK's job, and concluding an experiment (shipping the
winning arm) is a package edit, not runtime state. So the loop is: rototo assigns,
the app exposes, an external analysis picks a winner, a package edit ships it.
Layers and allocations compose and are governed like any entity (`[layer.<id>]` in
`governance.toml`), so tenant-scoped experiments fall out of the same machinery.
Bucketing folds into the layer; a standalone `bucket` primitive is only needed if
two layers must share a diversion for correlated assignment, deferred until there is
a case.

## Enum: the scalar value space

A catalog is a named set of objects. rototo also needs a named set of scalars: an
**enum**. Decision: keep catalog and enum as two named concepts that share the same
plumbing, rather than one "value space" concept that has to explain why its members
sometimes have fields and sometimes do not. The scalar-versus-object difference is
real (enum members have no id and no fields; you only test membership), so two clean
surfaces over shared machinery beats one branching mechanism.

Note on naming: what this captures is a named *set of allowed values* (an
enumeration or domain), not the `list<T>` value type. It is a set; membership is the
operation. "enum" is used here to avoid colliding with `list<T>`, but the exact word
("enum", "domain", "value set") is an open naming decision, as is whether catalog
and enum get a shared family name.

### Catalog and enum side by side

| | members are | keyed | member-of type | supports |
| --- | --- | --- | --- | --- |
| catalog | objects (rows) | yes: id is separate from value, dereference id to object | `catalog:orders` | `query` over fields |
| enum | scalars | no: the value is the member | `enum:tiers` | membership (`in`) |

In the base-tables-plus-views model, a catalog is a table and an enum is a
single-column lookup table (a domain).

### The two uses are one operation in two positions

An enum serves both as a condition parameter in CEL and as a constraint on a
variable's value. These are not two features. They are the same membership test on
the same named set, aimed at different things:

- **CEL condition:** `context.user.country in enum["supported-countries"]`. Is this
  input field in the set?
- **Value constraint:** `type = enum:tiers`. Is this variable's resolved value in the
  set?

A value constraint is literally "the resolved value must be `in` the enum", the same
membership test as the condition use, applied at the variable's output position and
checked at author time (defaults and rule values must be members; lint catches
typos). This is the "a constraint is a set of allowed values" unification made
concrete: the enum is the named set, and membership is used as a predicate (CEL) or
as a constraint (type).

### Why a named enum beats an inline list

- **Single source of truth.** The same `tiers` domain constrains the variable and
  appears in conditions. Change it once.
- **Reuse in CEL.** A JSON Schema `enum` keyword inlined in a shape cannot be
  referenced from a CEL expression. A named enum is usable in both the type system
  and the expression language. That cross-use is the reason to make it a first-class
  named thing.
- **Member-of typing.** `enum:tiers` is a nominal type, parallel to `catalog:orders`.

### Boundaries

- **Enum is the enumerated kind of scalar domain.** Ranges (`int & 1..100`) and
  patterns are other scalar constraints that JSON Schema already handles in the
  shape layer. Do not stretch "enum" to mean all scalar constraints.
- **The catalog/enum dividing line is scalars versus objects.** The moment a member
  needs fields or metadata (a label, a description, a price), it is a catalog. Enum
  members are bare scalars.

### It falls out of everything else

- **Self-hosting dogfoods it.** rototo's own set of primitive type names
  (`bool | int | number | string`) is an enum, so adding the primitive validates part
  of rototo's own grammar.
- **Tenancy covers it automatically.** An enum is a set of members, so it composes
  exactly like catalog entries: union of members plus tombstones to disable. Tenants
  extend or lock enums with the same algebra.
- **Resolution typing already handles it.** `enum:tiers` as a variable type means the
  resolution model's output is validated against the enum by the same constraint
  engine.

## Representations versus the model

A catalog stores each entry as its own file under `catalogs/<id>-entries/*.toml`.
A "table" is a compact form of the same thing: many rows in one file, useful for
flat homogeneous entries and for compact git diffs. Same data model, two
representations. This is the "boring surface, rigorous engine" pattern again, with
one wrinkle: rototo lints with text spans, so a naive reading says every
representation multiplies the lint and diagnostics code. It does not, if the seam
is placed correctly.

### Spans are a parser concern, not a lint concern

Set up a narrow waist:

```
surface text  ->  [ parser + span mapping ]  ->  canonical entry model (objects with spans)  ->  [ one lint + resolve engine ]
```

The expensive, valuable part (semantic lint and precise diagnostics) is written
once, against the canonical model. A span there is just "file plus range". The
parser attaches correct spans; lint reasons about entries and emits diagnostics in
canonical-model terms. So a new representation costs only its front-end (parse plus
span mapping). Lint, resolution, and the diagnostics catalog do not change.

The litmus test: lint code must never branch on "am I looking at a table or an
entries directory". If a representation forces a change to lint logic, that is a
coupling bug in the seam, not an inherent cost, and the fix is to push spans into
the canonical model.

### The cost is per subsystem the representation touches

| Subsystem | New representation costs code |
| --- | --- |
| lint (read) | yes: a parser producing canonical entries plus spans |
| resolution | no: runs on the canonical model |
| diagnostics | no: emitted in canonical-model terms |
| console authoring (write) | only if you author in that form: a serializer plus UI |
| docs and examples | minor |

So the cost depends on how many subsystems the representation reaches, not on the
representation existing.

### The lever: reuse the span-aware parser you already own

rototo already parses TOML with spans. If the compact table form is a TOML
array-of-tables in one file:

```toml
[[entry]]
id = "black-friday"
min_cart = 100
countries = ["DE", "FR"]

[[entry]]
id = "welcome"
min_cart = 0
countries = ["US"]
```

then all the TOML span machinery is reused, and the only new code is discovery and
id derivation: read entries from one array-of-tables file and take the id from an
`id` field, instead of from many files with the id from the filename. Spans, and
cell-level diagnostics, come for free. Choosing a bespoke format (CSV, a custom
grammar) is what multiplies code, because it owes a whole new span-aware parser. The
compact syntax you pick controls the cost far more than the decision to have two
representations.

### Let the data shape pick the representation

The table form fits flat, homogeneous rows. Deeply nested objects do not fit a 2D
table cleanly, so they stay per-entry files. Same principle as scalars-versus-objects
choosing enum-versus-catalog: the representation is chosen by the entries' shape, and
each catalog stores itself as either an entries directory or a table file.

### Committed versus convenience

If the table is a committed on-disk form (for compact git diffs, the stated
benefit), both forms are valid on disk, chosen per catalog, and both lower into the
same canonical model. If it were only an authoring convenience, it could lower to
canonical storage on save, which is cheaper but loses the compact-diff win. The
stated goal points at committed.

## Package organization

An entity-oriented layout (a catalog directory holding its schema and its entries
and its constraints; a variable file holding its type and resolution and
visibility) piles every concern onto one place and is hard to hold. Organize by
concern instead, the way a database is reasoned about. A config control plane is a
data system, so lay it out like one.

### The concern-to-directory mapping

| Concern | Database analog | Directory |
| --- | --- | --- |
| the data model (shapes, types, references) | DDL (`CREATE TABLE`, `CREATE TYPE`) | `model/` |
| the instances (entries, members) | the rows | `data/` |
| resolution (variables) | views (`CREATE VIEW`) | `variables/` |
| custom check residue (scripts) | triggers and procedures | `checks/` |
| the layering contract | grants | `governance.toml` |

The rule that makes it easy to hold: every entity is decomposed across the concern
directories. A catalog's shape is in `model/`, its rows in `data/`, its policy in
`governance.toml`. A variable's type and resolution are in `variables/`, its visibility
and lock policy in `governance.toml`. To reason about a concern, look in one directory;
to reason about an entity, follow its consistent name across a few directories.

### The concern split is the authorship boundary

The directories line up with the three authorities:

- `model/` + `governance.toml` + `checks/` are the app-owned contract (rarely changes),
- `data/` + `variables/` are the editable surface (what layers extend and override).

So an overlay mirrors the same tree but almost always writes only under `data/` and
`variables/`. Touching `model/` or `governance.toml` is immediately visible as "this
layer is trying to change the contract". The layout encodes the layering boundary.
It is also the self-hosting structure: rototo's own meta-model, described as a
package, uses the same `model/` and `data/` split.

### Decisions this forces

- **Enum splits, for consistency.** The enum declaration (scalar type, description)
  is in `model/enums/`, its members in `data/enums/`, exactly like a catalog's schema
  versus entries. Slightly verbose for a tiny enum, but a uniform rule
  ("definitions in `model/`, values in `data/`, always") is easier to hold, and it
  keeps enum members as tenant-extensible data.
- **"Constraints" is not one directory.** Shape, reference, and dependent
  constraints are the model, so they live in `model/`. Only the script residue and
  cross-cutting custom checks get `checks/`.
- **Governance is extracted, not embedded.** A variable file is just type plus
  resolution; its visibility and locked/open policy move to `governance.toml`. That is
  what pulls governance out of every entity.
- **Trade-off.** Aspect layout optimizes "reason about a concern" at the cost of
  "everything about one entity in one folder". For a data system that is the right
  trade; consistent naming keeps entity-following mechanical.

### Worked example: base package

```
plans-app/
├── rototo-package.toml
├── model/
│   ├── catalogs/plans.schema.json          # references enum:support-tiers and catalog:features
│   ├── catalogs/features.schema.json
│   ├── enums/regions.toml                   # { type = "string", description = ... }
│   ├── enums/support-tiers.toml
│   └── context/request.schema.json
├── data/
│   ├── catalogs/plans/free.toml
│   ├── catalogs/plans/growth.toml
│   ├── catalogs/plans/enterprise.toml
│   ├── catalogs/features.table.toml         # compact table form
│   ├── enums/regions.toml                    # members = ["us", "eu", "apac"]
│   └── enums/support-tiers.toml              # members = ["community", "standard", "priority"]
├── variables/
│   ├── is-enterprise.toml                    # internal bool condition
│   ├── active-plan.toml                      # catalog:plans, first-match
│   ├── enabled-features.toml                 # list<catalog:features>, filter
│   └── support-tier.toml                     # enum:support-tiers, first-match
└── governance.toml                           # the layering contract (see Governance section)
```

`model/enums/support-tiers.toml` (declaration only) and
`data/enums/support-tiers.toml` (values only):

```toml
# model/enums/support-tiers.toml
schema_version = 1
description = "Support tiers"
type = "string"
```

```toml
# data/enums/support-tiers.toml
members = ["community", "standard", "priority"]
```

`model/catalogs/plans.schema.json` carries the relationships (catalog to enum,
catalog to catalog):

```json
{
  "type": "object",
  "required": ["monthly_price", "support_tier", "included_features"],
  "properties": {
    "monthly_price":     { "type": "number", "minimum": 0 },
    "support_tier":      { "type": "string", "x-rototo-ref": "enum:support-tiers" },
    "included_features": { "type": "array",
                           "items": { "type": "string", "x-rototo-ref": "catalog:features" } }
  },
  "additionalProperties": false
}
```

`variables/active-plan.toml` (type plus resolution only; visibility and policy would
live in `governance.toml`):

```toml
schema_version = 1
description = "The plan governing this account at runtime"
type = "catalog:plans"

[resolve]
model   = "first-match"
default = "growth"

[[resolve.rule]]
when  = 'context.account.plan_key == "free"'
value = "free"

[[resolve.rule]]
when  = 'variables["is-enterprise"]'
value = "enterprise"
```

### Worked example: the Acme overlay

The overlay mirrors the tree and touches only `data/` and `variables/`, the editable
surface. Every membership mechanism appears once.

```
acme-overlay/
├── rototo-package.toml                       # extends = [ "git+...plans-app#main" ]
├── data/
│   └── catalogs/plans/
│       ├── acme-enterprise.toml              # UNION: new entry
│       ├── free.tombstone.toml               # TOMBSTONE: disable base 'free'
│       └── growth.patch.toml                 # PATCH: override select fields
└── variables/
    ├── active-plan.toml                      # OVERRIDE: replacement [resolve] block
    └── acme/in-trial.toml                    # ADD: namespaced internal condition
```

```toml
# data/catalogs/plans/growth.patch.toml  (field-level override; other fields inherited)
monthly_price = 59
```

```toml
# data/catalogs/plans/free.tombstone.toml
tombstone = true
reason = "Acme does not offer a free plan"
```

```toml
# variables/active-plan.toml  (a complete replacement [resolve] block; type stays with the base)
[resolve]
method = "rules"
default = "acme-enterprise"

[[resolve.rule]]
when  = 'variables["acme/in-trial"]'
value = "growth"
```

The overlay never touches `model/` or `governance.toml`, so a reviewer sees at a glance
that Acme changed data and resolution, not the contract.

## Self-hosting the meta-model

Like a database whose system catalog is itself made of tables, rototo should
describe and constrain its own meta-model with the same mechanism the app uses
for its domain model. Today the meta-model is validated by hand-written lint
(`src/lint/project/*`) while the domain model is validated by JSON Schema plus the
type language. Self-hosting collapses that into one engine.

### The same need already appears at both levels

The strongest evidence that the two levels should share one engine is that they
already demand the same feature:

- Domain level: a `catalog:orders` variable's value must be a member of the set
  named by `orders`. The allowed set of this field depends on another
  declaration.
- Meta level: a variable's `resolve.default` must match the value type named by
  its `type` field. The allowed set of this field depends on a sibling field.

Both are the same constraint kind, a dependent constraint where a value's allowed
set is derived from other data. Plain JSON Schema expresses neither. When the same
need shows up at both levels, that is the signal for one engine, not two.

### One engine, three authors

Self-hosting collapses "built-in lint" and "custom lint" into one mechanism that
differs only by author. rototo authors the meta-model constraints, the app authors
domain constraints, the tenant authors narrowing constraints. That is the three
nested authorities applied to constraints specifically: one constraint engine,
three authors, nested, each authored inside the one above and forbidden from
violating it.

### Where self-hosting bottoms out

Self-hosting shrinks hand-written validation to a kernel; it does not reach zero.
Three residues remain:

1. **A bootstrap fixed point.** Something must describe shapes that describe
   shapes. A tiny hand-written root meta-schema is the axiom, not validated by a
   lower level, exactly like a metaclass that is an instance of itself, or a
   database's bootstrap catalog rows.
2. **A semantic residue that is not data.** Genuinely algorithmic checks
   (qualifier-reference cycle detection, CEL expression typing against the roots,
   bucket semantics) stay as built-in engine code, like a database implementing
   its query planner in code rather than in SQL.
3. **Diagnostics as a contract.** The generic validator must still emit precise,
   located, stably-identified diagnostics (for example `rototo/variable-unknown-value`
   at a span), not a flat "schema validation failed at /resolve/default".

The useful consequence: rototo's own meta-model becomes the acceptance test for
the constraint engine. The engine is expressive enough exactly when it can
validate rototo's own grammar, because the meta-model is the hardest case.

### The residue symmetry

Both levels have the same three-part structure, which is why the external-script
escape hatch (see custom lint below) is not a bolt-on but a predicted piece.

| | rototo's meta-model | app's domain model |
| --- | --- | --- |
| declarative constraint layer | meta-catalog shapes, references, dependent constraints | catalog and context schemas, variable types, references, CEL rules |
| semantic residue | built-in engine code (cycles, expression typing, bucket) | external scripts (author-time checks) |

External scripts are "the app's engine code", the app-level analog of rototo's
own built-in semantics. Two orthogonal axes explain where each runs:

- **Expressiveness axis:** declarative constraint (the core language can say it)
  versus semantic residue (it cannot). Both levels have both.
- **Trust axis:** rototo's own code (trusted, runs everywhere including load)
  versus package-supplied code (untrusted, author-time only).

These are independent. rototo's residue runs at load because the code is rototo's.
The app's declarative layer also runs at load, because rototo's trusted engine
evaluates app-supplied data (the expression is the app's, the evaluator is
rototo's, sandboxed). The app's residue does not run at load, because it requires
running the app's own code. So the runtime-contract versus author-time-gate line
is not the declarative-versus-residue line, it is the trust line. The recursion
continues to the tenant, whose residue runs only in the tenant's own trusted
context, never at load and never in the multi-tenant console server.

### Constraint representation: extended JSON Schema

JSON Schema should be the canonical internal form of the declarative constraint
layer at both levels, but it is the shape sub-language inside a broader constraint
model, not the whole model. Decomposing the needs:

| Constraint need | JSON Schema | How |
| --- | --- | --- |
| shapes, scalars, enums, numeric and string bounds, required/optional | native | JSON Schema's core |
| combining constraints (AND, OR) | native | `allOf`, `anyOf` |
| references (`catalog:<id>`, entry ids, qualifier refs) | extension | `$ref` is a structural include, not a nominal "member of this data set". Needs a rototo keyword plus a resolver |
| dependent constraints (value matches a sibling field) | extension | beyond `if/then`; needs a rototo keyword resolving against other data |
| validation predicates (custom lint) | extension | a keyword carrying a CEL body, for example `x-rototo-check` |
| expression typing (the three roots, operators) | no | not shape validation; semantic residue, stays engine code |
| algorithmic checks (qualifier cycles, bucket) | no | not data; stays engine code |

Two properties make JSON Schema the right base for the self-hosting goal
specifically:

- **The bootstrap kernel is free.** JSON Schema already has a meta-schema, a JSON
  Schema that describes JSON Schemas. That is the self-hosting fixed point, from
  the standard rather than invented.
- **`allOf` is the meet.** The composition algebra (narrowing = meet, an overlay
  may only add constraints) is how `allOf` already works. A tenant overlay
  narrowing the base is the base shape wrapped in an `allOf` with the tenant's
  extra constraint. The constraint representation and the composition operation
  are the same thing.

Two firm boundaries:

- **Not the authoring surface.** JSON Schema is verbose and authoring is in TOML.
  Keep terse surfaces (the `list<catalog:orders>` type language, TOML shorthands)
  that desugar into the canonical extended JSON Schema. `catalog:orders` is sugar
  for `{type: string, x-rototo-ref: catalog:orders}`. Boring surface, rigorous
  engine.
- **rototo owns validation and diagnostics.** JSON Schema is the representation,
  not the validator experience. rototo drives validation and emits its own
  located, stably-identified diagnostics.

One distinction to keep crisp: CEL plays two roles and only one is a constraint.
Validation predicates (custom lint) are constraints and can live as an
`x-rototo-check` keyword. Resolution-time CEL (`when`/`query`) is runtime selection
logic, not validation; it is typed by the constraint model (its roots have
schemas) but is not part of it. The schema types those expressions, it does not
contain them.

The real design work is not "adopt JSON Schema", it is specifying the few
extension keywords (`x-rototo-ref`, the dependent-type keyword, `x-rototo-check`)
precisely, because that is where rototo's actual constraint semantics live.

## Why today's `extends` cannot carry this

`extends` (see `src/source/layer.rs`) is the right foundation but the wrong
granularity. It is a file-tree overlay with whole-file, last-writer-wins
replacement, flattened at load time: each parent is copied into a temporary
directory, then the child's files overwrite by path
(`copy_package_layer_recursive`). That gives exactly three operations:

1. add a new file,
2. replace a whole file at the same path,
3. nothing else.

The tenant model needs precisely the things it cannot do:

- **No deletion or disable.** A child can only add or overwrite a parent file,
  never remove one. "Enable or disable a provided catalog entry" is structurally
  impossible. There is no tombstone.
- **No sub-file merge.** Override granularity is the whole file. To change one
  rule or just the default of a variable, a tenant must copy and re-own the
  entire variable definition, then silently drifts from the base when the app dev
  changes the type or adds a rule. "Choose rules" today means "fork the variable."
- **No governance and no roles.** `extends` is symmetric and unrestricted: a
  child can override anything, including catalog schemas, context schemas,
  qualifier definitions, and variable types. For tenants that is backwards. The
  app developer must be able to lock the shape while opening only entries, rules,
  and toggles. There is no notion of "this is tenant-editable, this is frozen."
- **No structured, per-concept algebra.** Merge is purely path-based. There is
  nowhere to express "catalog = base entries union tenant entries minus disabled"
  or "a granted tenant's `[resolve]` block replaces the base's for this variable."
- **No collision protection or namespacing.** A tenant file at the same path
  clobbers the base, nothing flags an accidental id collision, and there is no
  tenant namespace.

So `extends` stays the foundation (git-backed, source-grammar-based,
deterministic, fingerprinted, cycle-safe), but tenants need a governed,
entry-and-rule-granular, role-aware composition layered on top of it.

## The conceptual foundation: composition as an algebra

"How do two packages merge" is best framed as an algebra: a merge operator over
configuration values, and the laws that operator obeys. Two structures do the
work.

### Specificity, meet, and the two special corners

Order configurations by **specificity**: `a <= b` means "a is more constrained,
pins down more, carries more information than b." Going down adds constraints;
going up forgets them.

A **merge is a meet**: combine two descriptions by piling their requirements
together. If they are compatible, the result is more specific than either. Two
named corners fall out:

- **Top**: "no requirements, anything allowed." It is the identity for merge:
  combining with an empty overlay changes nothing.
- **Bottom**: "contradiction, nothing satisfies this." It is where colliding
  requirements land. Bottom is the merge conflict, expressed as a value in the
  algebra rather than a special-cased error.

This is the whole intuition. Agreement narrows toward a pinned value.
Contradiction falls to bottom. Governance is then a single rule: **a legal
overlay's merge with the base must move down (more specific), never up (more
permissive), and never to bottom (contradiction).**

### Order sometimes matters, sometimes must not

Two different merge flavors show up, and keeping them separate is the key move:

- **Stack requirements** (commutative, associative, idempotent). Order and
  grouping do not matter. This makes layer flattening sound and lets us
  precompute and cache `base merge tenant` and fold in more layers later. Right
  for the contract: types and schemas.
- **Ordered override** (order-sensitive by design). "Tenant's default beats the
  base's default" is deliberately not commutative: whoever is on top wins. Right
  for values, rules, and defaults.

rototo needs both, applied to different parts of a package.

### Deletion needs an explicit marker

In a merge world where combining only ever adds, you cannot delete by omission.
Leaving something out does not remove it, because the base still asserts it, and
merging only piles on. Deletion requires an explicit **tombstone**: a positive
marker that says "this is removed," survives further merges, and wins. This is
exactly why `extends` cannot disable a base catalog entry today.

## The design: composition is three operations, not one

The core realization is that a tenant overlay composes with a base through three
distinct operations, each with its own law and its own prior art.

| What composes | How it combines | Order matters | Prior art | rototo mechanism |
| --- | --- | --- | --- | --- |
| **Shape**: variable types, catalog and context schemas | narrow only (meet / unification) | no | CUE | overlay can only narrow; contradiction is an illegal-override lint failure |
| **Values**: resolution | atomic replacement of the `[resolve]` block, gated by `override` | yes, by design | Nix `mkForce`, VS Code setting scopes | topmost granted layer's `[resolve]` wins whole; no key-level merge |
| **Membership**: which catalog entries are active | union plus explicit tombstones, keyed by id | disable beats presence | OverlayFS whiteouts, Kustomize `$patch: delete` | `active = (base union tenant) minus disabled` |

That table is the design. "What does merge mean here" resolves into: narrow the
contract, override the values, union-minus-tombstone the sets.

### Cross-cutting concerns

- **Governance** is the layering contract owned by each layer: which operations a
  layer below may perform on which targets. The binary locked/open intuition
  generalizes to per-operation `allow`/`deny` grants with a narrowing ceiling; the
  dedicated "Governance: the layering contract" section below specifies it.
  Precedent runs from IAM permission boundaries to Nix priorities to Salesforce
  per-component editability to VS Code per-setting scope.
- **Namespacing** is a mandatory prefix on tenant-authored entries and
  qualifiers, so a publisher and many tenants cannot collide. Salesforce managed
  packages show this is structural, not a nicety.
- **Layering** is an ordered scope chain (VS Code settings), with base then
  tenant as the first two rungs and room for team or user rungs below. `layer.rs`
  already tracks ordered, fingerprinted layers, so the skeleton exists.
- **Evolution** is an Avro-style taxonomy of safe versus breaking base changes,
  enforced by a Schema-Registry-style CI gate at authoring time, backed by ref
  pinning and last-known-good refresh at runtime.
- **Context dimension**: tenant identity rides the same rail as `env.now`. A
  resolve-for-tenant call injects `env.tenant`, captured once per resolution, so
  the base can also write cross-tenant rules keyed on tenant id. The authored
  overlay is the other half of tenant identity.

## Governance: the layering contract

`extends` lets a layer change anything, which is exactly wrong for tenants: the
app ships a contract and a tenant should only move within it. Governance is the
missing piece that makes layering safe. It is the set of rules that say what a
layer below may change, and it is the reason a tenant overlay cannot quietly
rewrite a catalog schema or a variable's type.

### The plane: a dial that only turns down

One picture carries the whole model. Governance is a dial on every capability,
and each layer down the stack can only turn it further down, never back up.

- **Default-closed.** Absent any rule, a layer below may do nothing. Capability
  exists only where a layer above deliberately opened it. The root of the stack
  (rototo, then the app) starts fully closed.
- **Narrowing only.** A layer may grant a capability only up to what it inherited,
  and may revoke below that: `allow <= inherited ceiling`. This one clamp is what
  guarantees a tenant can never exceed the app, and a team can never exceed the
  tenant.

This is the same meet-and-narrowing lattice the rest of this document uses,
applied to permissions. The prior art is IAM permission boundaries and SCPs (an
upper bound that lower grants cannot exceed) and the Unix umask (a mask that only
clears bits). Governance is that mask over rototo's operations.

### The operations, and which are binary

Governance controls five operations, and the split that matters is which have a
scope and which do not.

| Operation | What it controls | On disk | Scope |
| --- | --- | --- | --- |
| `add` | create a new entry or member | a new `<id>.toml` | none (binary) |
| `update` | change fields of an existing entry | `<id>.patch.toml` | entries, fields |
| `delete` | remove a base entry or member | `<id>.tombstone.toml` | entries |
| `constrain` | tighten a shape | schema `allOf` | none (binary) |
| `override` | replace a variable's `[resolve]` block | overlay `variables/<id>.toml` | none (binary) |

`add` mints an id that does not exist yet, so there is no target to scope. Any
"which id may be created" rule is a naming lint on the new id (for example,
brand-authored ids must match `<brand>-*`), not a permission. `constrain` and
`override` are likewise all-or-nothing. Only `update` and `delete` act on existing
entries, so only they carry a scope. The governance verbs `update` and `delete`
name the same acts as the on-disk `.patch.toml` and `.tombstone.toml` mechanisms.

### The file: a gate plus scoped policies

Governance is one `governance.toml` at the package root, with one block per
governed entity, keyed by the entity (`catalog.<id>`, `enum.<id>`,
`variable.<id>`). Each block is a **gate** naming which operations are on, plus,
for the scoped operations, a small **policy** table saying where they apply.

```toml
[catalog.offers]
allowed_operations = ["add", "update", "delete"]   # "constrain" absent = the shape is locked
denied_operations  = []

[catalog.offers.update_policy]
allowed_fields = ["representations"]   # re-word creative; every other field stays closed
denied_entries = ["welcome-hero"]      # ...and never on the fallback

[catalog.offers.delete_policy]
allowed_entries = ["*"]                # drop any offer from this storefront...
denied_entries  = ["welcome-hero"]     # ...except the fallback
```

- The **gate** (`allowed_operations` / `denied_operations`) is the whole story for
  the binary operations: `add`, `constrain`, and `override` are fully expressed by
  being present or absent.
- A **policy** (`update_policy`, `delete_policy`) exists only for a scoped
  operation, and only to name where it applies, through `allowed_entries`,
  `denied_entries`, `allowed_fields`, and `denied_fields`. List items are literal
  ids or `*` globs.

There is no directory mirroring `model/`; the entity blocks self-locate by id.
Dropping the earlier per-node grid also drops its two failure modes: the gate and
the policy no longer restate the same allow in two places, and there is no
cell-versus-column-versus-class precedence ladder to reason about.

### How a policy resolves

A policy table is four lists, and it resolves with no precedence to hold in your
head:

- An **allowlist** (`allowed_*`), when present, restricts to what it names; when
  absent, everything is in scope. An empty allowlist is a lint error, because "I
  listed nothing" reads two ways.
- A **denylist** (`denied_*`) subtracts. Deny wins, absolutely, glob or literal.
- `update` allows a change only when the field passes the field lists and the entry
  passes the entry lists.
- A scoped operation gated on but given no policy applies to everything; lint warns
  when `update` has no `allowed_fields`, because that silently includes fields added
  to the schema later.

The one thing a single table cannot express is a lone cell (freeze `entry.field`
while leaving that entry's other fields and that field on other entries open),
because a denylist removes a whole entry or a whole column. That case is rare, and
it drops to an `[[...]]` array-of-rules escape hatch for that one policy, where a
literal can out-rank a glob. Everything else stays a flat table.

### The plane at work: two moves, one ceiling

Everything is default-closed: an operation absent from `allowed_operations`, and a
target no allowlist reaches, is denied. That makes governance two authoring moves,
which are the single `allow <= inherited ceiling` invariant seen from both sides:

- **Open what you introduce.** `allowed_operations` and the allowlists grant
  capability on your own surface, up to what the layer above left open to you.
- **Revoke from what you inherited.** `denied_operations` turns off an inherited
  operation in one line, and denylists carve specific entries or fields out of it.

Across layers the effective permission only narrows: a lower layer intersects the
allowlists and unions the denylists it inherited, so a brand can tighten what the
app granted but never exceed it. In the worked example, the brand overlay revokes
`delete` for its own teams and restricts `update` to its own namespaced entries,
both strictly inside the base grant.

### The lint that keeps it honest

- A policy for an operation not in `allowed_operations` is dead: error.
- A gated scoped operation whose policy grants nothing: warn.
- A scope on a binary operation (`add`, `constrain`, `override`): error.
- An empty allowlist: error. A denylist that cancels everything an allowlist added:
  warn.
- Entry and field names are validated against the model. A field glob matching no
  field is an error, since fields are a fixed set; an entry glob matching nothing is
  a warning, since it may be there for entries a layer adds later.
- An overlay grant that exceeds the inherited ceiling is rejected at compose time,
  not silently clamped, so the author sees it and either drops the rule or asks the
  layer above to widen.

### What `override` grants: the whole `[resolve]` block

`override` is atomic replacement. A tenant granted `override` on a variable ships
their own complete `[resolve]` block in `variables/<id>.toml`, and at compose time
it replaces the base's block entirely. There is no key-level merge, no rule
prepending, and no fallback into the base resolution: either the tenant's block
runs or the base's does. The base keeps everything outside `[resolve]`: the
variable's id, `type`, and description. A tenant file that touches anything
outside `[resolve]` on a base variable is a lint error.

Replacement is what makes override uniform across resolution methods:

- **Base is `rules`**: the tenant's rules are the rules. No interleaving with base
  rule order, so tenants stay decoupled from the base's internal rule structure.
- **Base is `query`**: the base pipeline is simply not consulted for this tenant.
  A tenant who wants query behavior writes their own `method = "query"` block; it
  runs over the composed catalog (base entries plus their adds, minus their
  tombstones), so it composes naturally with their catalog grants.
- **Base is `allocation`**: replacing `[resolve]` opts the tenant out of the
  experiment cleanly, with no half-state where some contexts are shadowed and
  others still enroll. A tenant may point at an allocation their own layer
  defines.

What lint still holds fixed:

- The declared `type` is the base's. Every literal in the replacement's rules and
  assigns, and every entry its query can return, must match it.
- The replacement may reference only what is visible in the composed package:
  composed catalogs, the tenant's own layers and namespaced additions, `context`,
  and `env`.

Two consequences are accepted deliberately. First, the grant is honest:
`allowed_operations = ["override"]` means "this tenant owns this variable's
resolution outcome, wholesale," with no pretense of partial control. Do not grant
`override` on a variable you are actively experimenting on unless tenant opt-out
is acceptable. Second, there is no built-in "tenant rules first, base as
fallback": a tenant wanting base behavior plus one exception restates the base
resolution in their block. That duplication is explicit and locally readable,
and it spares the engine from defining stacking semantics per method. Trace
provenance is one field, the layer whose `[resolve]` block produced the value,
not a rule-stack walk.

## Design decisions taken

- **Tenant scope**: entries plus rules plus qualifiers. Tenants add or toggle
  catalog entries, choose rules and defaults for overridable variables, and
  define their own namespaced qualifiers. Tenants do not add brand-new variables,
  because the app only reads variables it ships.
- **Tenant identity**: authored overlay plus context dimension. Each tenant is a
  thin overlay package that extends the base, and `env.tenant` is a first-class
  resolve-time facet so the base can write cross-tenant rules too.
- **Resolution override**: whole-block replacement, not rule merge. A tenant
  granted `override` ships a complete `[resolve]` block that replaces the base's
  atomically; the base keeps the type and identity. Earlier drafts explored
  `locked`/`prepend`/`replace` rule-merge modes; replacement subsumes them with
  one rule that works identically for `rules`, `query`, and `allocation` bases
  and keeps tenants fully decoupled from the base's internal rule ordering. The
  cost, restating the base resolution to change one case, is accepted as
  explicit, readable duplication.

## Prior art to study (minimum path)

Each item maps to a specific rototo decision. Ordered so the reading builds on
itself.

1. **Order theory (posets and lattices)** and **CRDTs**. The math under the merge:
   specificity as a partial order, meet as merge, top and bottom, and tombstones
   for order-independent deletion.
2. **CUE**. Unification as meet: types and values are the same thing, combined
   with `&`, and an overlay can only narrow. The model for the contract layer.
   Borrow the model, not the syntax. Dagger shipped CUE as its end-user language
   and walked away over the learning curve, so put CUE-style thinking where
   experts author (the base contract), never in front of tenants.
3. **Nix modules**. Per-field typed merge plus an explicit priority lattice
   (`mkDefault`, `mkForce`, `mkOverride`) and ordering (`mkBefore`, `mkAfter`).
   The model for the values layer. `mkForce` is the closest analog to rototo's
   whole-block `[resolve]` replacement: the highest-priority definition wins
   outright rather than merging.
4. **Kustomize** and **OverlayFS whiteouts**. Deletion done two ways: explicit
   delete directives and filesystem whiteouts, both keyed by identity not
   position. OverlayFS is essentially `layer.rs` at the kernel level and already
   solved the disable gap.
5. **Salesforce managed packages** and **VS Code settings layering**. The
   publisher-ships, tenant-customizes model proven as a product, and layering
   generalized to an ordered chain of scopes with per-item override boundaries.
   Salesforce shows namespacing and per-component editability are load-bearing.
6. **Protobuf and Avro compatibility, Confluent Schema Registry**. The taxonomy
   of safe versus breaking schema changes, and enforcement as a compatibility
   gate. The model for evolving the base without breaking tenants.

Who actually uses CUE, for calibration: KubeVela, Timoni, Grafana (Thema), Holos,
hof. Adoption is concentrated in Kubernetes and platform engineering, where
experts author constrained schemas. The one flagship that put CUE in front of
every end user, Dagger, discontinued it. The signal: CUE's ideas are excellent
where experts own the contract, and its learning curve is the ceiling when
everyone has to write it.

## What `layer.rs` and rototo must grow

1. **Tombstones**: a whiteout mechanism so an overlay can disable a base entry.
   Impossible today.
2. **Per-concept structured merge**: replace whole-file overwrite with narrow the
   contract, swap the `[resolve]` block, union entries.
3. **A governance file** (`governance.toml`): `allow`/`deny` grants over
   (target, operation) with a narrowing ceiling, per the Governance section.
4. **Namespacing** for tenant-authored components.
5. **Two-layer lint**: tenant entries validate against the base schema, tenant
   rules reference only known qualifiers and match base-declared types, and edits
   touch only `open` items.
6. **A compatibility gate**: base evolution checked against existing overlays.
7. **`env.tenant` and resolve-for-tenant** in the SDK and CLI, plus layer
   provenance in the resolution trace (base default versus base rule versus tenant
   rule versus tenant entry).

## Smallest first checkpoint

Consistent with rototo's "smallest working slice" principle, one thin diagonal
through the stack delivers the original ask:

> catalog tombstones (item 1) plus a per-variable `override` grant with
> `[resolve]` replacement (items 2 and 3) plus `env.tenant` (item 7).

That alone lets a tenant add entries, enable or disable entries, and choose
rules, while namespacing, the compatibility gate, and multi-level scopes each
have a known place to grow into later.

## Custom lint and the execution boundary

rototo's package format spans four languages: TOML to author definitions and
values, JSON Schema to validate structured values, CEL to evaluate conditions at
resolution time, and Lua to author custom lint rules. These are role-separated
(author, validate, evaluate, extend), not redundant, so "converging" them does
not mean collapsing them into one language. The right convergence is a single
internal semantic model that the surfaces project into, plus fewer seams between
them. We are not designing a new formal-algebra authoring format: the algebra
belongs in the engine (how packages compose, narrow, and detect conflicts), not
in a language anyone types. Boring surfaces, rigorous engine.

Custom lint is where the language count is worth actively shrinking, and it turns
out to hinge on a security invariant rather than on taste.

### The invariant

Confirmed in the current code: `Package::load` defaults to `LintMode::Deny`,
which runs `compile_runtime_after_lint`, which runs the full lint pipeline,
including custom rules through `src/lint/custom/runner.rs` into `src/lua_lint.rs`.
So loading a package executes its Lua today, on any source. Loading a remote
`git+https://` package you do not control runs its (vendored, but arbitrary) Lua.
That is arbitrary code execution reached by the most innocent call in the SDK.

The invariant we want instead:

> Loading or resolving a package never executes package-supplied code. Only
> author-time gates (pre-push and CI) do.

This composes with a second constraint that exists anyway: the team and hosted
console must never shell out to author-supplied or tenant-supplied scripts, since
that would be remote code execution in a multi-tenant server holding per-user
GitHub tokens. Both follow from putting arbitrary execution only in trusted,
single-user contexts (a developer's pre-push, CI), never at load, never in the
server.

### Candidate direction (deferred): drop the embedded Lua engine, split custom lint into two tiers

Status: the Lua environment stays for now; whether to drop it is an open call to
be made later. The load-time execution invariant above still needs addressing on
its own (for example, by not running package-supplied Lua at `Package::load` for
untrusted sources), independent of the engine's long-term fate. The two-tier
split below is the leading candidate if and when the engine is retired.

**Tier 1: CEL constraints over the semantic model.** A declarative rule (`id`,
`title`, `help`, `target`, a CEL body, a `message`) evaluated against the
projection already produced by `src/lint/semantic_model.rs`. Sandboxed,
deterministic, no external toolchain. Runs everywhere (load, resolve, console,
CI) and ships inside the distributable package. This is anything a consumer's
correctness depends on.

**Tier 2: external script escape hatch.** An arbitrary script that reads the
projection JSON on stdin and writes diagnostics JSON on stdout. Runs only at
author-time, is excluded from the distributable archive, and never runs at load,
resolve, or in the console. This is org policy that consumers need not, and
cannot, re-verify.

The line between the tiers is exact: **Tier 1 is a guarantee the package makes
(part of the runtime contract); Tier 2 is publish-time policy the author enforces
before shipping.** That is also the rule for deciding where a given check
belongs.

### Escape-hatch protocol (a public contract once it is stdin/stdout)

- **Input:** a versioned JSON projection on stdin. `semantic_model.rs` already
  produces it internally; externalizing it means it now needs a stability and
  version guarantee.
- **Output:** structured JSON diagnostics on stdout (`rule`, `message`,
  `location`, `severity`) so they flow into the diagnostics catalog and `--json`.
  stderr carries the linter's own logs.
- **Exit code:** `0` means the linter ran (diagnostics in stdout are "found
  issues"); nonzero means the linter itself failed. CI must tell those apart.
- **Rule identity:** the script declares its `<authority>/<rule-id>`s in its
  manifest entry, preserving the stable diagnostics identity model. `rototo` stays
  reserved for built-ins.
- **Invocation:** the manifest declares the command or interpreter plus the
  declared rules. The cost is that CI and dev environments must provide the
  runtime (python, node, and so on), which is acceptable because it is
  author-time only.

### Sharp edges to own

- **Determinism.** Scripts can be non-deterministic or touch the network, and
  purity cannot be enforced. Mitigate with a timeout and documentation that
  scripts must be pure functions of stdin. Non-determinism is the author's
  problem, stated plainly.
- **Toolchain provisioning.** Vendored Lua needed nothing; scripts need their
  runtime present. Fine for orgs with CI, mild friction otherwise.
- **The moderately complex middle.** Lua covered rules too complex for CEL but
  not worth a subprocess. CEL-only plus scripts removes that middle. This is the
  only genuine risk and it is checkable: inventory the existing Lua rules
  (`examples/basic/lint/*.lua` and `tests/fixtures/packages/lint-failures`) and
  classify each as CEL-expressible (expected: most, via field checks, thresholds,
  existence, and `exists`/`all` macros) or genuinely needing a script (rare). If
  the bulk map to CEL cleanly, the middle gap is theoretical and the engine can be
  dropped.

### Vocabulary

Tier 2 is not really a "lint rule": it ships nowhere and runs at a different
lifecycle. It deserves a distinct concept name from the Tier 1 `lint/` rules (for
example an author-time **check**), so the manifest and CLI keep the "ships and
runs at load" versus "author-time gate only" distinction visible. Avoid a generic
noun; pick a precise one consistent with rototo's vocabulary discipline.

## Identifier and file conventions

Two conventions keep TOML and CEL clean.

- **Snake_case for every rototo-recognized identifier.** Anything rototo resolves
  by name surfaces as a bare segment in a TOML dotted header (`[variable.loyalty_band]`)
  and in a CEL path (`variables.loyalty_band`, `entry.plan_tier`), and a hyphen in
  CEL is the minus operator. So catalog, enum, variable, evaluation-context, layer,
  experiment, and bucket ids, catalog entry ids, arm names, and schema field and
  property names are all snake_case. This lets expressions use dotted access
  (`variables.loyalty_band`, `enum.support_tiers`, `experiment.cta_copy.arm`) instead
  of bracket-quoting, and removes the `entry.plan-tier`-as-subtraction hazard.
- **Data values keep their domain casing.** Enum members are the resolved values and
  often mirror an external domain (`"US"`, `"usd"`, `"premium"`); they are always
  quoted string literals, never bare identifiers, so they are not snaked. A catalog
  entry has a snake id (rototo's handle) distinct from its field values (data).
  Context field names are rototo identifiers and snake by default, with one boundary
  exception: when a context mirrors an external payload verbatim (for example Adobe
  XDM's `customerTier`), it keeps the source's casing at that edge.
- **No indentation in TOML.** Nested `[[table.subtable]]` headers already convey
  structure; leading whitespace is cosmetic and omitted.

Slash-namespaced tenant ids (`acme/in_trial`) cannot be bare identifiers, so they keep
bracket access. Package and directory names are not resolved-by-name in CEL, so they
are not bound by this rule.

## Open questions and parked items

- **Field-level entry edits.** Resolved: the `patch` operation (`<id>.patch.toml`,
  JSON Merge Patch, validated after merge) lets a layer change specific fields of a
  provided entry, and `patch` is a governable operation with cell-level targets.
  What stays deferred is the smallest-checkpoint ordering: whether patch ships in
  the first slice or after add plus enable/disable.
- **Multi-level tenancy.** base then tenant is the first two rungs. team and user
  rungs are a natural next ask and should be a longer scope chain, not a rewrite.
  The governance model already covers arbitrary depth: each rung inherits the rung
  above as its ceiling and may only narrow, so "overridable down to which scope"
  falls out of `allow <= inherited ceiling` without new mechanism.
- **Rule identity.** Rules are positional today (`Vec<RuntimeRule>`, no ids).
  Whole-block replacement needs no rule ids. Any future "replace a specific base
  rule" would need Kustomize-style merge keys first.
- **Tombstone versus base deletion.** If a tenant disables `free` and a later base
  release deletes `free`, the tombstone points at an empty grave (harmless). A
  tenant rule that references `free` is the real break. The compatibility gate is
  what tells those apart.
- **Lua engine's future (explicitly deferred).** The Lua environment stays for
  now; the drop-versus-keep call comes later. Before any removal: inventory
  existing Lua rules to confirm the CEL/script split, version the externalized
  `semantic_model.rs` projection as a public contract, and pick the precise name
  for the Tier 2 author-time check concept. The load-time execution invariant is
  a separate, nearer-term fix.
- **Nested trace provenance.** Qualifier dissolution (landed) removed the
  per-qualifier traces a variable's resolution trace used to carry; a trace no
  longer explains why a referenced condition variable was true, only that the
  rule matched. T6 (trace provenance) should add referenced-variable traces so
  a resolution stays debuggable through the whole reference chain.
- **Variable visibility marker.** With qualifiers dissolved into variables and
  `variables[...]` exposing every variable to expressions, decide the intent or
  visibility attribute (app-facing versus internal, or a `condition` role) and
  whether referenceability is gated on it, so the cross-variable dependency graph
  stays disciplined rather than fully connected.
- **Resolution methods are settled** (`rules`, `query`, `allocation`; see the
  resolution section). The remaining open detail is the one-hop reference
  dereference built-in for queries (the `matches_audience(entry.audiences)`
  shape): its name, signature, and the rule that it follows a catalog reference
  exactly one hop and evaluates an expression-typed field, nothing deeper.
