# The Expression Language

A few places in a package ask a question about the runtime: a variable rule's
`when` ("does this rule apply?"), a catalog query's `filter` ("does this
entry belong in the result?"), a layer's `unit` ("what value do we hash for
this request?"), and an allocation's `eligibility` ("is this unit enrolled?").
All of them are written in the same little expression language, and this page
is the whole language in one sitting.

If you've ever written a CEL expression, this will feel familiar - it *is* a
subset of CEL under the hood. But you don't need to know CEL to read on. The
expressions look a lot like a condition in any programming language: comparisons,
`&&` and `||`, a few handy functions.

```toml
when = '(context.user.tier == "premium")'
```

That's a real rule condition. It reads "the user's tier is premium," and when
that's true, the rule matches. Let's unpack what an expression can actually
reach.

## The five things an expression can read

An expression can only look at five roots. That's it - five names, and
everything hangs off them. Keeping the list short is what makes expressions easy
to reason about and easy for lint to check.

### `context` - the facts your app passed in

`context` is the bundle of request-time facts your application hands to rototo:
who the user is, where they're coming from, what's in their cart. You reach into
it with dots:

```toml
when = '(context.request.country in ["DE","FR","ES","IT","NL","SE"])'
```

The shape of `context` isn't a free-for-all - it's pinned down by your
[evaluation-context schema](./package-format.md). If you read
`context.user.tier` but the schema never declared it, lint tells you, so a typo
here doesn't quietly become "always false" in production.

### `entry` - the catalog entry in front of you

`entry` only shows up inside a catalog query's `filter` or `sort` (more on
those below). When you're filtering a catalog, `entry` is the one entry
currently being looked at, and you read its fields the same dotted way:

```toml
filter = "entry.enabled == true"
```

Outside of a query, `entry` doesn't exist - there's no entry to talk about.

### `variables` - other variables' resolved values

`variables` reads the resolved value of *another* variable, by its id. Write it
with a dot (`variables.premium_users`) or with brackets
(`variables["premium_users"]`). Ids are snake_case, so the dot form always
works for a plain id; brackets are what you need for a namespaced id like
`variables["payments/retry_limit"]`, since `/` can't appear in a dot path.

This is how a named condition gets reused. Define the condition once as a bool
variable (a "condition variable": `type = "bool"`, default `false`, a rule that
sets it `true`), and every other rule can lean on it by name:

```toml
when = '(variables["premium_users"]) && (variables["beta_rollout_bucket"])'
```

The referenced variable resolves lazily, against the same context, and the
result is memoized for the rest of that one resolution - so ten rules reading
`variables["premium_users"]` cost one evaluation, and they all see the same
answer. A chain of variables referencing each other is fine; a *cycle* is not.
Lint catches cycles at edit time (`rototo/variable-reference-cycle`), and
resolution refuses them too.

### `enums` - a named set of legal values

`enums` reads the member list of an enum you declared under `enums/<id>.toml`.
Write it with a dot (`enums.plan_tiers`) or with brackets for a namespaced id
(`enums["geo/regions"]`). The value is the plain member list, so the natural
use is a membership test:

```toml
when = 'context.account.plan_tier in enums.plan_tiers'
```

The point is naming the set instead of restating it. Without this, every rule
that cares about "a real plan tier" carries its own copy of
`["free", "team", "business"]`, and adding a tier means hunting them all down.
With `enums.plan_tiers`, the enum file is the one place the set lives, and the
rules follow it.

Because the list is an ordinary value, the collection tools work on it too:
`size(enums.plan_tiers)` or a `.exists(...)` comprehension are fine. Referencing
an enum the package doesn't declare is a lint error
(`rototo/expression-unknown-enum`), and lint also checks that the context path
you're testing has a type the enum's members could actually match.

### `env` - what rototo provides

`env` is the stuff rototo fills in for you. It has a small, fixed set of members:

- **`env.now`** - the current time, as an RFC3339 string. It's captured once at
  the start of a resolution, so every mention of `env.now` in that one
  resolution sees the exact same instant. No risk of two checks disagreeing
  because a millisecond ticked over.

- **`env.resolving.variable`** - the id of the variable being resolved right
  now. This one is special: it *only* works inside a `[[trace]]` policy in the
  manifest. A rule can't read it, and that's deliberate - a condition has to be
  a function of the request, not of who happens to be asking.

  ```toml
  [[trace]]
  when = 'env.resolving.variable == "checkout_redesign" && context.user.id == "tester-123"'
  ```

### What you can't read

Anything outside those five roots is rejected at lint time. Two cases come up
most:

- The retired qualifier spellings, `qualifier["id"]` and `env.qualifier["id"]`.
  Qualifiers were dissolved into condition variables - write
  `variables["id"]` instead, and lint's error points you there.
- Made-up `env` members like `env.region` or a typo like `env.noww`. If it isn't
  one of the members above, lint stops you.

This is a feature, not a nuisance: catching a bad reference while you're editing
beats discovering it when a rule silently never matches.

## The operators you'd expect

Comparisons, logic, and membership all work the way you'd guess:

| What you want | Write it |
| --- | --- |
| Equal / not equal | `==`, `!=` |
| Ordering | `<`, `<=`, `>`, `>=` |
| And / or / not | `&&`, `\|\|`, `!` |
| Is it in a list? | `context.region in ["us","eu"]` |
| Is this item in an array field? | `"admin" in context.user.roles` |
| Does a field exist? | `has(context.user.tier)` |

