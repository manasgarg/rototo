# Lint Warning Backlog

Status: proposed
Scope: built-in lint warnings that should help authors find stale, confusing,
or unreachable workspace structure without rejecting an otherwise valid
workspace.

This file only lists warnings that are not currently implemented. The current
warning set already includes:

- `rototo/qualifier-unreferenced`
- `rototo/variable-rule-shadowed`
- `rototo/variable-value-unused`

The intent is to keep `lint` as the single source of diagnostic truth. Future
`inspect` output should show these diagnostics in context instead of
reimplementing separate warning checks.

## High-Confidence Warnings

These checks are static, deterministic, and likely actionable with a low false
positive rate.

### `rototo/qualifier-unreachable`

A qualifier is referenced by another qualifier, but no variable rule can reach
it from a live resolution path.

Current `rototo/qualifier-unreferenced` only catches qualifiers with no incoming
references. It does not catch a chain where `a` is unreferenced and `a`
references `b`; `b` is referenced, but it still cannot affect resolution.

Implementation note: build reachability from all variable rule qualifier
references, then walk qualifier-to-qualifier references. Any qualifier outside
that reachable set is unreachable. Avoid duplicate noise by deciding how this
interacts with `rototo/qualifier-unreferenced`.

### `rototo/schema-unreferenced`

A `schemas/*.json` file is present but is not referenced by `[context].schema`
or any variable `schema`.

This usually means a schema was renamed, a variable moved away from schema
validation, or an old schema was left behind.

### `rototo/workspace-context-schema-missing`

One or more qualifier predicates read runtime context attributes, but the
workspace does not declare `[context].schema`.

Rototo can currently validate a referenced context path when a context schema is
present. Without a context schema, predicate paths can drift from the
application's runtime context contract.

### `rototo/qualifier-predicate-context-type-mismatch`

A qualifier predicate compares a context attribute in a way that conflicts with
the context schema type.

Examples:

- `op = "gt"` against a schema field typed as `string`
- `op = "bucket"` against a field whose schema type cannot be bucketed
- `op = "eq"` with a TOML string value against a schema field typed as integer
- `op = "in"` where the candidate list element types conflict with the schema

This requires a conservative JSON Schema type reader. When the schema type
cannot be determined, do not warn.

### `rototo/qualifier-predicate-duplicate`

A qualifier contains the same predicate more than once.

Because predicates are ANDed, duplicate predicates do not change behavior. They
make the condition harder to read and can hide copy-paste mistakes.

### `rototo/qualifier-predicate-contradiction`

A qualifier contains predicates that cannot all be true.

Conservative examples:

- same attribute with two different `eq` values
- same attribute with `eq` and `neq` for the same value
- same attribute with disjoint `in` sets
- same attribute with numeric bounds that cannot overlap
- same bucket attribute and salt with disjoint required ranges when both
  predicates must hold

If a contradiction is proven, rules using that qualifier can never match.

### `rototo/variable-value-unreachable`

A variable value is referenced only from resolution paths that can never select
it.

Current `rototo/variable-value-unused` catches values with no references. This
warning should catch values that are referenced, but only by shadowed rules,
always-false qualifiers, undeclared environments, or other unreachable paths.

### `rototo/variable-rule-selects-default-value`

A variable rule selects the same value as the containing environment default.

The rule can still match, but it does not change the resolved value. That often
means the wrong value key was used, or an old rollout rule was left behind.

### `rototo/variable-environment-duplicates-fallback`

A declared environment block has the same default value and equivalent rule list
as `[env._]`.

This is valid, but usually adds noise without changing runtime behavior. The
check should compare references, not parsed TOML text.

### `rototo/custom-lint-rule-unregistered`

A custom rule is declared in `rototo-workspace.toml`, but no Lua registration
references it.

This is different from "the rule emitted no diagnostics." A rule can be
registered and pass cleanly. The stale condition is that no handler is wired to
the declared rule at all.

### `rototo/custom-lint-file-unregistered`

A `lint/*.lua` file loads successfully but registers no handlers.

This usually means the file contains a typo in `register(lint)`, an old script
was left behind, or the script was copied before its handlers were added.

### `rototo/custom-lint-registration-duplicate`

The same custom lint file registers the same stage, selector, rule, and handler
more than once.

Duplicate registrations can produce duplicate diagnostics and make custom lint
behavior harder to reason about.

### `rototo/custom-lint-registration-empty-target`

A custom lint registration has a valid selector, but that selector matches no
workspace entities.

Examples:

- registering value lint in a workspace with no variable values
- registering schema lint in a workspace with no schemas
- registering a JSON path selector that is absent from every current target

For JSON path selectors, only warn when the selector is absent from every target
and the target set itself is non-empty.

### `rototo/variable-external-values-orphaned`

A `variables/<id>-values/` directory exists, but `variables/<id>.toml` does not.

The current source discovery only loads external value directories for known
variables, so this stale directory can otherwise be invisible.

### `rototo/variable-external-values-empty`

A `variables/<id>-values/` directory exists for a known variable but contains no
TOML value files.

This is usually leftover structure from a variable that moved values inline or
renamed its external values.

## Useful But Needs Conservative Analysis

These are valuable, but should only be implemented when rototo can prove the
condition without noisy guesses.

### `rototo/qualifier-duplicate-definition`

Two qualifiers have equivalent predicate sets.

This can point to duplicated named conditions. It should normalize predicate
order before comparing because predicate order does not affect qualifier
evaluation.

### `rototo/variable-rule-shadowed-by-broader-qualifier`

A later variable rule can never match because an earlier rule's qualifier is a
proven superset of the later rule's qualifier.

The existing `rototo/variable-rule-shadowed` only catches repeated use of the
same qualifier. This warning would catch cases such as an earlier
`premium-users` rule shadowing a later `premium-enterprise-users` rule when the
latter composes the former.

### `rototo/variable-rule-overlapping-bucket-ranges`

Rules in the same variable environment use bucket-based qualifiers whose bucket
ranges overlap and select different values.

This is not always invalid because first-match order is defined, but it is a
common source of rollout surprises. Only warn when the overlap is statically
provable for the same bucket attribute and salt.

### `rototo/variable-rule-duplicate-outcome`

Multiple rules in the same environment select the same value through different
qualifiers.

This can be intentional, so it should be lower priority than shadowing checks.
It is useful when cleaning old rollout paths because distinct conditions no
longer produce distinct behavior.

### `rototo/variable-value-duplicate-content`

Two value keys for the same variable contain identical JSON values.

This can be an intentional alias during rollout, but often means a value key was
renamed without removing the old branch.

### `rototo/workspace-environment-unused`

An environment is declared in the manifest but no variable has an explicit block
for it.

This should be treated carefully. It is valid for variables to rely entirely on
`[env._]`; this warning is most useful when no variable in the whole workspace
customizes the environment at all.

## Likely Custom Policy, Not Built-In

These are tempting, but probably belong in custom lint unless the product model
gets stricter:

- missing descriptions on qualifiers or variables
- maximum number of rules per variable
- maximum number of predicates per qualifier
- naming conventions for ids
- requiring every declared environment to override every variable
- requiring every context schema property to be used by a qualifier
- rejecting explicit environment blocks that intentionally document parity with
  fallback behavior
