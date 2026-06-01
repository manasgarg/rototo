# How to Select a Value for a Runtime Condition

Use this when a named runtime condition should select a different value in an
environment, such as enterprise accounts, one country, one tenant class, or a
stable bucketed rollout.

The operational goal is to keep the condition definition in the workspace
instead of branching in application code.

## Expected outcome

After this change:

- The runtime condition has a named qualifier.
- The variable has a value for that condition.
- The target environment has a rule that selects that value.
- Matching and non-matching contexts resolve to different value keys.

## Define the condition as a qualifier

Create a qualifier file with a product-readable name:

```text
qualifiers/enterprise-accounts.toml
```

```toml
schema_version = 1

description = "Accounts on the enterprise plan with at least 100 seats"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"

[[predicate]]
attribute = "account.seats"
op = "gte"
value = 100
```

The qualifier id is `enterprise-accounts`. It turns raw context fields into a
named condition that can be reviewed and reused.

## Add the rollout value

Add the target value to the variable. For a primitive value:

```toml
[values]
standard = 1000
enterprise = 2000
```

For larger structured values, put the value in an external value file such as:

```text
variables/llm-agent-config-values/enterprise.toml
```

The value key is the file stem, so this file defines `enterprise`.

## Add a rule to the environment

Add a rule under the environment where the condition should select the new
value:

```toml
[env.prod]
value = "standard"

[[env.prod.rule]]
description = "Enterprise accounts get the larger value"
qualifier = "enterprise-accounts"
value = "enterprise"
```

Rules are evaluated before the environment default. If the qualifier matches,
rototo selects `enterprise`. If it does not match, rototo selects `standard`.

## Verify matching and non-matching context

Lint the workspace:

```sh
rototo lint config/
```

Resolve with matching context:

```sh
rototo resolve config/ --variable max-output-tokens \
  --env prod \
  --context '{"account":{"plan":"enterprise","seats":250}}'
```

Resolve with non-matching context:

```sh
rototo resolve config/ --variable max-output-tokens \
  --env prod \
  --context '{"account":{"plan":"team","seats":25}}'
```

The two commands should return different `value_key` values.

## Common mistakes

Do not duplicate the same predicate in many variables. Define a qualifier once
and reuse it from rules.

Do not skip the non-matching test. Rollout mistakes often come from defaults,
not only from the matching branch.

Do not rely on missing context fields. A qualifier requires every context path it
reads, and a context schema makes the application/config boundary explicit before
runtime.

## Related docs

- `qualifier-reference` specifies qualifier files.
- `predicate-reference` specifies predicate operators.
- `variable-reference` specifies environment rules.
- `context-reference` explains context validation.
