# Rototo Concepts

Rototo is built around one tension: behavioral configuration should get the engineering rigor we give code, while still being as easy to change as data. To bring in the rigor, Rototo organizes everything into a **configuration package** that moves through a lifecycle a lot like code does. The idea is to gather all the pieces of a configuration, lay them out in an opinionated folder structure, and release them together as one unit. Inside a package, Rototo gives you a few core building blocks you can use to model and validate a wide range of configuration.

Here's the one-line version of each concept:

- **Package**: the git-versioned boundary that gets released as a unit.
- **Variable**: the named value an application asks for. It can be a plain type (`string`, `int`, and so on) or a structured value from a `catalog`.
- **Rule**: a conditional value for a variable. Each rule says "when this condition holds, use this value."
- **Condition variable**: a bool variable that gives a runtime condition a name, so many other variables can reuse it.
- **Catalog**: a named set of allowed values a variable can pick from. Handy for objects that follow a schema (LLM parameters, say).
- **Enum**: a named closed set of scalar values. It answers "is this value one of the allowed ones?" when a full catalog would be too heavy.
- **Context**: the runtime facts the application hands in.
- **Schema**: validation for package structure, context, catalog entries, or selected values.
- **Lint**: the check that a package is structurally and semantically ready to release.


## Rototo Package

Let's build a package we'll use for the rest of this page. If you haven't installed the `rototo` CLI yet, start there:

```sh
cargo install rototo
```

Now create a package called `app-config`. This is where all the configuration your application needs will live.

```sh
rototo init app-config
```

That gives you a folder that looks like this:

```sh
$> tree app-config
app-config
├── rototo-package.toml
├── variables
├── model
│   ├── catalogs
│   └── context
├── data
│   └── catalogs
└── lint

7 directories, 1 file
```

Those folders hold Rototo's building blocks - we'll get to each one. The split between `model` and `data` is the split between contracts and values: `model` holds the schemas and declarations that say what values must look like, and `data` holds the values themselves. The `variables` folder holds the values your application asks for, and `lint` holds your own custom checks. The `rototo-package.toml` file is the package-level file, and right now it just says:

```toml
schema_version = 1
```

That file is what marks `app-config` as the root of a Rototo package, and `schema_version = 1` tells Rototo which format it's reading. You don't have to think about this file beyond making sure it exists - either empty or with the `schema_version` key.

## Variable

A variable is the named value your application code asks for at runtime. The app asks for `checkout_timeout`, `llm_model`, or `enable_new_onboarding`, and Rototo figures out which value to hand back for the current context.

A quick word on the names themselves: every id Rototo recognizes (variables, enums, catalogs, catalog entries, evaluation contexts, samples) is lowercase snake_case, with `/` allowed for namespacing. That's not just taste. Ids appear in expressions, where a hyphen is the minus operator, so `checkout-timeout` would parse as a subtraction while `variables.checkout_timeout` just works. Lint enforces the convention as an error (`rototo/id-not-snake-case`).

A variable can be backed by a plain type - `bool`, `int`, `number`, `string`, or `list`. It can also pull a value from a catalog when the configuration is a structured object you want to reuse and validate as a named entry.

Here's a variable for the checkout timeout:

```toml
schema_version = 1
type = "int"

[resolve]
default = 2000
```

Right now it always resolves to 2000. That's already worth something: the value is named, typed, kept in the package, and reviewed outside the application binary.

We can check that it resolves the way we expect:

```sh
rototo resolve app-config \
  --variable checkout_timeout
```

The next step is to let the value depend on runtime context. Variables do that with rules. Each rule says: when these conditions match, use this value instead of the default. You can write those conditions inline, or point at a condition variable when the same condition needs to be reused across several variables.

## Rule

A rule is how a variable picks a value for a specific situation. From the application's side, the variable still has one name and one contract - but the package can say that some contexts should get a different value than the default.

Rules exist to keep conditional configuration out of your application code. Instead of writing branching logic like "enterprise accounts get a bigger limit" inside the service, the service passes the account facts as context and asks Rototo for the variable. The package owns the condition and the chosen value.

A variable starts with a default, and rules override that default when their conditions match:

```toml
schema_version = 1
type = "int"

[resolve]
default = 2000

[[resolve.rule]]
when = 'context.account.plan == "enterprise"'
value = 5000
```

When the application resolves this variable, Rototo checks the rules in order. The first one that matches wins. If none match, you get the default.

```sh
rototo resolve app-config \
  --variable checkout_timeout \
  --context account.plan=enterprise
```

That resolves to `5000`. With a different context - or no matching context - it's back to `2000`.

Rules can sit right next to the variable when the condition is local to that one decision. But once the same condition starts showing up in several variables, give it a name as a condition variable and have the rules point at that. It keeps the package easier to review: a reader can see that several variables lean on the same runtime condition, instead of re-reading the same predicate every time.

