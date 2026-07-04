# Composition and governance test matrix

This is the executable inventory for package composition: every behavior the
`extends` machinery promises, written as a Given / When / Then row, with the
test that proves it. The point of the file is the Coverage column. A row that
says GAP is a promise we currently keep only by reading the code.

How to use it:

- When you add or change composition behavior, add or update the row first,
  then make the Coverage column true.
- When you write a new composition test, name the row it covers.
- A GAP row is not necessarily a bug. It is an untested claim. Some GAP rows
  below also carry an open design question, marked with "design:".

Unless a row says otherwise, tests live in `tests/composition.rs`. Governance
lint rules (as opposed to compose-time enforcement) are covered in
`tests/package_lint.rs` and are out of scope here except where noted.

The enforcement source of truth is `src/source/layer.rs`
(`check_governed_file`, `sibling_entity_key`, `SiblingBases::admit`) and
`src/source/governance.rs`.

## 1. Overlay over one base: merge semantics

Given a base package that grants the operation (in the tests, a broad
`[defaults] allowed_operations = ["add", "update", "delete"]` contract) and
one overlay whose `extends` names it. Deny-by-default is unconditional, so
these rows are about what the operations *do* once permitted; section 2
covers who may perform them.

### Variables

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| V1 | writes `variables/<id>.update.toml` with a `[resolve]` block for a base variable | the resolve block swaps atomically; type and description stay with the base; the marker never lands in the projection | `overlay_composes_membership_values_and_additions` |
| V2 | adds a new namespaced variable (`variables/acme/in_trial.toml`) | the variable is discovered recursively as `acme/in_trial` and resolves | `overlay_composes_membership_values_and_additions` |
| V3 | restates a base variable's plain `<id>.toml` with different content, at any namespace depth | the load fails and points at `variables/<id>.update.toml`, with the namespaced id in the path | `variable_restatement_requires_the_update_marker` |
| V4 | restates a base variable's file byte-identically | the restatement composes as a no-op (diamond ancestry shape) | `byte_identical_variable_restatement_is_a_noop` |
| V5 | wins a variable's `[resolve]` through the marker | the resolution trace's `provenance` names the overlay | `trace_provenance_names_the_layer_that_owns_the_resolution` |
| V6 | puts any key other than `resolve` or `description` in an update marker, even restating the base's exact `type` | the load fails: "a variable update may only update [resolve] and description" | `variable_update_may_only_carry_resolve_and_description` |
| V7 | writes an update marker for a variable no base declares | the load fails: "variable update has no base variable to update" | `orphan_variable_updates_fail_loudly` |
| V8 | provides both `<id>.toml` and `<id>.update.toml` in one layer | the load fails; the package is contradicting itself | `same_layer_variable_add_and_update_conflict` |

There is deliberately no `variables/<id>.deleted.toml`: an overlay never
removes a base variable, it can only update its resolution. Removal is the
base's decision.

### Catalog entries

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| C1 | adds a new entry file | the entry lands next to the base's entries | `overlay_composes_membership_values_and_additions` |
| C2 | updates an entry (`<entry>.update.toml`) with scalars and a nested table | updated scalars replace, nested tables recurse, unmentioned fields are inherited | `overlay_composes_membership_values_and_additions` |
| C3 | updates a field whose value is an array | the array replaces the base array wholesale, no concatenation | `catalog_update_replaces_arrays_wholesale` |
| C4 | writes `<entry>.deleted.toml` for a base entry | the entry is gone from the projection and references to it become `rototo/variable-unknown-value` lint failures | `overlay_deleted_marker_removes_the_base_entry` |
| C5 | writes a deleted marker for an entry no layer below provides | the load fails: "deleted marker has no catalog entry to remove" | `orphan_deleted_and_update_markers_fail_loudly` |
| C6 | writes an update marker for an entry no layer below provides | the load fails: "update has no catalog entry to update" | `orphan_deleted_and_update_markers_fail_loudly` |
| C7 | provides an entry and its deleted marker in the same layer | the load fails naming both files | `same_layer_entry_and_deleted_marker_conflict` |
| C8 | provides an update marker and a deleted marker for one entry in the same layer | the load fails: updating and removing an entry are contradictory | `same_layer_update_and_deleted_marker_conflict` |
| C9 | updates an entry a layer below already updated (chain of three packages) | updates apply bottom-up; the top update sees the middle update's result | `a_three_deep_chain_composes_bottom_up` |

