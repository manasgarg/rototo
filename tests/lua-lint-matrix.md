# Custom Lua lint test matrix

Package-local lint rules are untrusted code rototo executes: `lint/*.lua`
files register rules that run against the composed package. That makes this
a sandbox boundary as much as an extension point. This file inventories the
contract, in the same form as `tests/composition-matrix.md`. The runtime is
`src/lua_lint.rs`; registration, marshalling, and targeting live in
`src/lint/custom/`.

Rule-id identity (authority grammar, `rototo` reservation) overlaps
`tests/lint-identity-matrix.md` I4; the enforcement rows live here.

## 1. The sandbox

| # | When a Lua file... | Then... | Coverage |
|---|---|---|---|
| B1 | touches `io`, `os`, `require`, `dofile`, `loadfile`, `package`, or other process/file/state globals | the global is absent: the sandbox exposes no filesystem, process, or module-loading surface | `custom_lint_sandbox_denies_file_process_state_and_require_globals` (`src/lua_lint.rs`) |
| B2 | spins in an infinite loop during registration | the run is bounded and fails instead of hanging the lint | `custom_lint_registration_loop_is_bounded` |
| B3 | spins in an infinite loop inside a handler | same bound, per handler invocation | `custom_lint_handler_loop_is_bounded` |
| B4 | errors with a path in scope | chunk names in error messages are sanitized package-relative labels, never absolute host paths | `custom_lint_registration_errors_use_safe_chunk_names`, `custom_lint_handler_errors_use_safe_chunk_names` |

## 2. Registration contract

| # | When a lint file... | Then... | Coverage |
|---|---|---|---|
| R1 | defines no `register` function | `rototo/custom-lint-failed` at register stage | `custom_lint_without_register_is_an_error` (`src/lua_lint.rs`), `snapshot_records_source_backed_failure_diagnostics` (`src/lint/engine.rs`) |
| R2 | registers a rule missing required fields, a non-callable handler, or an unsupported severity | `rototo/custom-lint-registration-invalid` naming the problem | `reports_registered_custom_lint_failures`, canonical fixtures `custom-lint-registration-invalid`, `custom-lint-rule-shape` (`tests/package_lint.rs`) |
| R3 | registers a target outside the address grammar | rejected with the target named; the full grammar and its round-trip live in the index matrix T1-T3 | `reports_custom_registration_contract_failures` (`tests/package_lint.rs`), `registered_lint_addresses_round_trip_between_grammar_and_parse` (`src/lint/custom/registry.rs`) |
| R4 | claims the `rototo` authority | rejected: `rototo/<rule-id>` identifies built-ins only | `lua_rules_may_not_claim_the_rototo_authority` (`tests/package_lint.rs`), `the_rototo_authority_is_reserved_for_built_ins` (`src/diagnostics.rs`) |
| R5 | uses a malformed rule id (wrong segment count, uppercase, underscores) | rejected at parse | `malformed_custom_rule_ids_are_rejected` (`src/diagnostics.rs`) |
| R6 | registers the same rule id twice with different metadata | `rototo/custom-lint-registration-duplicate` / rule-conflict diagnostics | canonical fixtures `custom-lint-registration-duplicate`, `custom-lint-rule-conflict` |
| R7 | exists but registers nothing | `rototo/custom-lint-file-unregistered`: a silent lint file is a mistake | canonical fixture `custom-lint-file-unregistered` |
| R8 | registers successfully | the registration lands in the index registry with file, stage, selector, and handler | `snapshot_index_records_custom_lint_registry` (`src/lint/engine.rs`), index matrix D8 |

## 3. Execution

| # | Given a registered rule, when it runs... | Then... | Coverage |
|---|---|---|---|
| X1 | against its target | diagnostics carry the registered rule id, policy stage, the target entity, and a source range | `reports_registered_custom_lint_failures`, `reports_registered_custom_lint_targets` (`tests/package_lint.rs`) |
| X2 | and the handler errors at runtime | `rototo/custom-lint-failed` on the lint file; the lint run itself never crashes | `reports_package_custom_lint_failures`, canonical fixture `custom-lint-failed` |
| X3 | and returns malformed output (non-table, missing `message`) | a contract failure diagnostic, not a panic | `reports_custom_lint_contract_failures` (`tests/package_lint.rs`) |
| X4 | in an overlay package | the rule sees the whole composed package, base files included | `overlay_lint_rules_run_against_the_composed_package` (`tests/composition.rs`, composition matrix X1) |
| X5 | with a warning severity | the severity flows to diagnostics and the catalog | `lists_package_custom_warning_severity` (`tests/diagnostics.rs`) |
| X6 | across every address form | each targeted entity kind is checked (the `custom-targets` fixture registers one rule per address form) | `reports_registered_custom_lint_targets` |

## 4. Discovery

| # | Given / When | Then | Coverage |
|---|---|---|---|
| D1 | `lint/<file>.lua` at the top of the lint directory | discovered, indexed, run | index matrix D8 and every row above |
| D2 | a `.lua` file in a subdirectory (`lint/sub/x.lua`) | pinned current behavior: ignored silently, with no unrecognized-file warning, because the discover walker covers `model/`, `data/`, `variables/`, and `layers/` but not `lint/`. Needs a decision: either discover recursively like every other collection, or warn. | `nested_lua_files_are_silently_ignored_today` (`tests/package_lint.rs`) |

## Current gap tally

0 GAP rows. One pinned-behavior row (D2) carries a needs-decision note for
the review pass.

When you add custom-lint capability (new address forms, new registration
fields, new sandbox surface), add the row and the test together; an empty
Coverage cell is a regression in this file's contract.
