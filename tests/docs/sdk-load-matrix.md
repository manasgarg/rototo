# SDK load and inspect test matrix

`Package::load` and `Package::inspect` (`src/sdk.rs`) are the app-facing
front door: one call that stages a source, lints it, compiles the runtime,
and hands back a package apps can resolve from. This file is the executable
inventory of those promises, in the same form as
`tests/docs/composition-matrix.md`. Unless a row says otherwise, tests live in
`tests/sdk.rs`.

Boundaries: source grammar and staging security live in
`tests/docs/source-auth-matrix.md`; the running-process lifecycle lives in
`tests/docs/refresh-matrix.md`; what resolution does with the loaded package
lives in `tests/docs/resolution-matrix.md`.

## 1. Load pipeline

| # | Given / When | Then | Coverage |
|---|---|---|---|
| P1 | `Package::load` on a clean package | the package loads linted and resolvable | `package_sdk_loads_linted_package`, `package_sdk_resolves_from_loaded_runtime_snapshot` |
| P2 | a package that fails lint with errors | the load is rejected: `load` never serves a package it could not vouch for | `package_sdk_rejects_package_when_lint_fails` |
| P3 | a package with only lint warnings | the load succeeds; warnings are not gates | `package_sdk_loads_package_when_lint_only_warns` |
| P4 | `Package::inspect` on the same broken package | inspection succeeds without the lint gate: the lower-level loader for tools that need staged data | `package_sdk_can_inspect_without_linting`, `package_sdk_loads_malformed_context_config_when_lint_is_skipped_for_inspection` |
| P5 | `LoadOptions::with_lint(LintMode::Skip)` | the gate can be bypassed explicitly for inspection tools | `package_sdk_can_load_with_lint_skipped_for_inspection_tools` |
| P6 | a local path, `file://`, or `git+file://…#ref:subdir` source | all load through the one source grammar | `package_sdk_loads_file_source`, `package_sdk_loads_git_file_source_with_ref_and_subdir` |
| P7 | an `http://` archive source | refused (see the source matrix for the full grammar rows) | `package_source_rejects_http_archive_source` |
| P8 | a package with extends layers | the SDK loads the composed result; child overrides win | `package_sdk_loads_layered_package_with_child_overrides` |
| P9 | a remote source staged into a temporary checkout | the checkout lives as long as the package and is removed on drop: staging is owned, not leaked | `dropping_a_package_removes_its_staged_checkout` |
| P10 | identity | a local source has no release id; a git source derives a `git:` release id | `package_identity_for_local_source_has_no_release_id`, `package_identity_for_git_source_derives_git_release_id` |

## 2. Fallback selection