### Enums

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| E1 | provides `data/enums/<id>.toml` with `members` for a base enum | the member sets union | `overlay_enum_members_union_with_the_base` |
| E2 | provides `deleted = [...]` next to `members` | the fold is (below + members) - deleted, and `deleted` never lands in the flattened file | `overlay_deletes_enum_members_from_the_base` |
| E3 | deletes a member no layer below declares | the load fails | `orphan_enum_member_deletes_fail_loudly` |
| E4 | adds and deletes the same member in one layer | the load fails | `same_layer_enum_member_add_and_delete_conflict` |
| E5 | deletes every remaining member | the load fails; an enum cannot compose to empty | `deleting_every_enum_member_fails_the_load` |
| E6 | restates the base's `model/enums/<id>.toml` at all (byte-identical excepted) | the load fails: `model/` files are never editable from above, so an enum's type cannot change | `governed_model_files_are_never_editable` |

### Contracts, samples, layers, lint (ungoverned overlay)

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| S1 | replaces a base catalog schema | the load fails unconditionally (G8); there is no ungoverned schema replacement | `governed_base_denies_ungranted_operations` |
| S2 | adds a sample file to a base evaluation context's `-samples/` directory | allowed without any grant; the sample validates against the base schema like any other | `governed_samples_reject_edits_but_admit_additions` |
| L1 | restates a base `layers/<id>.toml` with different buckets or unit | the load fails unless the contract grants `update` on `layer.<id>`; reassigning enrolled units is exactly what the grant gates | `governed_layer_updates_need_the_update_grant` |
| X1 | adds a `lint/*.lua` rule | the rule runs against the whole composed package, base files included | `overlay_lint_rules_run_against_the_composed_package` |

## 2. Governance

