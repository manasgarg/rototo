# Modeling Runtime Configuration

Runtime configuration becomes hard to operate when the model hides where a
decision is made. The app sends a few facts, another system turns those facts
into booleans, a config file holds a few values, and six months later nobody
can explain why one account received one behavior and another account did not.

Rototo puts that decision in one place. The modeling question is not "which TOML
files do I need?" It is:

> Where should this runtime decision live?

In rototo, facts live in [context](reference-context.html). Named conditions
live in [qualifiers](reference-qualifiers.html). Selected configuration lives
in [variables](reference-variables.html). Structured payloads live in
[catalogs](reference-catalogs.html). Schemas and
[custom lint](reference-custom-lua-lint.html) protect the boundaries.
Packages and [layers](reference-package-layering.html) define who owns
which part of the control plane.

The rest of this guide is about choosing those boundaries deliberately.

## Start With The App Boundary

Start with the question the application needs to ask at runtime.

For an account limit policy, the app probably does not want to ask four
separate questions:

```text
max-projects?
max-members?
audit-retention-days?
enabled-features?
```

Those fields may all change together. They may be reviewed together. The app
may need them as one account profile. If so, the rototo variable should model
that atomic decision:

```text
account-limit-profile
```

The app now has one stable call:

:::sdk-snippet modeling-runtime-app-call
```rust
let limits = pkg
    .resolve_variable("account-limit-profile", &context)?;
```

```python
limits = pkg.resolve_variable(
    "account-limit-profile",
    context,
)
```

```typescript
const limits = pkg.resolveVariable(
  "account-limit-profile",
  context,
);
```

```java
VariableResolution limits = pkg
    .resolveVariable("account-limit-profile", context)
    .get();
```

```go
limits, err := pkg.ResolveVariable(
    ctx,
    "account-limit-profile",
    resolveContext,
    nil,
)
```
:::

The app asks for the policy it needs. Rototo selects the value. The app does
not reconstruct policy by resolving a pile of loosely related variables.

Splitting variables is still right when the app can change, test, observe, or
fail the decisions independently. What matters is that the split follows the
application boundary, not the number of fields in a payload.

## Treat The Package As An Administrative Boundary

A package is an administrative boundary, not an application deployment
boundary.

That distinction matters. A [package](reference-package-layout.html)
answers:

- who owns this configuration;
- who reviews changes;
- which schemas and lint rules apply;
- which files form one control-plane unit.

An application deployment answers a different question: which binary is
running, and which [package source](reference-package-sources.html) URI is
that binary configured to load?

Those boundaries often overlap, but they are not the same. A single package
can be loaded by multiple application deployments. A single application
deployment can load a layered package assembled from multiple administrative
owners. A package change can affect future resolutions in a running service
without redeploying the binary.

I would usually model these as stronger package boundaries:

```text
product-defaults
customer-acme-config
acme-support-team-config
payments-runtime-policy
```

And I would be more cautious with boundaries like:

```text
frontend-prod
backend-prod
service-a-config
```

Service-specific packages are not wrong. Sometimes one service really owns a
policy end to end. But the first question should be ownership and policy, not
deployment topology.

Layering makes this concrete:

```text
product-defaults
  -> customer-acme-config
      -> acme-support-team-config
```

The app may load `acme-support-team-config`. Rototo still preserves the
administrative story: product owns the schema and defaults, the customer owns
account-wide policy, and the support team owns a narrower override.

## Put Facts In Context, Policy In The Package

The runtime context should describe facts the app already knows:

```json
{
  "account": {
    "id": "acct_123",
    "plan": "enterprise",
    "seats": 120
  },
  "request": {
    "country": "DE"
  }
}
```

The context should not contain the decision rototo is supposed to make:

```json
{
  "use_enterprise_limits": true
}
```

That boolean may feel convenient, but it moves policy out of the package.
Rototo can no longer explain why enterprise limits applied. Reviewers cannot
inspect the condition. A future operator sees the selected value, but the
reason already happened somewhere else.

In rototo, the app supplies facts:

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 120
  }
}
```

The package owns the policy:

```toml
when = 'context.account.plan == "enterprise"'
```

That is the split I want. The application owns what happened in this request.
The package owns what that fact means for runtime behavior.

## Use Qualifiers To Name Operational Conditions

[Qualifiers](reference-qualifiers.html) are not just reusable conditions. They
are the vocabulary that shows up in rules, traces, tests, and debugging
conversations.

For example:

```toml
# qualifiers/enterprise-account.toml
schema_version = 1

when = 'context.account.plan == "enterprise"'
```

Now a variable rule can say what it means:

```toml
[[resolve.rule]]
when = 'qualifier["enterprise-account"]'
value = "enterprise"
```

And a trace can explain the selection in the same language:

```text
rule[0] if enterprise-account -> enterprise (matched)
```

Create a qualifier when the condition explains why behavior changes. Compose
qualifiers when the composed name carries meaning:

```toml
when = 'qualifier["enterprise-account"] == true'

