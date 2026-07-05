# CEL expression test matrix

Rototo's `when` and query `filter`/`sort` expressions are a CEL subset: the
`cel` crate (pinned `=0.14.0`) does the parsing and evaluation, and rototo
owns the schema-aware front-end around it (`src/expression/`): the five
roots, reference extraction, constraint inference, and fixture-context
synthesis.
This file is the executable inventory of that contract, in the same form as
`tests/docs/composition-matrix.md`. Unless a row says otherwise, tests are the
inline suite in `src/expression/mod.rs`.

Boundaries: what lint does with an invalid expression (which rule fires,
where the diagnostic points) belongs to the lint matrices; what resolution
does with an evaluated result belongs to `tests/docs/resolution-matrix.md`.

## 1. Roots: exactly five, everything else rejected

| # | Given an expression reading... | Then... | Coverage |
|---|---|---|---|
| R1 | `context`, `entry`, `variables["<id>"]`, `enums.<id>`, `env.now` | all five roots evaluate; no issues reported | `flags_invalid_expression_roots` (the valid arm), `evaluates_context_paths_entry_paths_and_variables`, `evaluates_enum_membership`, `evaluates_env_members` |
| R2 | an unknown root (`foo.bar`) | reported as `UnknownRoot` | `flags_invalid_expression_roots` |
| R3 | an unknown `env` member (`env.bogus`) | reported as `UnknownEnvMember` | `flags_invalid_expression_roots` |
| R4 | the retired `qualifier["x"]` or `env.qualifier["x"]` spellings | reported as `LegacyQualifier`, whose lint diagnostic points at `variables["<id>"]` | `flags_invalid_expression_roots` |
| R5 | identifiers bound by a CEL comprehension (`.exists(x, ...)`) | not misread as unknown roots | `comprehension_bound_identifiers_are_not_unknown_roots` |
| R6 | `env.resolving.*` | valid only inside `[[trace]]` policies (resolution matrix T4) | `env_resolving_outside_trace_policy_is_rejected` (`tests/sdk.rs`) |
| R7 | `variables["<id>"]` in either spelling (`variables.premium` dot form included) | both spellings extract the same reference | `tracks_variable_references_in_both_spellings` |
| R8 | `enums.<id>` in either spelling (`enums["geo/regions"]` for namespaced ids) | both spellings extract the enum reference, and `context.<path> in enums.<id>` records the membership pair for lint's type refinement | `tracks_enum_references_in_both_spellings` |
| R9 | `enums.<id>` naming an enum the package does not declare | `rototo/expression-unknown-enum` at lint (trace policies and layer expressions report through their own catch-all rules); at evaluation, a resolution error naming the enum | canonical fixture `rules/project/expression-unknown-enum`, `lint-failures`, `evaluates_enum_membership` (the error arm) |

## 2. Evaluation semantics

| # | Given / When | Then | Coverage |
|---|---|---|---|
| V1 | boolean composition | CEL precedence and short-circuiting hold; the right side of a short-circuited `&&`/`||` never evaluates | `evaluates_logical_precedence_and_short_circuiting` |
| V2 | comparisons, `in` membership, JSON equality | deep equality over JSON values, membership over lists | `evaluates_comparison_membership_and_json_equality` |
| V3 | context paths, entry paths, variable references | values thread from the caller's JSON and the lazy variable resolver | `evaluates_context_paths_entry_paths_and_variables`, `evaluates_variable_references` |
| V4 | `env.now` | the injected instant compares as an RFC3339 string and feeds the time functions | `evaluates_env_members` |
| V8 | `enums.<id>` | binds the enum's member list as an ordinary CEL list, so `in`, `size`, and comprehensions compose; a rule `when` and a query `filter` both select through it end to end | `evaluates_enum_membership`, `resolves_enum_membership_in_when_and_query` (`src/resolve.rs`) |
| V5 | a syntactically malformed expression | rejected at parse; unknown-identifier checking is the schema-aware layer's job (R2), not the parser's | `rejects_malformed_expressions_at_parse` |
| V6 | a wrong-typed evaluation (missing context key, absent `entry`, non-bool operand, unknown function, bad regex, bad cidr, non-collection `size`) | an evaluation error, never a coercion; a non-bool result for a bool position gets the stable "did not evaluate to bool" message | `reports_evaluation_errors_with_stable_messages` |
| V7 | a basic end-to-end expression | parse then evaluate round-trips | `parses_and_evaluates_basic_expression` |

## 3. The function registry