Deny by default, unconditionally: an overlay may always mint new ids, but
modifying anything a base declared needs a grant, and a base without a
`governance.toml` grants nothing - the file is where grants live, not a
switch. The grantable operations are add, update, and delete; the retired
names `constrain` and `override` are not accepted. A `[defaults]` block
grants across all base-declared entities, per-entity blocks refine it, and
deny wins from either level. `model/` files are never grantable: schemas are
narrowed with overlay custom lint, never edited from above. Governance runs
between a child and its bases only - sibling bases in one extends list are
not overlays of each other, so cross-sibling touching is a conflict (section
3), not a governed operation.

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| G1 | adds a catalog entry under an `add` grant | the load succeeds | `governed_base_admits_the_granted_overlay` |
| G2 | deletes an entry listed in `delete_policy.denied_entries` | the load fails: "governance denies delete of entry ... " | `governed_base_denies_ungranted_operations` |
| G3 | deletes an entry the delete policy allows (`allowed_entries = ["*"]`, not denied) | the load succeeds and the entry is gone | `granted_deletes_and_updates_walk_the_allowed_side` |
| G4 | updates a field in `update_policy.allowed_fields` on a permitted entry | the load succeeds with the updated value | `granted_deletes_and_updates_walk_the_allowed_side` |
| G5 | updates a field outside `allowed_fields` | the load fails naming the field | `governed_base_denies_ungranted_operations` |
| G6 | updates an allowed field on an entry in `update_policy.denied_entries` | the load fails naming the entry (deny wins over the field allowlist) | `granted_deletes_and_updates_walk_the_allowed_side` |
| G7 | restates a whole entry file the base owns | the load fails and points at `<entry>.update.toml` / `<entry>.deleted.toml` | `governed_base_denies_ungranted_operations` |
| G8 | touches a base catalog schema | always denied; the error points at custom lint under `lint/` | `governed_base_denies_ungranted_operations` |
| G9 | touches a base enum declaration (`model/enums/<id>.toml`) | always denied, same custom-lint pointer | `governed_model_files_are_never_editable` |
| G10 | touches a base evaluation context schema | always denied, same custom-lint pointer | `governed_model_files_are_never_editable` |
| G11a | restates a base sample file | denied: "add a new sample file instead" | `governed_samples_reject_edits_but_admit_additions` |
| G11b | adds a new sample file for a base context | allowed without any grant | `governed_samples_reject_edits_but_admit_additions` |
| G12 | provides `data/enums/<id>.toml` where the base already has one | the load fails unless the contract grants `update` on `enum.<id>` | `governed_enum_members_check_update_and_add` |
| G13 | provides `data/enums/<id>.toml` where the base declares the enum but has no members file | the load fails unless the contract grants `add` on `enum.<id>` | `governed_enum_members_check_update_and_add` |
| G14 | updates a base variable through `<id>.update.toml` under an `update` grant | the load succeeds and the overlay resolution wins | `governed_base_admits_the_granted_overlay` |
| G15 | updates a base variable through `<id>.update.toml` with no `update` grant | the load fails: "governance denies update on variable.<id>" | `governed_base_denies_ungranted_operations` |
| G16 | adds a brand-new variable id | always allowed; new ids mint freely | `governed_base_denies_ungranted_operations` |
| G17 | updates a base namespaced variable (`variables/acme/foo.update.toml`) | the governance target is `variable."acme/foo"` (path separators become `/`) | `governed_namespaced_variable_targets` |
| G18 | restates a base `layers/<id>.toml` | the load fails unless the contract grants `update` on `layer.<id>` | `governed_layer_updates_need_the_update_grant` |
| G19 | restates a `lint/*.lua` file the base owns | the load fails: replacing a base lint file is not modeled | `governed_lint_files_cannot_be_replaced` |
| G20 | introduces its own new catalog (schema + entries) | ungoverned; a catalog the overlay declares is its own to fill | `overlay_minted_catalogs_are_ungoverned` |
| G21 | grants its own sub-overlays an operation the base never granted it | the load fails: "governance grant exceeds the inherited ceiling" | `governance_grants_cannot_exceed_the_inherited_ceiling` |
| G22 | re-grants a subset with more denies | narrowing is legal; the load succeeds | `governance_grants_cannot_exceed_the_inherited_ceiling` |
| G23 | carries a governance.toml that does not parse | the load fails at enforcement time: "failed to parse the overlay governance.toml" | `unparseable_overlay_governance_fails_the_load` |
| G24 | (via diamond ancestry) restates a governed base file byte-identically | not treated as a governed update; the load succeeds | `diamond_ancestry_composes_the_shared_base_once` |
| G25 | updates or deletes anything declared by a base with no governance.toml | the load fails: no contract means no grants; adding a new id still succeeds | `a_base_without_a_contract_denies_modification` |
| G26 | operates under a `[defaults]` grant with a per-entity deny | the defaults grant applies to every base-declared entity; the entity's denied_operations wins over it | `defaults_grants_yield_to_entity_denies` |
| G27 | declares its own `[defaults]` wider than the base's | the load fails: "[defaults] allows <op> but the base does not grant it as a default"; grants over entities the base does not declare stay free | `defaults_ceiling_is_enforced` |

The lint half of governance (parse-failed, shape, unknown-target,
unscoped-update, empty allowlist) is covered in `tests/package_lint.rs`.

## 3. Sibling bases: two packages in one extends list

Given `extends = ["../left", "../right"]` where neither base was authored as
an overlay of the other. The rule: siblings must be entity-disjoint, except
catalogs, which are shared at entry granularity, and except byte-identical
restatements, which is how diamond ancestry looks.

