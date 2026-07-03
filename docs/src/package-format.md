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
│   ├── premium-users.toml
│   └── checkout-redesign.toml
├── catalogs/                        # structured value sets + their schemas
│   ├── checkout-redesign.schema.json
│   └── checkout-redesign-entries/
│       └── control.toml
├── evaluation-contexts/             # the shape of the facts your app passes in
│   ├── request.schema.json
│   └── request-samples/
│       └── premium-enterprise.json
└── lint/                            # your own custom checks, in Lua
    └── checkout-redesign.lua
```

The one rule that ties it together: **the file name is the id**. A file at
`variables/checkout-redesign.toml` defines a variable whose id is
`checkout-redesign`. A catalog schema at `catalogs/checkout-redesign.schema.json`
defines a catalog whose id is `checkout-redesign`. You never write the id
*inside* the file - the filename already said it.

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
already self-contained, so there's nothing left to point at.)

The second is `[[trace]]`, which turns on resolution tracing for specific cases
without redeploying your app. You can have as many as you like:

```toml
[[trace]]
when = 'env.resolving.variable == "checkout-redesign" && context.user.id == "user-123"'
```

The `when` is an [expression](./expressions.md) - same language as everywhere
else. We cover what tracing is for in [Using Rototo](./adoption.md); here, just
know it's a manifest thing.

## Variables: the values your app actually reads

A **variable** is the thing your application asks for at runtime. It has a type,
a default, and an optional list of rules that override the default when some
condition holds.

The simplest kind is a plain on/off flag. Here's `user-is-admin.toml`:

```toml
schema_version = 1
description = "Whether the user should receive admin UI affordances"
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'variables["admin-users"]'
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
- `[resolve]` - required. Holds the `default` (required) and the rules.
- `[[resolve.rule]]` - zero or more. Each has a `when` condition and a `value`.

Both the default and every rule value have to match the declared `type` - rototo
checks that for you, so a `bool` variable can't accidentally default to a string.

Some old syntax is gone: a top-level `schema` field and a `[values]` section are
both rejected. Declare a `type` and put your literal values directly under
`[resolve]`.

## Condition variables: naming a runtime condition

That `variables["admin-users"]` in the rule above deserves a closer look. "Is
this a premium user?" "Is this request coming from Europe?" Conditions like
these tend to show up in more than one variable, and repeating the same
expression everywhere is how definitions drift apart.

The fix is to give the condition a name - and in rototo, a named condition is
just a bool variable. By convention we call it a **condition variable**: type
`bool`, default `false`, and a rule that flips it to `true` when the condition
holds. Here's `eu-users.toml`:

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
when = '(variables["premium-users"]) && (variables["beta-rollout-bucket"])'
value = true
```

There's nothing special about a condition variable to rototo - it resolves like
any other bool, and your app can resolve it directly if it wants the yes/no
answer itself. The convention is for readers: a bool named `eu-users` with a
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
| `list<...>` | a list of a specific item type |

The `list<...>` form lets you say what's *in* the list. The item can be a
primitive or a catalog reference - `list<string>`, `list<int>`,
`list<catalog:payment-methods>`. What you can't do is nest lists inside lists:
`list<list<string>>` is rejected. One level deep is the limit.

Here's a plain list variable, `payment-methods.toml`:

```toml
schema_version = 1
description = "Payment methods enabled at checkout"
type = "list"

[resolve]
default = ["card", "paypal"]

[[resolve.rule]]
when = 'variables["mobile-users"]'
value = ["card", "apple_pay", "google_pay"]
```

## Catalogs: values with a real shape

Sometimes a value isn't a single number or string - it's a structured object
with several fields, and you've got a few named versions of it. A checkout page
layout, say: each variant has a heading, a subheading, an image, some body copy.
That's what a **catalog** is for. It's a set of named entries, all sharing one
schema.

A catalog comes in two parts. First, the schema, at
`catalogs/<id>.schema.json` - an ordinary JSON Schema describing what every
entry must look like. Here's `checkout-redesign.schema.json`:

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

Second, the entries, each a TOML file under `catalogs/<id>-entries/`. The
filename is the entry's id. Here's `control.toml`:

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
type = "catalog:checkout-redesign"

[resolve]
default = "control"

[[resolve.rule]]
when = 'variables["premium-users"]'
value = "premium"
```

The variable resolves to an entry id like `"control"`, and rototo hands your app
the full structured entry behind it.

There's one more trick worth a mention: a `list<catalog:...>` variable can pick
its entries with a `query` instead of a hardcoded list - a small expression that
runs over each catalog entry and keeps the ones that match. That's an
[expressions](./expressions.md) topic, so we'll cover the syntax there.

## Evaluation contexts: the facts your app passes in

When your app asks rototo to resolve a variable, it passes in a bundle of facts
about the current request - who the user is, what country they're in, what's in
their cart. That bundle is the **evaluation context**, and an evaluation-context
schema pins down its shape so the package and the app can't quietly disagree
about it.

The schema lives at `evaluation-contexts/<id>.schema.json` - again, plain JSON
Schema. Here's a trimmed `request.schema.json`:

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
`evaluation-contexts/<id>-samples/`. Each is a JSON file - the filename is the
sample's id - that has to validate against the schema. Here's
`premium-enterprise.json`:

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
rules. Here's `checkout-redesign.lua`:

```lua
function register(lint)
  lint:rule({
    id = "consumer-experience/checkout-heading-required",
    title = "Checkout heading is missing",
    help = "Set heading to visible checkout copy.",
    target = "/catalogs/checkout-redesign/entries",
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

The handler returns a list of problems. Each problem just needs a `message`;
returning an empty list `{}` means "all good." The [diagnostics
reference](./diagnostics.md) covers how these show up next to the built-in ones.

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