| # | Function family | Coverage |
|---|---|---|
| F1 | `startsWith`/`starts_with`/`prefix`, `endsWith`/`ends_with`/`suffix`, `contains` (strings and lists) | `evaluates_supported_functions` |
| F2 | `matches`/`regex`, `glob`, `semver` | `evaluates_supported_functions` |
| F3 | `cidr` (single range and list of ranges) | `evaluates_supported_functions` |
| F4 | `has`, `present`, `missing`, `path`, `size` | `evaluates_supported_functions` |
| F5 | `bucket(unit, salt, lo, hi)` boundary behavior | `evaluates_supported_functions` (full and empty ranges); determinism is the resolution matrix's B1/B2 |
| F6 | the time family: `timeAfter`, `timeAtOrAfter`, `timeBefore`, `timeAtOrBefore`, `timeBetween`, in both camelCase and snake_case spellings | `evaluates_supported_functions` |
| F7 | error paths (invalid regex, invalid cidr, `size` of a scalar, unknown function) | `reports_evaluation_errors_with_stable_messages` |

## 4. Reference extraction (`references.rs`)

| # | Given / When | Then | Coverage |
|---|---|---|---|
| E1 | nested context paths, function arguments, variable references | every referenced context path, entry path, and variable is extracted | `extracts_references_from_nested_paths_functions_and_variables`, `tracks_variable_and_entry_references` |
| E2 | the extraction output | feeds the reference walker rows of the semantic index matrix (CC1-CC4) and the lint rules that check unknown variables and context paths | consumers' matrices |

## 5. Constraint inference (`types.rs`)

| # | Given / When | Then | Coverage |
|---|---|---|---|
| T1 | comparisons and function uses of a context path | the path's expected scalar types are inferred (string, int, number, bool) | `infers_context_path_scalar_types_from_use` |
| T2 | `cidr` and time functions | the string operand is refined (ip / timestamp expectations) | `infers_refined_string_types_from_cidr_and_time_functions` |
| T3 | a `bucket` unit argument | left unconstrained: any JSON value can hash | `leaves_bucket_value_argument_unconstrained` |
| T4 | conflicting uses of one path | both expectations are recorded rather than one silently winning | `records_conflicting_uses_as_multiple_expectations` |
| T5 | `context.<path> in enums.<id>` | the membership is recorded at parse (the member type is unknown there); lint refines the path's expected scalar family from the declared enum and reports mismatches through `rototo/variable-rule-context-path-type-mismatch` | `tracks_enum_references_in_both_spellings` (the membership map), `enum_membership_mismatches` (`src/lint/builtins/evaluation_context.rs`) |

## 6. Fixture-context synthesis (`synthesize.rs`)

Synthesis inverts an expression into a context that makes it true (or
false): the engine behind `rototo fixtures`.

| # | Given an expression shaped as... | Then... | Coverage |
|---|---|---|---|
| Y1 | equality / inequality | a satisfying (or refuting) context synthesizes and round-trips through evaluation | `synthesizes_equality_and_inequality`, `assert_round_trip` |
| Y2 | orderings | boundary-respecting values synthesize | `synthesizes_orderings` |
| Y3 | membership (`in`) | an element (or non-element) synthesizes | `synthesizes_membership` |
| Y10 | membership against `enums.<id>` | a member (or non-member) synthesizes when the caller supplies the member list; the fixtures CLI passes no enum data yet (the inspect report does not carry members), so those candidates honestly return `None` and are dropped | `synthesizes_enum_membership` |
| Y4 | boolean composition | conjunctions and disjunctions merge branch contexts | `synthesizes_boolean_composition` |
| Y5 | `bucket(...)` | a unit id hashing into the wanted range is searched for and found | `synthesizes_bucket`, `fixtures_command_synthesizes_a_unit_id_per_arm` (`tests/fixtures.rs`) |
| Y6 | an unsatisfiable bucket range | the candidate search stops at `MAX_BUCKET_CANDIDATES` and reports no context instead of spinning | `bucket_synthesis_gives_up_after_the_candidate_budget` |
| Y7 | `variables["<id>"]` composition | the referenced condition's own expression synthesizes recursively and its context merges in | `synthesizes_through_condition_composition`, `synthesizes_contexts_through_variable_references` |
| Y8 | a shape the synthesizer does not model (free-form string functions) | `None`, honestly, rather than a wrong context | `returns_none_for_uninvertible_shapes` |
| Y9 | the whole package surface | `rototo fixtures` prints runnable resolve commands whose contexts actually satisfy the rules, in both context forms, JSON output included | `tests/fixtures.rs` (all six tests, including `printed_resolve_command_runs_end_to_end`) |

## Current gap tally

0 GAP rows.

When you add or change expression behavior (a new function, a new root, a
new synthesizable shape), add the row and the test together; an empty
Coverage cell is a regression in this file's contract.
