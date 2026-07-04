# Semantic index test matrix

The semantic index (`src/lint/index/`) is the data structure the whole lint
tree stands on: every built-in rule, the reference walker, hover, completion,
go-to-definition, document symbols, inspect views, and the public semantic
model all read from the one index a lint pipeline builds. This file is the
executable inventory of what the index itself promises, in the same
Given / When / Then form as `tests/docs/composition-matrix.md`: every row names the
test that proves it, and a GAP row is a promise we currently keep only by
reading the code.

How to use it:

- When you change discovery, projection, or node shapes, add or update the
  row first, then make the Coverage column true.
- When you write a new index test, name the row it covers.
- Direct rows are tested from inside the lint tree (the index is
  `pub(in crate::lint)`), in `src/lint/index/tests.rs` unless noted. Keep
  those tests on the promises below (shape, locations, targets, isolation,
  determinism), not on incidental structure, so they survive refactors.

The construction source of truth is `src/lint/stages/` (discover through
register), `src/lint/source/discover.rs` (file-to-document mapping), and
`src/lint/project/` (document-to-node projection).

## 1. Discovery: every file becomes the right node

Given a package containing a file of each kind the package format defines.
When the lint pipeline builds the index, then each file lands as exactly one
node in the right map, keyed by the id its path encodes.