One fallback, same source grammar, zero leniency. (What happens after a
fallback start is the refresh matrix's S2/R5/R6/R7.)

| # | Given a fallback option, when the primary... | Then... | Coverage |
|---|---|---|---|
| F1 | is unreachable | the fallback loads and `served_fallback()` is true | `package_loads_the_fallback_when_the_primary_is_unavailable` |
| F2 | fails lint | same: any primary failure triggers the fallback, staging and gating alike | `package_falls_back_when_the_primary_fails_lint` |
| F3 | is healthy | the primary wins; the fallback is never touched | `package_prefers_a_healthy_primary_over_the_fallback` |
| F4 | and the fallback both fail | one error naming both attempts, primary first, with both reasons | `package_load_error_names_both_attempts_when_both_fail` |
| F5 | fails and the fallback fails lint | the fallback went through the identical pipeline: a lint-failing fallback is a failed fallback (F4's error) | `a_lint_failing_fallback_is_a_failed_fallback` |
| F6 | is an https archive with a bare token, and the fallback is an https archive on a different origin | the fallback is refused by the single-origin binding rather than receiving a token minted for the primary; scoped tokens are the dual-archive answer, and a local fallback never touches the binding (decided keep-as-is, review finding 8) | `bare_token_binding_spans_primary_and_fallback_archive_origins` |

Cross-language: F1, F3, and F4 run through all four language SDKs as shared
contract cases (`tests/sdk-contract/cases.jsonl`, operation
`load_package_with_fallback`; runner arms in `tests/sdk_contract.rs` and the
per-language runners).

## 3. Reading the loaded package

| # | Given / When | Then | Coverage |
|---|---|---|---|
| R1 | `inspect_package` / `inspection()` | entities, paths, and linters are enumerated | `sdk_inspects_package` |
| R2 | `lint_package`, `lint_variable`, `lint_catalog` | scoped lint views over the same pipeline | `sdk_lints_package`, `sdk_lints_condition_variable`, and `tests/package_lint.rs` |
| R3 | `list_variables`, `list_catalogs` | app-facing listings | `sdk_lists_variables_for_apps`, `sdk_lists_catalogs_for_apps` |
| R4 | `read_variable`, `read_catalog` and the plural forms | typed `VariableConfig` / `CatalogConfig`, with declared sources | `sdk_reads_variable_config`, `sdk_reads_catalog_config`, `sdk_reads_all_basic_variable_configs_with_declared_sources`, `sdk_reads_primitive_variable_values`, `sdk_reads_condition_variable_configs` |
| R5 | catalog entry files | read as whole TOML objects, nested tables included | `catalog_entry_files_are_whole_toml_objects` |
| R6 | the diagnostics catalog | built-in rules plus the package's declared custom rules | `sdk_reads_diagnostic_catalog` |
| R7 | `semantic_model()` | the projected model of the loaded package (shape owned by `tests/semantic_model.rs` and the index matrix) | `tests/semantic_model.rs` |
| R8 | the sample app | the documented app-loading flow compiles and runs | `sdk_sample_app_runs` |

## 4. Context validation at the boundary

(Resolution semantics themselves are the resolution matrix's; these rows are
the load-level contract that a context is checked before any rule runs.)

| # | Given / When | Then | Coverage |
|---|---|---|---|
| C1 | a context violating the evaluation context schema | resolution is refused up front | `package_sdk_validates_evaluation_context_against_schema`, `package_sdk_resolves_with_context_contract` |
| C2 | a context missing a fact a condition reads, even when the schema allows absence | refused: conditions never silently read null | `package_sdk_rejects_missing_condition_context_even_when_schema_allows_it` |
| C3 | a non-object context | refused | `package_sdk_rejects_non_object_evaluation_context` |
| C4 | explicit opt-out | `resolve_variable_with_options` can bypass validation deliberately | `package_sdk_can_bypass_context_validation_explicitly` |
| C5 | a context schema behind a symlink escaping the package | refused at load | `package_sdk_rejects_context_schema_symlink_escape` |

## 5. Reflection and lookup

The app-side surface for what hydration used to do implicitly
(`design/package-reflection.md`; landed for Rust, language SDKs follow).

| # | Given / When | Then | Coverage |
|---|---|---|---|
| X1 | `list_enums` / `read_enum` | list ids; one list's contract and members together; a missing id errors | `reflection_reads_enums` |
| X2 | `list_entries` / `read_entry` | entry ids only; one raw entry as authored (no hydration, no id injection) | `reflection_discovers_looks_up_and_walks_references` |
| X3 | `resolve_reference` / `resolve_reference_at` | one hop with hydration's exact semantics: entry lookup, pointer application, first-match rule for multi-catalog pins, raw target value; errors name the address | `reflection_discovers_looks_up_and_walks_references` |
| X4 | `ValueRef` construction | from an address, from a raw entry-ref string plus its pins, or from a dynamic ref object; canonical `address()` rendering | `reflection_discovers_looks_up_and_walks_references` |
| X5 | `references_in` | every `x-rototo-ref` field in a value reported as (pointer, reference), `$ref` indirection included, nothing spliced | `reflection_discovers_looks_up_and_walks_references` |
| X6 | the post-hydration app story | resolve a variable, get raw refs, follow exactly the ones the app renders | `billing_entitlements_follow_through_the_lookup_surface` |

## 6. Error surface

| # | Given / When | Then | Coverage |
|---|---|---|---|
| E1 | any load failure | one `RototoError` with a human-readable message; the CLI and bindings map from it | throughout `tests/sdk.rs`; per-language mapping in each SDK's wrapper tests |
| E2 | sources appearing in error messages | they pass through `redacted_source` (F4's message builder) | by construction in `fallback_load_error`; the redaction function's own behavior is a source-matrix concern |

Note for the review pass: `LoadOptions::with_source_auth` takes the
`SourceAuth` list directly, so bare-versus-scoped mutual exclusion cannot be
violated in Rust (the type forbids it); the language SDKs re-enforce it at
their option boundaries and test it in their wrapper suites (mutual
exclusion of `package_token` and `package_tokens`).

## Current gap tally

0 GAP rows.

When you add or change load behavior, add the row and the test together; an
empty Coverage cell is a regression in this file's contract.