## Condition Variable

A condition variable is a named runtime condition. It looks at facts from the application context and answers whether that condition is true for the current resolution.

Condition variables exist because configuration decisions often share the same conditions. If several variables need to know whether an account is on an enterprise plan, that condition deserves one name and one definition - so the package can review and change it in one place instead of repeating the same predicate across many rules.

There's no separate entity for this: a named condition is just a bool variable, shaped by convention. It has `type = "bool"`, a default of `false`, and a rule that sets it to `true` when the condition holds. Here's one called `enterprise_account`:

```toml
schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'context.account.plan == "enterprise"'
value = true
```

Now another variable's rule can read that condition by name, through the `variables` root:

```toml
schema_version = 1
type = "int"

[resolve]
default = 2000

[[resolve.rule]]
when = 'variables["enterprise_account"]'
value = 5000
```

When Rototo resolves the variable, it resolves `enterprise_account` against the same context the application passed in, lazily and at most once per resolution.

```sh
rototo resolve app-config \
  --variable checkout_timeout \
  --context account.plan=enterprise
```

The rule matches because `enterprise_account` comes out true, so the selected value is `5000`.

Condition variables can also build on other condition variables, so the package can compose named conditions out of smaller named conditions while keeping the rules readable. Reference cycles are the one thing Rototo refuses: lint flags them (`rototo/variable-reference-cycle`) and resolution rejects them. The useful line to hold: condition variables describe *when* a configuration choice applies, and the variables that reference them describe *what* value the application gets.

(If you knew an earlier Rototo: qualifiers were a separate entity for exactly this job. They were dissolved into condition variables, so one concept, the variable, now covers both.)

## The expression language