| # | Given a file at... | Then the index holds... | Coverage |
|---|---|---|---|
| D1 | `rototo-package.toml` with `[[trace]]` policies | one `ManifestNode` with the trace policies in order and `extends` state (`Missing` when the key is absent) | `every_package_file_kind_projects_to_exactly_one_node` |
| D2 | `variables/<id>.toml`, including nested `variables/acme/in_trial.toml` | a `VariableNode` per file, keyed `flag` and `acme/in_trial`: directories are namespaces | `every_package_file_kind_projects_to_exactly_one_node` |
| D3 | `model/enums/<id>.toml` and `data/enums/<id>.toml`, both halves, including namespaced ids | an `EnumNode` and an `EnumMembersNode` under the same id in their two maps | `every_package_file_kind_projects_to_exactly_one_node` |
| D4 | `model/catalogs/<id>.schema.json` plus `data/catalogs/<id>/<entry>.toml` | a `CatalogNode` with a compiled validator and a `CatalogEntryNode` per entry file keyed by file stem | `every_package_file_kind_projects_to_exactly_one_node` |
| D5 | `model/context/<id>.schema.json` plus `model/context/<id>-samples/<sample>.json` | an `EvaluationContextNode` and a sample node per file; files inside a `-samples/` directory are never read as schemas | `every_package_file_kind_projects_to_exactly_one_node` |
| D6 | `layers/<id>.toml` with `[[allocation]]` and `[[allocation.arm]]` | a `LayerNode` with allocations and arms in declaration order | `every_package_file_kind_projects_to_exactly_one_node` |
| D7 | `governance.toml` with a `[<kind>.<id>]` block | a `GovernanceNode` whose blocks carry the kind and id; unknown kinds are collected, not dropped | `every_package_file_kind_projects_to_exactly_one_node` |
| D8 | `lint/<file>.lua` registering a rule | a `CustomLintFileNode` plus a `CustomLintRegistration` with the parsed selector address | `every_package_file_kind_projects_to_exactly_one_node`, `snapshot_index_records_custom_lint_registry` (`src/lint/engine.rs`) |
| D9 | a file under a rototo-owned directory that no entity claims (wrong extension, entry for an undeclared catalog) | no node anywhere, and a `rototo/unrecognized-file` warning naming the file: nothing vanishes silently | `unclaimed_files_produce_no_nodes_and_a_discover_warning` |
| D10 | a symlink to a directory named like an entity file | discovery skips it rather than indexing a directory | `sorted_directory_entries_skips_symlinked_directories` (`src/lint/source/discover.rs`) |
| D11 | in-memory overlay documents only (no files on disk, the LSP's unsaved-buffer path) | the same kinds map from the same paths; a whole package can index from overlays alone | `snapshot_discovers_overlay_only_package_files` (`src/lint/engine.rs`) |
| D12 | an overlay path for an `<id>.update.toml` or `<id>.deleted.toml` marker | no document and no node: markers are consumed at flatten time and an unsaved marker is not lintable on its own | `overlay_marker_paths_are_not_documents` |

## 2. Failure isolation: broken input degrades one node, not the index

The invariant the LSP silently depends on: a half-broken package must still
index, or every feature dies while the author is mid-keystroke.

| # | When one file... | Then... | Coverage |
|---|---|---|---|
| I1 | fails to parse as TOML | that file gets no node and a parse diagnostic; sibling files keep their nodes untouched | `a_broken_file_never_drops_sibling_nodes` |
| I2 | parses but a field has the wrong shape (`type = 5`, missing `[resolve]`) | the node survives with typed error states (`TypeSourceNode::Invalid`, `ResolveNode::Missing`) instead of vanishing | `parseable_files_with_wrong_shapes_keep_their_nodes_with_error_states` |
| I3 | is an enum members file whose `members` is not an array | the `EnumMembersNode` survives with `ProjectField::Invalid`, carrying the location for diagnostics | `parseable_files_with_wrong_shapes_keep_their_nodes_with_error_states` |
| I4 | is a catalog schema that is not valid JSON | the `CatalogNode` survives with `json: None`, no validator, and a syntax-stage diagnostic points at the file | `parseable_files_with_wrong_shapes_keep_their_nodes_with_error_states` |
| I5 | is a catalog schema that parses but does not compile as JSON Schema | the node keeps the JSON and records the compile failure in `invalid_message` | `parseable_files_with_wrong_shapes_keep_their_nodes_with_error_states` |
| I6 | is a broken variable or Lua file in a real fixture package | the failure surfaces as a source-backed diagnostic on the right path, from the index-backed pipeline | `snapshot_records_source_backed_failure_diagnostics` (`src/lint/engine.rs`) |

## 3. Location fidelity: nodes point where they were declared

Ranges are what diagnostics squiggles, hover, and go-to-definition render.
Positions are zero-based lines and characters.

| # | Given... | Then... | Coverage |
|---|---|---|---|
| L1 | a variable file | the node's location names the file; the `type` field's location starts on the declaring line | `node_and_field_locations_span_their_declaring_lines` |
| L2 | a `[resolve]` block with a default and a rule | the default and the rule's `when` each carry a range starting on their own line | `node_and_field_locations_span_their_declaring_lines` |
| L3 | catalog entries, samples, and enum members files | each node's location names its own file, and projected fields carry ranges | `node_and_field_locations_span_their_declaring_lines` |
| L4 | a diagnostic produced from an indexed expression reference | the diagnostic's range covers exactly the reference span (line and character precise) | `snapshot_diagnostic_ranges_cover_references` (`src/lint/engine.rs`) |
| L5 | an overlay document with a version | diagnostics computed from the index carry the overlay's document version, and ranges are correct against the overlay text, not the disk text | `snapshot_lints_overlay_without_writing_to_disk_and_groups_empty_documents` (`src/lint/engine.rs`) |

## 4. Target addressing: one grammar for nodes and custom lint

Custom lint rules select what they check with logical addresses like
`/catalogs/<id>/entries/<key>`. Those addresses and the index's node targets
must stay one grammar.

| # | Given / When | Then | Coverage |
|---|---|---|---|
| T1 | every supported address form | parsing yields the expected `RegisteredLintAddress`, and rendering the parsed address returns the input string byte for byte | `registered_lint_addresses_round_trip_between_grammar_and_parse` (`src/lint/custom/registry.rs`) |
| T2 | an unsupported address (no leading slash, unknown root, too many segments, non-numeric rule index) | registration is rejected with `rototo/custom-lint-registration-invalid` | `unsupported_lint_addresses_are_rejected` (`src/lint/custom/registry.rs`) |
| T3 | a trailing slash on an address | it normalizes away instead of reading as an empty segment | `trailing_slashes_normalize_in_lint_addresses` (`src/lint/custom/registry.rs`) |
| T4 | each node kind | `target()` and `field_target()` produce the `SemanticEntity` carrying the node's own id | `every_package_file_kind_projects_to_exactly_one_node` (spot-checked per kind) |

Design note, recorded for the review pass: a namespaced id contains `/`, which
the address grammar reads as a path separator, so `/variables/acme/in_trial`
is rejected and a namespaced variable cannot be targeted by custom lint at
all. Worse, a variable literally named `acme/values` would collide with the
`/variables/<id>/values` form if the grammar were naively widened. Today's
behavior (rejection) is pinned by `unsupported_lint_addresses_are_rejected`;
whether namespaced entities should be addressable, and with what escaping, is
an open design question.

## 5. Determinism

| # | Given / When | Then | Coverage |
|---|---|---|---|
| Det1 | the same package linted twice in separate pipelines | the projected semantic model serializes byte-identically: discovery order, node maps, references, and locations are all stable | `semantic_model_projection_is_deterministic_across_runs` (`tests/semantic_model.rs`) |
| Det2 | any index build | map iteration order is sorted by id by construction (`BTreeMap` throughout, sorted directory listings in discovery) | by construction; Det1 is the observable check |

## 6. Consumer consistency: one snapshot, all features agree

The LSP foundation claim, expressed as index properties. If two features
disagree about what exists, the bug is here.

| # | Invariant | Coverage |
|---|---|---|
| CC1 | Every resolved reference edge points at an entity the index actually holds: variables, catalogs, catalog entries, and allocations named by edges exist in their maps | `resolved_reference_edges_agree_with_the_index_and_definition` |
| CC2 | The declaration map and the per-edge resolution agree: a resolved edge's target has a declaration location | `resolved_reference_edges_agree_with_the_index_and_definition` |
| CC3 | Go-to-definition at any resolved reference site returns a definition: the walker and the definition provider read the same snapshot | `resolved_reference_edges_agree_with_the_index_and_definition` |
| CC4 | The reference index records resolved and unresolved edges distinctly, with unresolved edges kept (they drive `variable-rule-unknown-variable` and friends) | `snapshot_reference_index_records_resolved_and_unresolved_edges` (`src/lint/engine.rs`) |
| CC5 | The public semantic model projects exactly the index's entity sets: variable ids, catalog ids, evaluation context ids, entry counts, and linter files match one for one | `index_agrees_with_the_projected_semantic_model_and_symbols` |
| CC6 | Document symbols for every indexed variable file are rooted at the variable's id: the symbol tree and the index describe the same file | `index_agrees_with_the_projected_semantic_model_and_symbols` |
| CC7 | The diagnostics catalog is index-backed: built-in rules globally, plus the custom rules the index registered for this package | `diagnostic_catalog_entries` tests in `tests/diagnostics.rs` |

## 7. Projection to the public semantic model

`tests/semantic_model.rs` owns the public shape (`package_semantic_model`):
version pinning, entity fields, locations with ranges, references, and JSON
stability. Those tests are the public half of CC5 and are inventoried there
rather than duplicated here.

## Current gap tally

0 GAP rows. One recorded design question: custom-lint addressing of
namespaced ids (section 4 note).

When you add or change index behavior, add the row and the test together; an
empty Coverage cell is a regression in this file's contract.
