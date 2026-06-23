# Configuration Primitives

When an application asks rototo for a value at runtime, what is rototo actually
evaluating?

The answer is deliberately small. rototo does not ship a separate subsystem for
each configuration problem — a flag service here, an override service there, an
account-tier service somewhere else. It gives you a few primitives and one
resolution that composes them. You model a rollout, a kill switch, a
per-account exception, or a routing policy by combining the same primitives,
the way you would model a leaderboard or a queue on top of a handful of data
structures instead of a purpose-built service.

That restraint is the point. A small set of primitives that compose is easier
to review, trace, and reason about than a large set of features that each
behave a little differently. This page is the model those primitives form: what
each one is, how they compose, what the composition guarantees, and where the
model deliberately stops.

The reference pages specify each primitive exactly. This page is the shape they
share.

## One Input: The Request Context

Everything rototo evaluates is a function of one input: the facts the
application supplies at request time. rototo calls that input the
[request context](reference-context.html), and it is always a JSON object.

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 120
  },
  "request": {
    "country": "DE"
  }
}
```

The split this enforces matters more than the format. The application owns what
happened in this request — the plan, the seat count, the country. The package
owns what those facts mean for runtime behavior. A context that already carries
the decision, like `use_enterprise_limits = true`, has moved policy out of the
package and into the caller, where no reviewer can see it and no trace can
explain it.

The context is the only input the rest of the model reads. Every other
primitive either reads it, constrains it, or selects against it.

## Qualifiers Name Conditions

A raw fact is not yet a decision. The first primitive turns a fact into a named
condition: a [qualifier](reference-qualifiers.html).

```toml
# qualifiers/enterprise-account.toml
schema_version = 1

when = 'context.account.plan == "enterprise"'
```

A qualifier is a predicate over the context: facts in, one boolean out. The
`when` expression reads context paths, calls helper functions, and returns
`true` or `false`. The naming is the load-bearing part. `enterprise-account` is
the word that later shows up in variable rules, resolution traces, tests, and
the conversation when someone asks why an account got the behavior it did.

Qualifiers compose with each other. A qualifier can read another qualifier by
name, which is the first composition rule of the model:

```toml
# qualifiers/large-enterprise-account.toml
schema_version = 1

when = 'qualifier["enterprise-account"] && context.account.seats >= 100'
```

References form a graph, and rototo rejects cycles when it resolves them. The
[expression language](reference-predicate-operators.html) is shared everywhere a
condition is written: comparisons, presence checks with `has(...)`, bucket
assignment with `bucket(value, salt, start, end)`, and the boolean operators
`&&`, `||`, and `!`, which short-circuit so a guarded path is never read when it
cannot matter.

## Variables Select Values

A condition decides nothing on its own. The second primitive does the selecting:
a [variable](reference-variables.html) chooses one value.

```toml
# variables/checkout-experience.toml
schema_version = 1
type = "string"

[resolve]
default = "classic"

[[resolve.rule]]
when = 'qualifier["premium-account"]'
value = "redesign"
```

A variable is a selector: conditions in, one typed value out. It holds a default
and an ordered list of rules. Resolution walks the rules in file order, and the
first rule whose condition holds selects its value; if none match, the default
wins. The default is the baseline policy for everyone who matches no named
condition, not filler.

There is exactly one selection strategy — first match — and that is deliberate.
One well-understood rule keeps every selection explainable by reading the rules
top to bottom. Put narrower or higher-priority rules first. rototo flags the two
shapes that make a selection hard to read: a rule that selects the same value as
the default, and two rules that use the same condition. See
[variable resolution](reference-variable-resolution.html) for the exact
evaluation order.

A variable does not return a bare value. It returns the value together with the
record of how it was selected, which is what makes the model observable rather
than merely configurable.

## Catalogs And Schemas Constrain The Value

A selected value is only useful if it has the shape the application expects. Two
primitives bound what a value may be, so a shape mistake is caught in the
package instead of in application code.

A [schema](reference-context.html) is JSON Schema applied to structure — both to
the request context the app supplies and to the structured values a variable can
select. A [catalog](reference-catalogs.html) is a named set of validated
structured values that a variable selects by key:

```toml
# variables/account-limit-profile.toml
schema_version = 1
type = "catalog:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
when = 'qualifier["enterprise-account"]'
value = "enterprise"
```

Each catalog entry is a structured value validated against the catalog schema
before resolution can select it:

```toml
# catalogs/account-limit-profile-entries/enterprise.toml
enabled_features = ["audit-log", "priority-support"]