The strings in `when` (and the `query` form used for catalog-backed variables) aren't some bespoke Rototo syntax. They're a subset of [CEL](https://cel.dev), the Common Expression Language. CEL is a small, well-specified, side-effect-free language built for exactly this job: evaluating a boolean (or a value) against a structured input, safely and predictably. Reusing it means the syntax is already documented and stable, and the evaluation holds no surprises - no loops, no assignment, no I/O.

Rototo evaluates these expressions and adds two things on top of plain CEL. First, four input roots are always in scope. `context` is the runtime facts the application passes in. `entry` is the catalog entry under consideration in a `query`. `variables` reads another variable's resolved value - `variables["enterprise_account"]` is how a rule leans on a condition variable; the referenced variable resolves lazily and is memoized for the rest of that resolution. And `env` is everything Rototo itself provides - kept separate so that what the application supplies (`context`) stays visibly distinct from what the control plane supplies. Today `env` has one member you can use in rules: `env.now`, the evaluation timestamp, an RFC3339 string Rototo captures once per resolution. Second, a set of named functions that configuration conditions keep reaching for - things like `startsWith`, `matches`, `semver`, `cidr`, `bucket`, and the `timeBefore`/`timeBetween` family. So a `when` expression is ordinary CEL - `==`, `&&`, `in`, `has()`, indexing, comparisons - against those roots, plus those functions.

`env.now` reads the wall clock, so a condition that depends on it resolves differently as time passes. That's exactly right for a launch window meant to open on its own, but it does mean the same package version is no longer a pure function of the context you pass. When you need a resolution you can reproduce - in a test, a `diff`, or an audit - pass the evaluation time in `context` and compare against that path instead, so the timestamp is an input you control rather than the ambient clock.

Rototo deliberately sticks to a subset. The schema-aware lint looks at how each `context` path is used and confirms an evaluation context declares it with a matching type - so a condition that compares a string field as a number, or reads a field no context provides, gets caught before release instead of at runtime. Paths used as an IP (`cidr`) or a timestamp (the `time*` functions) have to be declared with the matching JSON Schema format, because Rototo checks those formats on the values too.

## Catalog

Plain variables are plenty for a timeout, a feature flag, or a string. But some configuration needs to be a structured object. An LLM configuration, for instance, isn't just a model name - it might include the model, gateway, prompt, token budget, and temperature, and those fields should be reviewed and validated together.

A catalog is a named set of allowed structured values. Each entry has a name, and each entry has to match the catalog schema. A catalog-backed variable then selects one of those entries by name.

For example, here's a catalog schema for LLM parameters:

```json
{
  "type": "object",
  "required": ["model", "gateway", "max_output_tokens", "temperature"],
  "properties": {
    "model": { "type": "string" },
    "gateway": { "type": "string" },
    "max_output_tokens": { "type": "integer", "minimum": 1 },
    "temperature": { "type": "number", "minimum": 0, "maximum": 2 }
  },
  "additionalProperties": false
}
```

Save that as:

```sh
model/catalogs/llm_parameters.schema.json
```

The schema is a contract, so it lives under `model`. The entries are values, so they live under `data`, in a folder named after the catalog:

```sh
data/catalogs/llm_parameters/standard.toml
data/catalogs/llm_parameters/enterprise.toml
```

A `standard` entry might look like this:

```toml
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = 2400
temperature = 0.3
```

And an `enterprise` entry like this:

```toml
model = "gpt-5"
gateway = "openai"
max_output_tokens = 5000
temperature = 0.2
```

Now a variable can select from that catalog:

```toml
schema_version = 1
type = "catalog:llm_parameters"

[resolve]
default = "standard"

[[resolve.rule]]
when = 'variables["enterprise_account"]'
value = "enterprise"
```

From the application's point of view, this still behaves like any other variable. The app asks for the named variable, passes context, and gets the selected value back. The difference is that the value is a validated catalog entry, not a primitive literal.

This keeps structured configuration from getting scattered across a bunch of unrelated variables. When several fields have to change together, a catalog gives that combination a name and lets lint catch missing fields, wrong field types, and references to entries that don't exist.

## Catalog Query

Sometimes the application doesn't want one catalog entry - it wants a filtered list of them. A dropdown is the classic case: the package might define every supported LLM parameter set, but the app should only show the ones that are currently enabled.

Catalog queries handle that. A variable can resolve to `list<catalog:...>` and use a query to pick the matching entries.

First, add an `enabled` field to the `llm_parameters` catalog schema:

```json
{
  "type": "object",
  "required": ["enabled", "label", "model", "gateway", "max_output_tokens", "temperature"],
  "properties": {
    "enabled": { "type": "boolean" },
    "label": { "type": "string" },
    "model": { "type": "string" },
    "gateway": { "type": "string" },
    "max_output_tokens": { "type": "integer", "minimum": 1 },
    "temperature": { "type": "number", "minimum": 0, "maximum": 2 }
  },
  "additionalProperties": false
}
```

Then entries can say whether they're selectable:

```toml
enabled = true
label = "Fast model"
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = 2400
temperature = 0.3
```

Now define a variable that returns the enabled entries:

```toml
schema_version = 1
type = "list<catalog:llm_parameters>"

[resolve]
default = []

[[resolve.rule]]
query = "entry.enabled == true"
```

When the application resolves this variable, Rototo runs the query against each catalog entry and returns every entry that matches as part of the list.

That gives the application a reviewed, validated set of dropdown options without hardcoding the choices in the UI. Rototo owns which entries exist and which are enabled; the application owns how to render the list it gets back.

## Enum

Catalogs handle structured objects. But plenty of values are just one scalar from a short, closed list: a plan tier is `free`, `team`, or `business`, and nothing else. Declaring that as a plain `string` leaves the door open for a typo like `"buisness"` to ship. Building a catalog for it is overkill: there's no object, just a name.

An enum answers the question "is this value one of the allowed ones?" It follows the same contract/values split as a catalog. The declaration under `model/enums/plan_tiers.toml` says what kind of scalar the members are:

```toml
schema_version = 1
type = "string"
```

And the members under `data/enums/plan_tiers.toml` say which values exist:

```toml
members = ["free", "team", "business"]
```

A variable uses it with `type = "enum:plan_tiers"` (or `list<enum:plan_tiers>` for a list). From then on, every default and rule value in that variable has to be a member, and lint fails the package on anything else. A misspelled member is unreleasable, which is exactly what you want.

Enums also show up inside schemas. A catalog schema or an evaluation context schema can pin a field to an enum with `"x-rototo-ref": "enum:plan_tiers"`, so catalog entries and sample contexts get the same member check. The [package format](./package-format.md) page covers that annotation in full.

## Context

Context is the runtime data the application hands to Rototo when it asks for a variable. The package holds the configuration, but the application is the one that knows the facts about the current request, user, account, device, cart, or environment. Context is how those facts get into the resolution.

For example, this CLI input:

```sh
rototo resolve app-config \
  --variable checkout_timeout \
  --context account.plan=enterprise
```

is the same as resolving with this JSON context:

```json
{
  "account": {
    "plan": "enterprise"
  }
}
```

Rules read that context through `context.<path>` expressions:

```toml
when = 'context.account.plan == "enterprise"'
```

Context should have a contract. Without one, package authors can accidentally write rules against fields the application never sends, or compare a field as a string when the app actually sends a number. Rototo handles that with evaluation context schemas.

Create a schema at:

```
model/context/request.schema.json
```

For the examples above, it might start like this:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "account": {
      "type": "object",
      "additionalProperties": true,
      "properties": {
        "plan": { "type": "string" },
        "seats": { "type": "integer" }
      }
    },
    "user": {
      "type": "object",
      "additionalProperties": true,
      "properties": {
        "id": { "type": "string" },
        "tier": { "type": "string" }
      }
    }
  }
}
```

You can also keep sample contexts beside the schema:

```
model/context/request-samples/enterprise.json
```

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 250
  },
  "user": {
    "id": "user-123",
    "tier": "premium"
  }
}
```

