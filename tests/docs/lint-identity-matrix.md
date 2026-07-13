# Lint engine and diagnostic identity test matrix

Lint is core behavior, and its diagnostics are an API: rule ids are stable
identifiers that tooling, docs, and suppression workflows key on. This file
inventories the engine's meta-promises, the identity rules, and the
machinery that keeps per-rule coverage honest, in the same form as
`tests/docs/composition-matrix.md`. Unless a row says otherwise, tests live in
`tests/package_lint.rs`.

What this file deliberately does not do: re-list every individual rule.
Per-rule behavior is owned by the canonical fixture table inside
`tests/package_lint.rs` (row M2), which is itself a machine-checked
inventory; duplicating it here would rot.

## 1. Rule identity

| # | Promise | Coverage |
|---|---|---|
| I1 | Every built-in rule id is flat `rototo/<rule-id>`: starts with `rototo/`, exactly one slash, never nested | `builtin_diagnostic_rule_ids_are_flat_rototo_ids` |
| I2 | Custom rules are `<authority>/<rule-id>` with a non-`rototo` authority; the `rototo` authority is reserved | custom-lint matrix (`tests/docs/lua-lint-matrix.md`); `CustomRuleId::parse` unit behavior |
| I3 | Custom catalog entries never claim a built-in entity | `custom_diagnostic_catalog_entries_do_not_claim_variable_entity` (`tests/diagnostics.rs`) |

Retired rule ids carry no machinery: when a rule dissolves, its variant,
catalog metadata, fixtures, and tests are deleted outright (decided
2026-07-05; there is no reserved-id list pre-stability).

## 2. The diagnostics catalog

| # | Given / When | Then | Coverage |
|---|---|---|---|
| C1 | `rototo show --lint-rules` with no package | the global catalog: built-in rules only | `lists_global_diagnostics`, `lists_global_diagnostics_as_json` (`tests/diagnostics.rs`) |
| C2 | `rototo show <package> --lint-rules` | the package-scoped catalog: built-ins plus the package's declared custom rules, warning severities included | `lists_package_scoped_diagnostics_when_requested`, `lists_package_level_custom_diagnostics`, `lists_package_custom_warning_severity`, `lists_custom_lint_example_diagnostics` (`tests/diagnostics.rs`) |
| C3 | `--lint-rule <id>` | one rule's entry, entity included; a missing id is an error naming it | `gets_package_diagnostic`, `gets_package_custom_diagnostic`, `missing_diagnostic_fails` (`tests/diagnostics.rs`) |
| C4 | the catalog source | index-backed: built-ins from `RototoRuleId::iter()`, customs from the package's registry (index matrix CC7) | `diagnostic_catalog_entries` in `src/lint.rs`, exercised by C1-C3 |

## 3. Coverage machinery: every rule provably fires

| # | Promise | Coverage |
|---|---|---|
| M1 | Every non-retired `RototoRuleId` is accounted for: it has a canonical fixture (with expected severity, stage, entity, and location asserted) or an explicit pending entry; nothing falls through silently | `canonical_rule_fixture_table_covers_every_rototo_rule` |
| M2 | Each canonical fixture under `tests/fixtures/packages/rules/<stage>/<rule-id>/` fires exactly its expected diagnostics | `assert_canonical_fixture` over `canonical_rule_fixtures()` |
| M3 | Every rule on the pending list that no other test fired now has a scratch-package firing test (list parse/schema-version/shape/members-parse, unknown catalog, catalog entry parse, sample schema-mismatch/shape/parse) | `pending_rules_fire_from_scratch_packages` |
| M4 | The `lint-failures` fixture reports exactly its declared rule set: additions and regressions both fail the equality | `lint_failures_expected_rule_ids`, `lint_failures_fixture_reports_expected_rule_ids` |
| M5 | Every TOML/JSON parse failure among the fixtures is intentional and listed; a fixture that rots into unparseability fails the ratchet | `package_fixture_parse_failures_are_intentional` |
| M6 | The curated example packages stay lint-clean | `lints_curated_examples`, plus `examples.rs` loading each example through the SDK's lint gate |

Note for the review pass: M3's scratch tests prove the pending rules fire,
but those rules still lack canonical fixtures with asserted stage, entity,
and location. Promoting them from the pending list to
`canonical_rule_fixtures()` is mechanical follow-up work; the pending list
is the worklist.

## 4. Selection and output

| # | Given / When | Then | Coverage |
|---|---|---|---|
| S1 | `lint --variable <id>` | diagnostics scope to that variable | `lints_condition_variable_by_id`, `lints_variable_from_discovered_package` (`tests/qualifier_variable.rs`) |
| S2 | `lint --lint-rule <id>` / `--lint-authority <a>` | diagnostics filter to the selection; a failing rule keeps the failing exit, an unmatched filter reports ok | `lint_selectors_filter_diagnostics_and_exit_status` (`tests/cli.rs`) |
| S3 | `inspect --lint-rule` / `--linter` | the inspect views scope the same way | `tests/package_inspect.rs` selector tests |
| S4 | `--quiet` | success output is suppressed, diagnostics never are | `quiet_suppresses_successful_lint_output`, `quiet_keeps_lint_diagnostics` (`tests/cli.rs`) |
| S5 | diagnostics ordering | stable across runs (sorted before output) | `sort_diagnostics` by construction; the determinism row Det1 of the index matrix covers the pipeline end to end |
| S6 | JSON output shape | rule, severity, stage, target entity, location with range, related locations | `assert_expected_diagnostics` machinery across the canonical fixtures |

## 5. Stage model

| # | Promise | Coverage |
|---|---|---|
| P1 | Stages run in order (discover, parse, project+register, then reference, value, graph, policy), and each checked stage runs built-ins then registered custom lints | by construction (`src/lint/stages/mod.rs::run_until`); observable through the per-stage fixture directories in M2 |
| P2 | A `run_until` stop point yields the stages up to it (the LSP and inspect paths rely on partial runs) | exercised through every snapshot consumer; index matrix section 2 covers the isolation half |

## Current gap tally

0 GAP rows. One recorded follow-up: promote the pending rules to canonical
fixtures (section 3 note).

When you add a lint rule: add the `RototoRuleId`, a canonical fixture
directory, and the table entry together; extend the `lint-failures` fixture
when the rule belongs in its coverage set. The equality assertions in M1,
M4, and M5 make skipping any of those a test failure, not a review comment.