You can index with dots (`context.user.tier`) or with brackets when a key has
funny characters (`context["account.plan"]`).

A few things are *not* in the language, because the CEL subset leaves them out:
no loops, no comprehensions, no assigning to things, no defining your own
functions. Expressions are meant to *ask a question*, not run a program. And
because lint knows your context schema, it'll also catch type mismatches - like
comparing a string field against a number.

## Built-in functions

On top of the operators, there's a set of functions for the comparisons that
come up over and over. Several have both a camelCase and a snake_case spelling
(and sometimes a short alias) - pick whichever reads best to you; they do the
same thing.

| You want to check… | Functions |
| --- | --- |
| Text starts with something | `startsWith` / `starts_with` / `prefix` |
| Text ends with something | `endsWith` / `ends_with` / `suffix` |
| Text (or a list) contains something | `contains` |
| Text matches a pattern | `matches` / `regex`, or `glob` for glob-style |
| Version comparison | `semver` |
| A deterministic rollout bucket | `bucket` (see below) |
| An IP is in a range | `cidr` / `inCidr` / `in_cidr` |
| A value is present / absent | `present` / `missing` |
| Reach a nested path, or get a size | `path`, `size` |
| Time comparisons | `timeAfter`, `timeBefore`, `timeBetween`, `timeAtOrAfter`, `timeAtOrBefore` (and their snake_case forms) |

The time functions pair naturally with `env.now` when you want a condition
that's true only after a date, or only within a window.

## Buckets: gradual rollouts that stay put

The one function worth its own section is `bucket`, because it's how you ship
something to "10% of users" and have that 10% stay the same 10% from one request
to the next.

```toml
when = '(bucket(context.user.id, "checkout_redesign_2026_05", 0, 1000))'
```

You call it `bucket(value, salt, start, end)`. Here's the idea:

- It takes your `value` (usually a stable id like the user id) and a `salt`
  string, hashes them together, and lands on a number from **0 to 9999** - think
  of it as which of 10,000 slots this value falls into.
- It returns true when that number is in the range `[start, end)` - start
  included, end excluded.

So `0, 1000` is "the first 1,000 slots out of 10,000" - 10%. The hashing is
deterministic and side-effect-free: the same user id and salt always land in the
same slot, so a user who's in the rollout *stays* in it, and the same user
doesn't flip in and out between requests.

The `salt` is what lets you run independent rollouts. Change the salt and you get
a fresh, unrelated 10% - so two different features rolling out to "10%" don't
hit the exact same users.

`bucket` is the inline form, good for a one-off condition inside a single
rule. When several variables need to read one shared assignment - an
experiment driving the layout, the copy, and the CTA together - that's a layer
and `method = "allocation"`, covered below and on the
[concepts page](./concepts.md).

## Queries: picking catalog entries with an expression

Expressions also show up in a catalog query - a variable whose
`[resolve]` block declares `method = "query"` and reads its value straight out
of a catalog's entries. Which entries match is often a fact the entries
already carry, so instead of hardcoding the answer in the variable, the query's
`filter` describes it:

```toml
[resolve]
method = "query"
from = "notifications"
filter = 'entry.channel == context.channel && entry.active == true && variables["premium_users"]'
```

rototo runs the `filter` once per entry in the `from` catalog. For each entry,
`entry` is that entry, and `context`, `variables`, and `env` are the same as
everywhere else. If the whole thing comes out true, the entry stays.

A simpler one:

```toml
filter = "entry.enabled == true"
```

That's "keep every enabled entry." A query can also order the survivors with a
`sort` expression - evaluated once per entry, it produces the sort key rather
than a true/false answer - and trim the result with `order` and `limit`. The
exact field list, and the single-entry form where the top sorted entry wins,
live in the [package format](./package-format.md) reference. From the
expression language's side there are just two slots: `filter` asks a question
like a `when` does, `sort` produces a value. Both read the same roots
(`context`, `entry`, `variables[...]`, `env.now`) and use all the same
operators and functions - the only new thing is that `entry` is now in play.

## Layers: `unit` and `eligibility`

The last two slots live in a layer file - the shared bucket lines behind
`method = "allocation"` variables (the [concepts page](./concepts.md) tells
that story). A layer has a `unit` expression, and each of its allocations can
have an `eligibility` expression:

```toml
unit = "context.user.id"

[[allocation]]
id = "cta_copy_test"
eligibility = '!variables["enterprise_accounts"]'
```

`unit` produces the value that gets hashed onto the layer's buckets - usually
a stable id. It may read `context` only: no `variables`, no `entry`. That
restriction is what keeps assignment a pure function of the request, so the
same user lands on the same bucket every time.

`eligibility` asks a question like a `when` does: is this unit enrolled in the
allocation at all? It can read `context` and `variables[...]` - handy for
keeping a whole named condition's worth of users out of an experiment - but
not `entry`. There's no catalog entry in play here, so `entry` doesn't exist,
same as outside a query.

## A note on stability

The expression engine is a pinned version of the CEL implementation rototo
builds on, and that's on purpose: the exact parsing and evaluation behavior is
part of rototo's contract, not an implementation detail that drifts under you. An
expression that resolves a certain way today resolves the same way tomorrow.
