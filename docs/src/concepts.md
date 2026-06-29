# Rototo Concepts

The key aspect of Rototo's design is to deal with the dual nature of behavioral configuration: it should follow engineering rigor of code while expressing configuration as data.
To bring in the engineering rigor, Rototo builds around the notion of cofiguration packages that can follow a lifecycle that's similar to code.
The whole idea is to pull together all the assets that make up a configuration, organize them in an opionionated directory structure and release them as a unit.
Within a package, Rototo provides core primitives that can be used for modeling and validating a large variety of configuration representations.

Here is a one line overview of key concepts:
- Package: the Git-versioned configuration boundary that's released as a unit.
- Variable: the named value an application asks for. It can be regular data types (`string`, `int` etc) or a composite value from a `catalog`.
- Rule: a conditional value selection for a variable. Each rule specifies conditions and the value that gets selected when condition is met.
- Qualifier: a reusable named condition that can be used in multiple variables.
- Catalog: a named set of allowed values that variables can select from. Useful to organize objects that adhere to a schema (e.g. LLM parameters).
- Context: runtime facts supplied by the application.
- Schema: validation for package structure, context, catalog, or selected values.
- Lint: validation that the package is structurally and semantically releasable.


## Rototo Package

Let's first create a Rototo package that would be used throughout this document. Begin with installing the `rototo` cli if you haven't done that yet.

```sh
cargo install rototo
```

Now, we can create a Rototo package called `app-config`. This is where we would store all the configuration needed by our application.

```sh
rototo init app-config
```

This would create a dir structure that looks like the following:
```sh
$> tree app-config
app-config
├── rototo-package.toml
├── evaluation-contexts
├── qualifiers
├── variables
├── catalogs
├── lint
6 directories, 1 file
```

In this package, `evaluation-contexts`, `qualifiers`, `variables`, `catalogs` and `lint` are Rototo's primitives that we would cover later. The file `rototo-package.toml` is the package level file with the following contents:

```toml
schema_version = 1
```

The presence of `rototo-package.toml` indicates that `app-config` is the root dir of a Rototo package. And the `schema_version = 1` indicates the current schema for this package. We don't have to worry about this file beyond the fact that it must exist as either blank or with `schema_version` key.

## Variable

A variable is the named configuration value that application code resolves at runtime. The application asks for `checkout-
timeout`, `llm-model`, or `enable-new-onboarding`, and Rototo decides which value should be returned for the current context.

A variable can be backed by a primitive type such as `bool`, `int`, `number`, `string`, or `list`.
It can also select a value from a catalog when the configuration is a structured object that should
be reused and validated as a named entry.

For example, a variable might define the timeout used by checkout:

```toml
schema_version = 1
type = "int"

[resolve]
default = 2000
```

At this point the variable always resolves to 2000. That is already useful: the value is named,
typed, stored in the package, and reviewed outside the application binary.

We can ensure that the variable resolves per our expectation:

```sh
rototo resolve app-config \
  --variable checkout-timeout
```

The next step is to make the value depend on runtime context. Variables do that through rules. Each
rule says: when these conditions match, use this value instead of the default. Those conditions can
be written directly, or they can refer to qualifiers when the same condition needs to be reused
across multiple variables.

## Rule

A rule is how a variable chooses a value for a specific runtime situation. The variable still has
one name and one contract from the application's point of view, but the package can say that some
contexts should receive a different value than the default.

Rules exist to keep conditional configuration out of application code. Instead of writing branching
logic like “enterprise accounts get a larger limit” inside the service, the service passes account
facts as context and asks Rototo for the variable. The package owns the condition and the selected
value.

A variable starts with a default value and the rules can override that default when their conditions match:

```toml
schema_version = 1
type = "int"

[resolve]
default = 2000

[[resolve.rule]]
when = 'context.account.plan == "enterprise"'
value = 5000
```

When the application resolves this variable, Rototo checks the rules in order. The first matching
rule wins. If no rule matches, Rototo returns the default.

```sh
rototo resolve app-config \
  --variable checkout-timeout \
  --context account.plan=enterprise
```

That resolution returns `5000`. With a different context, or with no matching context, it returns
`2000`.

Rules can stay close to the variable when the condition is local to that one decision. When the same
condition starts appearing in several variables, give it a name as a qualifier and reference that
qualifier from the rules. That keeps the package easier to review: readers can see that several
variables depend on the same runtime condition instead of reinterpreting the same predicate each
time.

## Qualifier

A qualifier is a named runtime condition. It evaluates facts from the application context and
returns whether that condition is true for the current resolution.

Qualifiers exist because configuration decisions often share the same conditions. If several
variables need to know whether an account is on an enterprise plan, that condition should have one
name and one definition. The package can then review and change the condition in one place instead
of repeating the same predicate across many variable rules.

For example, create a qualifier called `enterprise-account`:

```toml
schema_version = 1

when = 'context.account.plan == "enterprise"'
```

A variable rule can then reference that qualifier:

```toml
schema_version = 1
type = "int"

[resolve]
default = 2000

[[resolve.rule]]
when = 'env.qualifier["enterprise-account"]'
value = 5000
```

