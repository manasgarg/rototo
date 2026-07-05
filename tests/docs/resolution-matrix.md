# Variable resolution test matrix

Resolution (`src/resolve.rs`, hydration in `src/resolve/hydrate.rs`) is the
runtime answer to "what value does this variable have for this context".
This file is the executable inventory of its promises, in the same form as
`tests/docs/composition-matrix.md`. Unless a row says otherwise, tests are the
inline suite in `src/resolve.rs`; `tests/qualifier_variable.rs` holds the
CLI-level resolve tests (the file predates the variable rename), and
`tests/sdk.rs` holds the SDK-level ones.

Boundaries: expression evaluation semantics live in
`tests/docs/expression-matrix.md`; context validation at the API boundary is the
SDK load matrix's section 4; trace provenance across layers is
`tests/docs/composition-matrix.md` V5/D2.

## 1. Selection: rules, defaults, fail-closed

| # | Given / When | Then | Coverage |
|---|---|---|---|
| S1 | rules where an earlier and a later rule both match | the first matching rule wins; no rule matching falls back to the default | `resolves_variable_default_and_fails_closed`, `resolves_when_conditions_and_catalog_query_variables` |
| S2 | no rule matches and no default exists | resolution fails closed rather than inventing a value | `resolves_variable_default_and_fails_closed` |
| S3 | a predicate over context values | true and false outcomes select accordingly, for the whole operator set | `resolves_predicate_operator_true_and_false_outcomes`, `resolves_expanded_predicate_operators` |
| S4 | a condition read through `variables["<id>"]` | the referenced condition variable resolves and gates the rule | `resolves_condition_indirection_and_cycles`, `resolves_cross_variable_references_and_cycles` |
| S5 | a `when` that fails to evaluate (wrong type, malformed) | resolution errors; conditions never silently coerce | `malformed_conditions_return_errors_during_resolution` |
| S6 | a context missing a path a condition reads | resolution errors: absent facts are not null | `missing_context_paths_fail_resolution`, `package_sdk_rejects_missing_condition_context_even_when_schema_allows_it` (`tests/sdk.rs`) |
| S7 | numeric equality near integer precision limits | exact comparison, no lossy large-integer casts | `numeric_equality_is_exact_without_lossy_large_integer_casts` |

## 2. Cross-variable references

| # | Given / When | Then | Coverage |
|---|---|---|---|
| X1 | `variables["<id>"]` chains | referenced variables resolve lazily, on first use | `resolves_cross_variable_references_and_cycles` |
| X2 | the same referenced variable read by several rules, by a trace policy, or across a batch | one evaluation per resolution run: values memoize in the shared state, batch resolve and batch trace included, and a traced batch always agrees with the resolved batch it explains | `batch_resolve_and_batch_trace_share_one_evaluation_state` (`src/resolve.rs`); the cache is also what makes X3's cycle detection work |
| X3 | a reference cycle reached at resolve time | resolution errors naming the cycle (the lint-time twin is `rototo/variable-reference-cycle`, `tests/package_lint.rs`) | `resolves_cross_variable_references_and_cycles`, `resolves_condition_indirection_and_cycles` |
| X4 | `env.now` read in several expressions of one resolution, or across a batch | one instant per resolution run, captured at state creation (RFC3339); batch resolve and batch trace share it | by construction (`ResolutionState::new` captures `now` once) plus `batch_resolve_and_batch_trace_share_one_evaluation_state` |

## 3. Buckets

| # | Given / When | Then | Coverage |
|---|---|---|---|
| B1 | a bucket predicate at range boundaries | inclusive edges behave exactly; assignment is deterministic for a unit | `resolves_bucket_boundaries_and_is_deterministic` |
| B2 | the same unit across processes | hashing is stable (salted, canonicalized unit value) | `resolves_bucket_boundaries_and_is_deterministic` (`bucket_value` / `stable_unit_hash` under it) |

## 4. Catalog queries (`method = "query"`)

| # | Given / When | Then | Coverage |
|---|---|---|---|
| Q1 | `single` with sort | the top entry after sort wins | `query_single_selects_the_top_entry_after_sort` |
| Q2 | `single` without sort | exactly one match required; several is an error telling you to sort or narrow | `query_single_without_sort_requires_exactly_one_match` |
| Q3 | `single` with no match | the default applies, or resolution errors without one | `query_single_with_no_match_uses_default_or_errors` |
| Q4 | a list query | matches sort and truncate at the limit | `query_list_sorts_and_limits_matches` |
| Q5 | a list query with no match and no default | the empty list | `query_list_with_no_match_is_empty_without_default` |
| Q6 | sort keys of mixed types | resolution errors: incomparable keys never sort arbitrarily | `query_sort_keys_must_be_comparable` |
| Q7 | the `entry` root in filter and sort | each entry is evaluated as its hydrated view with its `id` injected | `resolves_when_conditions_and_catalog_query_variables`, `query_predicates_see_hydrated_views_and_apps_get_raw_entries` (`tests/sdk.rs`) |

