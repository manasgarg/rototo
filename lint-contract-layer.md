# Design: Lint Diagnostic Contract Layer

Status: implemented
Scope: `src/diagnostics.rs`, `src/lint.rs`, `src/lua_lint.rs`,
`src/catalog.rs`, `src/model.rs`, `src/output.rs`, `src/main.rs`, lint tests,
workspace fixtures, docs, and `CLAUDE.md`.

## Problem Statement

rototo's lint diagnostics need to be a stable, enumerable contract. Before this
change, the emitted shape had two identities:

- broad `code` families, such as `rototo/workspace-toml-file-invalid`;
- precise `rule` ids, such as `rototo/variable/env/unknown-environment`.

The broad code was the field exposed by the catalog, while the precise rule was
the field users and automation actually needed. Rule ids were string literals
scattered through lint emit sites, so typos, uncatalogued rules, and divergent
test expectations were easy to introduce.

The same diagnostic also carried `source`, `kind`, and `details.title`, which
restated information that belongs in rule metadata.

## Contract

A diagnostic has one identity:

```json
{
  "rule": "rototo/variable-unknown-type",
  "severity": "error",
  "path": "variables/example.toml",
  "message": "variable declares unknown type: currency",
  "help": "Use one of bool, int, number, string, or list."
}
```

Rule ids use this form:

```text
<authority>/<rule-id>
```

The id contains exactly one slash. `rototo` is reserved for built-in rules.
Built-in rule ids are flat and must not use nested forms such as
`rototo/variable/unknown-type`.

Custom Lua lint may emit non-`rototo` authorities, for example:

```text
payments/max-token-budget
billing/invalid-plan-limit
consumer-experience/banner-too-large
```

Custom rules must be declared in reviewable TOML before Lua can emit them:

```toml
[lint]
path = "../lint/llm.lua"

[[lint.rule]]
id = "payments/max-token-budget"
title = "Token budget exceeds payments policy"
help = "Lower max_output_tokens or update the payments policy."
```

Lua returns the rule id and a concrete message:

```lua
return {
  {
    rule = "payments/max-token-budget",
    message = "enterprise.max_output_tokens exceeds 5000"
  }
}
```

Rust owns built-in rototo metadata. TOML owns custom rule metadata. Lua does not
own title or help text.

## Implementation Shape

Built-in rules are represented by `RototoRuleId`, a closed Rust enum. Each
variant has exhaustive metadata:

- serialized rule id;
- severity;
- entity;
- title;
- help.

Emitted diagnostics store `DiagnosticRule`, either:

- `DiagnosticRule::Rototo(RototoRuleId)`;
- `DiagnosticRule::Custom(CustomRuleId)`.

`CustomRuleId` is validated at runtime:

- exactly one slash;
- non-empty authority and rule id;
- lowercase ASCII letters, digits, and hyphen only;
- authority must not be `rototo`.

The global diagnostic catalog lists rototo rules. A workspace-scoped catalog
lists rototo rules plus custom rules declared in that workspace. Duplicate
custom ids with identical metadata are deduplicated. Duplicate custom ids with
different metadata are lint failures.

## Built-In Rule Namespace

Built-in rototo rules currently include:

```text
rototo/workspace-not-found
rototo/workspace-manifest-missing
rototo/workspace-manifest-parse-failed
rototo/workspace-manifest-schema-failed
rototo/workspace-context-schema-ref
rototo/workspace-context-schema-attribute
rototo/qualifier-parse-failed
rototo/qualifier-schema-version
rototo/qualifier-missing-table
rototo/qualifier-predicate-missing
rototo/qualifier-predicate-shape
rototo/qualifier-predicate-unknown-op
rototo/qualifier-predicate-unknown-qualifier
rototo/qualifier-predicate-bucket
rototo/qualifier-predicate-value
rototo/variable-parse-failed
rototo/variable-schema-version
rototo/variable-missing-table
rototo/variable-type-or-schema
rototo/variable-unknown-type
rototo/variable-lint-shape
rototo/variable-values-missing
rototo/variable-unknown-value
rototo/variable-value-type-mismatch
rototo/variable-value-schema-mismatch
rototo/variable-schema-ref
rototo/variable-env-missing-default
rototo/variable-unknown-environment
rototo/variable-env-shape
rototo/variable-rule-shape
rototo/variable-rule-unknown-qualifier
rototo/variable-external-values-load-failed
rototo/variable-external-value-parse-failed
rototo/variable-external-value-duplicate
rototo/custom-lint-failed
rototo/custom-lint-invalid-rule
rototo/custom-lint-unknown-rule
rototo/custom-lint-rule-conflict
rototo/schema-parse-failed
rototo/schema-invalid
```

## Test Contract

Tests assert both exhaustiveness and specificity:

- every `RototoRuleId::iter()` entry is emitted by a targeted fixture;
- custom lint has fixtures for declared custom rule emission, malformed rule
  ids, undeclared rule ids, script failures, and conflicting metadata;
- external variable value load, parse, and duplicate failures have separate
  fixtures and rules;
- CLI catalog tests cover global rototo rules and workspace-scoped custom rules.

`just check` is the release gate for this layer.