When Rototo resolves the variable, it evaluates the qualifier against the same context passed by the
application.

```sh
rototo resolve app-config \
  --variable checkout-timeout \
  --context account.plan=enterprise
```

The rule matches because `enterprise-account` evaluates to `true`, so the selected value is `5000`.

Qualifiers can also compose with other qualifiers. That lets the package build named conditions from
smaller named conditions, while still keeping variable rules readable. The useful boundary is that
qualifiers describe when a configuration choice should apply, while variables describe what value
the application receives.

## The expression language

The strings in `when` (and the `query` form used for catalog-backed variables) are not a bespoke
Rototo syntax. They are a subset of [CEL](https://cel.dev), the Common Expression Language. CEL is a
small, well-specified, side-effect-free expression language designed exactly for this job: evaluating
a boolean (or a value) against a structured input, safely and predictably. Reusing it means the
syntax is already documented and stable, and the evaluation has no surprises — no loops, no
assignment, no I/O.

Rototo evaluates these expressions and adds two things on top of plain CEL. First, three input
variables are always in scope. `context` is the runtime facts the application passes in. `entry` is
the catalog entry under consideration in a `query`. `env` is everything Rototo itself provides, so
that what the application supplies (`context`) stays visibly separate from what the control plane
supplies. Today `env` has two members: `env.qualifier["enterprise-account"]` reads another qualifier,
and `env.now` is the evaluation timestamp, an RFC3339 string Rototo captures once per resolution.
Second, a set of named functions that configuration conditions keep needing — for example
`startsWith`, `matches`, `semver`, `cidr`, `bucket`, and the `timeBefore`/`timeBetween` family. So a
`when` expression is ordinary CEL — `==`, `&&`, `in`, `has()`, indexing, comparisons — against those
variables, plus those functions.

`env.now` reads the wall clock, so a condition gated on it resolves differently as time passes. That
is the right behavior for a launch window that should open on its own, but it means the same package
version is no longer a pure function of the context you pass. When you need a resolution you can
reproduce — in a test, a `diff`, or an audit — pass the evaluation time in `context` and compare
against that path instead, so the timestamp is an input you control rather than the ambient clock.

Rototo deliberately keeps to a subset. The schema-aware lint checks how each `context` path is used
and confirms an evaluation context declares it with a matching type, so a condition that compares a
string field as a number, or reads a field no context provides, is caught before release rather than
at runtime. Paths used as an IP (`cidr`) or a timestamp (the `time*` functions) must be declared with
the matching JSON Schema format, because Rototo asserts those formats on the values too.

## Catalog

Primitive variables are enough for values like a timeout, a feature flag, or a string. But some configuration needs to be represented as a structured object. An LLM configuration, for example, is not just a model name. It may include the model, gateway, prompt, token budget, and temperature. Those fields should be reviewed together and validated together.

A catalog is a named set of allowed structured values. Each entry in the catalog has a name, and each entry must match the catalog schema. A catalog-backed variable then selects one of those entries by name.

For example, define a catalog schema for LLM parameters:

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
catalogs/llm-parameters.schema.json
```

Then add catalog entries under a matching entries directory:

```sh
catalogs/llm-parameters-entries/standard.toml
catalogs/llm-parameters-entries/enterprise.toml
```

A `standard` entry might look like this:

```toml
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = 2400
temperature = 0.3
```

And an `enterprise` entry might look like this:

```toml
model = "gpt-5"
gateway = "openai"
max_output_tokens = 5000
temperature = 0.2
```

A variable can now select from that catalog:

```toml
schema_version = 1
type = "catalog:llm-parameters"

[resolve]
default = "standard"

[[resolve.rule]]
when = 'env.qualifier["enterprise-account"]'
value = "enterprise"
```

The variable still behaves like any other variable from the application's point of view. The
application asks for the named variable, passes runtime context, and receives the selected value.
The difference is that the selected value is a validated catalog entry, not a primitive literal.

This keeps structured configuration from being scattered across multiple unrelated variables. If
several fields must change together, a catalog gives that combination a name and lets lint catch
missing fields, invalid field types, and references to entries that do not exist.

## Catalog Query

Sometimes the application does not need one catalog entry. It needs a filtered list of entries. A
common example is a dropdown: the package may define every supported LLM parameter set, but the
application should only show the ones that are currently enabled.

Catalog queries handle that case. A variable can resolve to `list<catalog:...>` and use a query to
select matching catalog entries.

First, add an `enabled` field to the `llm-parameters` catalog schema:

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

Then catalog entries can decide whether they are selectable:

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
type = "list<catalog:llm-parameters>"

[resolve]
default = []

[[resolve.rule]]
query = "entry.enabled == true"
```

When the application resolves this variable, Rototo evaluates the query against each catalog entry.
Every matching entry is returned as part of the resolved list.

That gives the application a reviewed, validated set of dropdown options without hardcoding the
allowed choices in the UI. Rototo owns which entries exist and which ones are enabled; the
application owns how to render the returned list.

## Context

Context is the runtime data the application gives to Rototo when it asks for a variable. The Rototo package
contains the configuration, but the application still knows the facts about the current request,
user, account, device, cart, or environment. Context is how those facts are injected into resolution process.

For example, this CLI input:

```sh
rototo variable resolve checkout-timeout \
  --package app-config \
  --context account.plan=enterprise
```

is equivalent to resolving with this JSON context:

```json
{
  "account": {
    "plan": "enterprise"
  }
}
```

Rules and qualifiers read that context through `context.<path>` expressions:

```toml
when = 'context.account.plan == "enterprise"'
```

Context should have a contract. Without one, package authors can accidentally write rules against
fields the application never sends, or compare a field as a string when the application sends a
number. Rototo handles that with evaluation context schemas.

Create a schema at:

```
evaluation-contexts/request.schema.json
```

For the examples above, the schema might start like this:

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
evaluation-contexts/request-samples/enterprise.json
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

Those samples are useful for local resolution, linting, review, and documentation. They make the
runtime assumptions visible in the package instead of leaving them implicit in application code.

Context is not configuration. It is the input used to choose configuration. The package owns the
rules, qualifiers, schemas, catalog entries, and variable values; the application owns the runtime
facts it passes into resolution.

## Schema

Schemas are the foundation and first line of defence in Rototo's validation strategy. A package
can contain many files, but the important values still need contracts be it evaluation contexts or catalog entries.

We have already used two kinds of schemas.

The first is the evaluation context schema:

```text
evaluation-contexts/request.schema.json
```

This schema describes the runtime facts the application may pass into resolution. When a qualifier
reads context.account.plan, the schema is where that path is declared and typed. That lets Rototo
catch package mistakes before release, such as a qualifier depending on context.account.tier when
the application only sends context.account.plan.

The second is the catalog schema:

```
catalogs/llm-parameters.schema.json
```

This schema describes every entry in the llm-parameters catalog. If the schema says
max_output_tokens must be an integer and temperature must be between 0 and 2, every catalog
entry has to satisfy that contract.

For example, this entry is valid:

```toml
enabled = true
label = "Fast model"
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = 2400
temperature = 0.3
```

This entry should fail validation:

```toml
enabled = true
label = "Broken model"
model = "gpt-5-mini"
gateway = "openai"
max_output_tokens = "many"
temperature = 9
```

That failure matters because it happens while the package is being reviewed, not after the
application has loaded the configuration in production.

Schemas are not the runtime API. Variables are still what applications resolve. Schemas sit behind
variables, catalogs, and context to make sure the package is safe to release. The next step is lint:
the package-level check that applies these contracts and reports whether the whole package is
structurally and semantically valid.

## Lint

Lint is the package release gate. It checks whether the package files are valid on their own and
whether they make sense together as one releasable unit.

Run lint before treating a package change as ready:

```sh
rototo lint app-config
```

Built-in lint covers the Rototo model: the package manifest, variables, rules, qualifiers, catalogs,
catalog entries, evaluation context schemas, and the references between them. It can catch mistakes
such as a variable selecting a catalog entry that does not exist, a rule referencing an unknown
qualifier, a catalog entry failing its schema, or a qualifier reading a context path that is not
declared by an evaluation context schema.

Rototo also supports custom lint for policy that belongs to your package. Built-in lint can validate
Rototo semantics, but it cannot know your operational rules. For example, a team may decide that
enabled LLM parameter sets must use a conservative temperature.

Custom lint lives under the package's lint/ directory as Lua files:

```lua
function register(lint)
  lint:rule({
    id = "ai/llm-temperature-limit",
    title = "Enabled LLM temperature is too high",
    help = "Keep enabled LLM parameter sets at or below temperature 1.0.",
    target = "/catalogs/llm-parameters/entries",
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

The custom rule id uses an authority owned by the package or team, such as `ai/....` Rototo's
built-in diagnostics use the reserved `rototo/...` authority.

For automation, lint can emit JSON:

```sh
rototo lint app-config --json
```

Lint is where the package model comes together. Variables define what applications ask for, rules
and qualifiers define when values apply, catalogs define reusable structured values, context schemas
define runtime inputs, and lint checks that those pieces form a coherent package before release.

## Putting It Together

A Rototo package is the unit that gets reviewed and released. Inside that package, variables define
the values applications ask for, rules choose values for runtime situations, qualifiers give shared
conditions a name, catalogs hold structured reusable values, context carries runtime facts from the
application, schemas define the contracts, and lint checks that the whole package is releasable.

At runtime, the application does not read individual TOML or JSON files. It loads a package source
and resolves named variables with context:

```sh
rototo resolve app-config \
  --variable checkout-timeout \
  --context account.plan=enterprise
```

The same model works when the package comes from Git instead of a local directory. The source changes,
but the boundary stays the same: the application loads a reviewed package and asks Rototo for typed
configuration values.

That is the core Rototo model. Configuration remains data, so it can be reviewed, validated, and
released independently from the application binary. But it still follows engineering discipline:
clear ownership, explicit contracts, reproducible package state, and checks before release.