## 5. Allocations (`method = "allocation"`)

| # | Given / When | Then | Coverage |
|---|---|---|---|
| A1 | a running allocation | arms assign deterministically by bucket | `allocation_assigns_arms_deterministically` |
| A2 | an ineligible unit | the default value, not an arm | `allocation_ineligible_units_resolve_to_the_default` |
| A3 | a non-running allocation | no assignment happens | `allocation_only_assigns_while_running` |
| A4 | a bucket no arm claims | the default | `allocation_unclaimed_buckets_resolve_to_the_default` |
| A5 | a traced allocation resolution | the trace records the assignment | `allocation_trace_records_the_assignment` |

## 6. References in catalog-backed values

Hydration is for resolution, not for apps (decided 2026-07-05, review
finding 1): query predicates evaluate against hydrated entry views, and the
value an app receives is the raw entry. Apps follow references explicitly
through the reflection surface (`design/package-reflection.md`).

| # | Given a selected catalog entry whose schema pins refs... | Then... | Coverage |
|---|---|---|---|
| H1 | a query filter or sort reads `entry.<field>` | the predicate sees the hydrated view: every ref form spliced in (`<entry>#<json-pointer>`, multi-catalog targets, dynamic `{catalog, entry, pointer}` objects, refs behind same-document and relative-file `$ref` indirection); cycles fall back to the raw value | `query_predicates_see_hydrated_views_and_apps_get_raw_entries` (`tests/sdk.rs`), `relative_schema_refs_resolve_against_the_catalog_base` (`src/resolve/hydrate.rs`) |
| H2 | the query-selected value returns to the app | the raw entry, ref strings as authored, with only the entry `id` injected (identity is not hydration) | `query_predicates_see_hydrated_views_and_apps_get_raw_entries`, `resolves_when_conditions_and_catalog_query_variables` (`src/resolve.rs`), `billing_resolves_the_plan_with_raw_entitlement_refs` (`tests/examples.rs`) |
| H3 | a rules- or default-selected value returns to the app | the same raw contract: value shapes are method-independent | `rules_selected_catalog_values_reach_apps_raw` (`tests/sdk.rs`) |

## 7. Trace

| # | Given / When | Then | Coverage |
|---|---|---|---|
| T1 | an app-requested trace | the trace records the selected value, source, and rule outcomes | `resolves_condition_variable_with_trace_output`, `resolves_variable_with_trace_output` (`tests/qualifier_variable.rs`), `app_requested_trace_is_emitted_to_subscriber` (`tests/sdk.rs`) |
| T2 | a `[[trace]]` policy matching the resolution | the trace emits to subscribers; non-matching resolutions stay silent | `package_trace_policy_emits_for_matching_resolution`, `package_trace_policy_does_not_emit_for_other_users` (`tests/sdk.rs`) |
| T3 | no subscriber | tracing is skipped entirely: zero cost on the hot path | `resolving_without_subscribers_skips_tracing` (`tests/sdk.rs`) |
| T4 | `env.resolving` outside a `[[trace]]` policy | rejected: the root exists only for policies | `env_resolving_outside_trace_policy_is_rejected` (`tests/sdk.rs`) |
| T5 | a composed package | trace provenance names the layer that owns the resolution (finer per-variable sidecar labels win) | `trace_provenance_names_the_layer_that_owns_the_resolution`, `a_three_deep_chain_composes_bottom_up` (`tests/composition.rs`) |

## 8. CLI surface

| # | Given / When | Then | Coverage |
|---|---|---|---|
| C1 | `rototo resolve --variable` with `--context` in each form (JSON, `@file`, `path=value`, merged left to right) | values resolve and print, `--json` included | `resolves_variable_by_id`, `resolves_variable_with_context_assignments`, `resolves_condition_variable_by_id`, and the rest of `tests/qualifier_variable.rs` |
| C2 | `--variables` (all) | every variable resolves, conditions included | `resolves_all_variables`, `resolves_all_variables_including_conditions` |
| C3 | no `--context` | an empty object context | `resolves_variable_without_context_as_empty_object` |

## Current gap tally

0 GAP rows.

When you add or change resolution behavior, add the row and the test
together; an empty Coverage cell is a regression in this file's contract.