| # | When the siblings... | Then... | Coverage |
|---|---|---|---|
| B1 | declare disjoint variables | both compose and resolve | `extends_composes_disjoint_sibling_bases` |
| B2 | declare the same variable id with different content | the load fails naming both bases | `sibling_bases_conflict_on_the_same_file` |
| B3 | both restate a shared governed ancestor's files byte-identically (diamond) | the restatements skip; the app composes all three branches | `diamond_ancestry_composes_the_shared_base_once` |
| B4 | add distinct entries to a shared ancestor's catalog | both entries land; the catalog is shared additively | `sibling_bases_add_disjoint_entries_to_a_shared_catalog` |
| B5 | provide the same catalog entry with different content | the load fails on "catalog <id> entry <entry>" | `sibling_bases_conflict_on_the_same_catalog_entry` |
| B6 | one provides an entry, the other a deleted marker for it | the load fails on the shared entry key | `sibling_base_may_not_touch_another_siblings_catalog` |
| B7 | one provides an entry, the other an update marker for it | the load fails on the shared entry key | `sibling_base_may_not_update_another_siblings_entry` |
| B8 | carry diverging schemas for the same catalog | the load fails on "catalog <id> schema" | `sibling_bases_conflict_on_diverging_catalog_schemas` |
| B9 | one declares `model/enums/<id>.toml`, the other provides `data/enums/<id>.toml` | the load fails: the two halves of one enum belong to one owner, deliberately - member adjustment is the overlay relationship (E1), where governance gates it | `sibling_enum_declaration_and_members_conflict` |
| B10 | each add a different sample for the same evaluation context | samples compose additively like catalog entries: each sample file is its own key, distinct samples land, the same sample id with different content conflicts | `sibling_bases_add_disjoint_samples_to_a_shared_context` |
| B11 | declare the same experimentation layer id | the load fails on "layer <id>" | `sibling_bases_conflict_on_the_same_layer_id` |
| B12 | carry the same non-entity file (for example `lint/checks.lua`) with different content | the load fails per file path | `sibling_bases_conflict_on_the_same_lint_file` |

## 4. Depth: chains longer than two

| # | Given / When | Then | Coverage |
|---|---|---|---|
| D1 | A chain of three (app extends mid, mid extends core), each contributing to one catalog and one variable | markers and updates apply bottom-up; the app sees mid's edits to core before its own | `a_three_deep_chain_composes_bottom_up` |
| D2 | A base that is itself a flattened projection with a provenance sidecar | the finer per-variable labels from the sidecar win over the layer's single label in traces | `a_three_deep_chain_composes_bottom_up` |
| D3 | Governance declared at the bottom of a three-deep chain | the contract binds the top overlay, not just the immediate child | `governance_binds_through_a_three_deep_chain` |

## 5. Invariants better held by property tests

These are not rows; each quantifies over all inputs and belongs in a
`proptest` suite rather than example-based tests.

- Flattening is deterministic: composing the same sources twice yields
  byte-identical projections.
- Restating any file byte-identically anywhere in the graph never changes
  the projection or the outcome (no-op law; the diamond skip is one corner
  of it).
- For any pair of arm-claim maps, the diff detail counts satisfy
  claimed + released + reassigned = |buckets whose assignment differs|,
  and `allocation_arms_expanded` implies released = reassigned = 0.
- Diffing any package against itself reports no changes.
- Catalog update merge is associative over a chain: update(update(e, p1), p2)
  equals applying the flattened composition of p1 then p2.

## Current gap tally

0 GAP rows. Every promise in this file is enforced by a named test. When you
add or change composition behavior, add the row and the test together - an
empty Coverage cell is a regression in this file's contract, not a note.

Design questions resolved along the way, recorded so the reasoning survives:

- C8: a same-layer update marker plus deleted marker fails the load; the two
  markers contradict each other.
- E6/S1: the enum-declaration and schema-replacement questions dissolved when
  `model/` edits from above became unconditionally denied.
- B9: siblings may not split an enum's declaration and members between them;
  member adjustment belongs to the overlay relationship, where governance
  gates it.
- B10: sibling bases share a context's sample directory additively, one key
  per sample file, mirroring catalog entries.
- L1: layer restatement is gated by the `update` grant on `layer.<id>`;
  bucket reassignment is exactly what the grant is for.