when = 'context.account.seats >= 100'
```

That could be named `large-enterprise-account`.

Avoid chains where a reader has to open five files to understand one rule. A
qualifier should reduce cognitive load. If the name no longer helps explain
the decision, the model is probably too indirect.

## Choose Primitive Values Or Catalogs By Contract Shape

Primitive values are right when the selected configuration is truly one
scalar or one list:

```toml
schema_version = 1
type = "int"

[resolve]
default = 3

[[resolve.rule]]
when = 'qualifier["expanded-account"]'
value = 25
```

[Catalogs](reference-catalogs.html) are the better fit when the selected
value is a policy entry:

```text
account-limit-profile
notification-delivery-policy
inference-routing-policy
service-degradation-policy
```

For account limits, a catalog-backed variable can select one validated entry:

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

The entry can carry the whole profile:

```toml
# catalogs/account-limit-profile-entries/enterprise.toml
enabled_features = ["audit-log", "priority-support"]

[limits]
projects = 100
members = 250
monthly_requests = 1000000
```

The catalog schema validates the selected entry before the app consumes it.
That is the practical reason to use catalogs: the package can prove the
policy entry has the shape the app expects.

Without that, shape errors move back into application code. The app becomes
the first place to discover that a field is missing or a value has the wrong
type.

## Treat Defaults As The Baseline Policy

Defaults are not filler. The default value is the policy for everyone who does
not match a named condition.

In a healthy variable, the default is normal behavior and rules are exceptions:

```toml
[resolve]
default = "growth"

[[resolve.rule]]
when = 'qualifier["enterprise-account"]'
value = "enterprise"

[[resolve.rule]]
when = 'qualifier["free-account"]'
value = "starter"
```

[Rules use first-match semantics](reference-variable-resolution.html). Put
narrower or higher-priority rules before broader rules.

Two patterns are worth treating as model smells:

- a rule selects the same value as the default;
- two rules use the same qualifier.

Rototo reports both. They may not break runtime behavior, but they make policy
harder to read. A reviewer should be able to tell which condition changes the
selected value and why it wins.

## Model Buckets Deliberately

Buckets help because assignment happens inside the reviewed package, not in
application-side randomization.

A [bucket condition](reference-predicate-operators.html) looks like this:

```toml
schema_version = 1

when = 'bucket(context.account.id, "account-limit-profile-2026-06", 0, 1000)'
```

The context attribute should be stable. Account id, user id, or package id
are common choices. Request ids are usually wrong because they change every
request.

The `range` controls how much of the bucket space matches. A range of
`[0, 1000]` matches ten percent of the `0..10000` space.

The `salt` defines the assignment universe. Changing the range changes the
percentage while preserving assignments for existing buckets. Changing the
salt reshuffles assignments.

That makes salt changes operationally significant. Use them when you mean to
reshuffle, not as an incidental rename.

## Decide Which Package Owns The File

In a [layered package](package-layering.html), ownership is part of the
model.

A common shape is:

```text
product-defaults
  catalogs/account-limit-profile.schema.json
  variables/account-limit-profile.toml

customer-acme-config
  catalogs/account-limit-profile-entries/acme_default.toml
  variables/account-limit-profile.toml

acme-support-team-config
  qualifiers/support-pilot-account.toml
  catalogs/account-limit-profile-entries/support_pilot.toml
  variables/account-limit-profile.toml
```

The product layer owns the contract. The customer layer owns the customer-wide
default. The support team layer owns a narrow override.

Remember that layered replacement is file-level. If a child layer writes
`variables/account-limit-profile.toml`, it replaces the inherited file at that
path. It is not patching individual TOML fields.

I want that because ownership is visible in the diff. It also means teams should
keep variable files readable and intentional. A child layer that replaces a
variable owns the full rule order for that variable.

## Use Schemas For Shape And Lint For Judgment

Schemas and [custom lint](reference-custom-lua-lint.html) protect different
kinds of mistakes.

Use schemas for structure:

- the app must provide `account.plan` as a string;
- a catalog value must include `limits.projects`;
- a field must be an integer within a JSON Schema range;
- unknown fields should be rejected.

Use custom lint for judgment:

- production account limits must stay below an approved ceiling;
- a provider routing policy must not pair incompatible providers;
- incident banner copy must include a support link;
- production values must not point at local endpoints.

The distinction I rely on is:

> Use schemas for shape. Use custom lint for judgment.

That keeps structural contracts close to the values they validate, and keeps
local policy explicit without forcing it into JSON Schema contortions.

## Modeling Checklist

When I am deciding whether a rototo model is ready to grow, I use this
checklist:

- What variable does the app resolve?
- Is this one atomic decision, or several independent decisions?
- Which facts must the app provide as context?
- Which qualifiers explain why behavior changes?
- Is the selected value primitive or structured?
- What schema validates the app boundary or selected entry?
- Does any local policy need custom lint?
- Which package layer should own this file?

If those answers are clear, the
[production workflow](production-workflow.html) becomes much easier. The next
step is to wire the model into an application so the service loads a package
source, resolves named variables, refreshes safely, and reports what it
selected.