[limits]
projects = 100
members = 250
monthly_requests = 1000000
```

Use a primitive value — `bool`, `int`, `number`, `string`, or `list` — when the
configuration is truly one scalar or one list. Use a catalog when the selected
value is a structured policy entry the package should prove correct before the
application consumes it. Either way, the contract lives next to the value, and
the application is not the first place a missing field or wrong type is
discovered.

## Lint Keeps The Model Honest

Composition is only safe if the parts actually fit. The last primitive is the
guard: [lint](reference-lint-overview.html) checks that the package is
internally consistent before it can be released.

Built-in lint validates structure and references: the manifest parses, files
declare their schema version, rules reference qualifiers that exist, catalog
keys resolve, primitive values match their declared type, and structured values
validate against their schema. These diagnostics carry stable `rototo/<rule-id>`
identities.

[Custom Lua lint](reference-custom-lua-lint.html) captures the policy only your
team knows — a production limit must stay under an approved ceiling, two
providers must not be paired, a banner must include a support link — under a
non-`rototo` authority. Schemas check shape; custom lint checks judgment. The
distinction keeps structural contracts close to the values they validate and
leaves room for local policy without forcing it into JSON Schema.

Lint is what lets the model assume its own invariants at resolution time: by the
time an application loads a package, the references resolve and the values fit.

## The Shape Of The Model

Step back and the whole model is small:

- **One input** — the request context.
- **Two evaluation primitives** — qualifiers name conditions, variables select
  values.
- **Two contract primitives** — schemas constrain shape, catalogs hold sets of
  validated structured values.
- **One guard** — lint proves the package is consistent before release.

The composition rules are equally small. Qualifiers compose with qualifiers.
Rules read context and qualifiers through one expression language. Variables
select among values bounded by schemas and catalogs. Lint validates the whole
graph before an application can load it.

One operation runs the model. Given a package version and a request context,
resolution:

1. validates the context against a compatible context schema;
2. evaluates the qualifiers the target needs, following references and
   short-circuiting boolean operators;
3. walks the variable's rules in order and selects the first match, otherwise
   the default;
4. validates the selected value — its primitive type, or its catalog entry
   against the catalog schema;
5. returns the value, its source, and a trace of the rules and conditions that
   produced it.

Resolution is a pure function of the loaded package version and the supplied
context. Nothing else is read.

## What The Model Guarantees

The composition is built to make three properties hold, because they are what
make configuration safe to change without a redeploy.

**It is deterministic.** Resolution reads no ambient clock and draws no
randomness. The same package version and the same context always select the
same value. Time and identity enter only as facts the application supplies: a
bucket reads a stable account id from context, and a dated condition compares a
timestamp the caller passes in. Determinism is what makes a resolution
reproducible in a test and in production.

**It is explainable.** Resolution runs in the application process and returns a
trace: the selected value and its source, each rule's outcome in order, and the
condition that each qualifier evaluated.

```json
{
  "id": "enterprise-account",
  "when": "context.account.plan == \"enterprise\"",
  "value": true
}
```

Existing logs and observability can report what was selected, from which
package version, and why, without reimplementing any of this logic. See
[resolution output](reference-resolution-output.html) for the full shape.

**It is reviewable.** Every primitive is a file in git. Each condition, value,
schema, and policy change has an author, a diff, and a history, and moves
through the same review and tests as code.

## What Stays Outside The Model

The model selects and validates a value. It does not compute one, run a sequence
of steps, merge sources, or keep state. Those belong to the application, and
keeping them out is what keeps resolution pure and explainable.

- **Computation** — arithmetic, scoring, rating, and string templating. The
  package stores the weights and thresholds; the application does the math.
- **Orchestration** — workflows, approval chains, and escalation sequences.
  rototo configures the decisions inside a workflow; it does not run the
  workflow.
- **Merging and precedence** — deep-merge and source-precedence layering across
  configuration sources. [Package layering](reference-package-layering.html)
  replaces files at a path; it does not patch individual fields.
- **State** — counters, quotas, usage metering, and lifecycle clocks. The
  package stores the limit, not the running count.
- **Traversal beyond a single hop** — org-tree walks and multi-level
  relationship chains. A single-hop fact supplied in context is fine; iterative
  traversal is the application's.

Each line is a deliberate boundary, not a missing feature. A value you can
compute is a value you cannot review; a workflow you can run is a decision you
cannot trace. The boundary is what lets the guarantees hold.

## Where To Go Next

You now have the whole model: one input, two evaluation primitives, two
contracts, one guard, one resolution, three guarantees, and a clear edge.

The next question is judgment — where each runtime decision should live, how to
draw variable boundaries, when a catalog earns its place, and which layer owns a
file. [Modeling Runtime Configuration](modeling-runtime-configuration.html)
works through those choices. When you need the exact format of any single
primitive, the reference pages for
[context](reference-context.html), [qualifiers](reference-qualifiers.html),
[variables](reference-variables.html), and [catalogs](reference-catalogs.html)
specify it.