Those samples are handy for local resolution, linting, review, and documentation. They make the runtime assumptions visible in the package instead of leaving them buried in application code.

Context isn't configuration. It's the input used to *choose* configuration. The package owns the rules, condition variables, schemas, catalog entries, and variable values; the application owns the runtime facts it passes into resolution.

## Schema

Schemas are the foundation and the first line of defense in how Rototo validates a package. A package can hold a lot of files, but the values that matter still need contracts - whether that's an evaluation context or a catalog entry.

We've already used two kinds of schema.

The first is the evaluation context schema:

```text
model/context/request.schema.json
```

This describes the runtime facts the application may pass into resolution. When a rule reads `context.account.plan`, the schema is where that path is declared and typed. That lets Rototo catch package mistakes before release - like a rule depending on `context.account.tier` when the app only ever sends `context.account.plan`.

The second is the catalog schema:

```
model/catalogs/llm_parameters.schema.json
```

This describes every entry in the `llm_parameters` catalog. If the schema says `max_output_tokens` must be an integer and `temperature` must sit between 0 and 2, every entry has to satisfy that contract.

For example, this entry is valid:

```toml
enabled = true
label = "Fast model"
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = 2400
temperature = 0.3
```

And this one should fail validation:

```toml
enabled = true
label = "Broken model"
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = "many"
temperature = 9
```

That failure matters because it happens while the package is being reviewed - not after the application has already loaded the configuration in production.

Schemas aren't the runtime API. Variables are still what applications resolve. Schemas sit behind variables, catalogs, and context to make sure the package is safe to release. Which brings us to lint: the package-level check that applies these contracts and tells you whether the whole package holds together.

## Lint

Lint is the release gate for a package. It checks whether the files are valid on their own, and whether they make sense together as one releasable unit.

Run it before you treat a package change as ready:

```sh
rototo lint app-config
```

Built-in lint covers the Rototo model: the package manifest, variables, rules, catalogs, catalog entries, evaluation context schemas, and the references between them. It catches things like a variable selecting a catalog entry that doesn't exist, a rule referencing a variable that doesn't exist, a cycle between variables that reference each other, a catalog entry that fails its schema, or a rule reading a context path no evaluation context schema declares.

Rototo also supports custom lint for the policy that belongs to *your* package. Built-in lint validates Rototo semantics, but it can't know your operational rules. A team might decide, say, that enabled LLM parameter sets have to use a conservative temperature.

Custom lint lives under the package's `lint/` folder as Lua files:

```lua
function register(lint)
  lint:rule({
    id = "ai/llm-temperature-limit",
    title = "Enabled LLM temperature is too high",
    help = "Keep enabled LLM parameter sets at or below temperature 1.0.",
    target = "/catalogs/llm_parameters/entries",
    handler = "check_temperature",
  })
end


function check_temperature(package, entry)
  if entry.value.enabled == true and entry.value.temperature > 1.0 then
    return {
      {
        message = "enabled LLM parameter set must use temperature <= 1.0",
        path = "/value/temperature",
      }
    }
  end
  return {}
end
```

The custom rule id uses an authority your package or team owns, like `ai/...`. Rototo's built-in diagnostics use the reserved `rototo/...` authority.

For automation, lint can emit JSON:

```sh
rototo lint app-config --json
```

Lint is where the package model comes together. Variables define what applications ask for, rules and condition variables define when values apply, catalogs hold reusable structured values, context schemas define runtime inputs, and lint checks that all of it forms a coherent package before release.

## Putting It Together

A Rototo package is the unit that gets reviewed and released. Inside it, variables define the values applications ask for, rules choose values for runtime situations, condition variables give shared conditions a name, catalogs hold structured reusable values, context carries runtime facts from the application, schemas define the contracts, and lint checks that the whole thing is releasable.

At runtime, the application doesn't read individual TOML or JSON files. It loads a package source and resolves named variables with context:

```sh
rototo resolve app-config \
  --variable checkout_timeout \
  --context account.plan=enterprise
```

The same model works when the package comes from git instead of a local folder. The source changes, but the boundary stays the same: the application loads a reviewed package and asks Rototo for typed configuration values.

That's the core Rototo model. Configuration stays data, so it can be reviewed, validated, and released on its own schedule, apart from the application binary. But it still follows engineering discipline: clear ownership, explicit contracts, reproducible package state, and checks before release.
