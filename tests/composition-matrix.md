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

## 1. Overlay over one base: merge semantics (no governance)

Given a base package and one overlay whose `extends` names it.

### Variables

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| V1 | restates a base variable with a bare `[resolve]` block | the resolve block swaps atomically; type and description are inherited from the base. design: overrides move to an explicit `<id>.update.toml` marker so a plain `<id>.toml` is always an add and an override announces itself; variables get no deleted marker (task #36) | `overlay_composes_membership_values_and_additions` |
| V2 | adds a new namespaced variable (`variables/acme/in_trial.toml`) | the variable is discovered recursively as `acme/in_trial` and resolves | `overlay_composes_membership_values_and_additions` |
| V3 | declares a different `type` for a base variable | the load fails naming both types | `overlay_cannot_change_a_variable_type` |
| V4 | restates the same `type` | the load succeeds; restating is agreement. design: direction reversed, an overlay variable file should carry only the fields it overrides, and repeating an immutable field should be an error even when identical (task #36) | `overlay_cannot_change_a_variable_type` |
| V5 | wins a variable's `[resolve]` | the resolution trace's `provenance` names the overlay | `trace_provenance_names_the_layer_that_owns_the_resolution` |

### Catalog entries

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| C1 | adds a new entry file | the entry lands next to the base's entries | `overlay_composes_membership_values_and_additions` |
| C2 | patches an entry (`<entry>.patch.toml`) with scalars and a nested table | patched scalars replace, nested tables recurse, unpatched fields are inherited | `overlay_composes_membership_values_and_additions` |
| C3 | patches a field whose value is an array | the array replaces the base array wholesale, no concatenation | GAP |
| C4 | writes `<entry>.deleted.toml` for a base entry | the entry is gone from the projection and references to it become `rototo/variable-unknown-value` lint failures | `overlay_deleted_marker_removes_the_base_entry` |
| C5 | writes a deleted marker for an entry no layer below provides | the load fails: "deleted marker has no catalog entry to remove" | `orphan_deleted_markers_and_patches_fail_loudly` |
| C6 | writes a patch for an entry no layer below provides | the load fails: "patch has no catalog entry to override" | `orphan_deleted_markers_and_patches_fail_loudly` |
| C7 | provides an entry and its deleted marker in the same layer | the load fails naming both files | `same_layer_entry_and_deleted_marker_conflict` |
| C8 | provides a patch and a deleted marker for one entry in the same layer | undefined today: `reject_same_layer_entry` only guards against the entry file itself, so both markers apply in directory order. design: this should fail the load like C7 | GAP |
| C9 | patches an entry a layer below already patched (chain of three packages) | patches apply bottom-up; the top patch sees the middle patch's result | GAP (no depth-3 chain test at all, see D1) |

### Enums

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| E1 | provides `data/enums/<id>.toml` with `members` for a base enum | the member sets union | `overlay_enum_members_union_with_the_base` |
| E2 | provides `deleted = [...]` next to `members` | the fold is (below + members) - deleted, and `deleted` never lands in the flattened file | `overlay_deletes_enum_members_from_the_base` |
| E3 | deletes a member no layer below declares | the load fails | `orphan_enum_member_deletes_fail_loudly` |
| E4 | adds and deletes the same member in one layer | the load fails | `same_layer_enum_member_add_and_delete_conflict` |
| E5 | deletes every remaining member | the load fails; an enum cannot compose to empty | `deleting_every_enum_member_fails_the_load` |
| E6 | restates the base's `model/enums/<id>.toml` with a different `type` | undefined today: the file overwrites and existing members may stop matching. design: should enum declarations get the V3 treatment (type change is an error, restating is legal)? | GAP |

### Contracts, samples, layers, lint (ungoverned overlay)

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| S1 | replaces a base catalog schema | the file overwrites, and base entries must validate against the new schema or lint fails | GAP |
| S2 | adds a sample file to a base evaluation context's `-samples/` directory | the sample validates against the base schema like any other | GAP |
| L1 | restates a base `layers/<id>.toml` with different buckets or unit | the overlay file wins wholesale. design: should bucket count or unit changes get a guard, since they reassign enrolled units silently? | GAP |
| X1 | adds a `lint/*.lua` rule | the rule runs against the whole composed package, base files included | GAP |

## 2. Governance: the base declares governance.toml

Given a base with a governance contract and an overlay extending it. The
grantable operations are add, update, delete, and override. `model/` files
are never grantable: schemas are narrowed with overlay custom lint, never
edited from above.

| # | When the overlay... | Then... | Coverage |
|---|---|---|---|
| G1 | adds a catalog entry under an `add` grant | the load succeeds | `governed_base_admits_the_granted_overlay` |
| G2 | deletes an entry listed in `delete_policy.denied_entries` | the load fails: "governance denies delete of entry ... " | `governed_base_denies_ungranted_operations` |
| G3 | deletes an entry the delete policy allows (`allowed_entries = ["*"]`, not denied) | the load succeeds and the entry is gone | GAP |
| G4 | patches a field in `update_policy.allowed_fields` on a permitted entry | the load succeeds with the patched value | GAP |
| G5 | patches a field outside `allowed_fields` | the load fails naming the field | `governed_base_denies_ungranted_operations` |
| G6 | patches an allowed field on an entry in `update_policy.denied_entries` | the load fails naming the entry (deny wins over the field allowlist) | GAP |
| G7 | restates a whole entry file the base owns | the load fails and points at `<entry>.patch.toml` / `<entry>.deleted.toml` | `governed_base_denies_ungranted_operations` |
| G8 | touches a base catalog schema | always denied; the error points at custom lint under `lint/` | `governed_base_denies_ungranted_operations` |
| G9 | touches a base enum declaration (`model/enums/<id>.toml`) | always denied, same custom-lint pointer | GAP |
| G10 | touches a base evaluation context schema | always denied, same custom-lint pointer | GAP |
| G11a | restates a base sample file | denied: "add a new sample file instead" | GAP |
| G11b | adds a new sample file for a base context | allowed without any grant | GAP |
| G12 | provides `data/enums/<id>.toml` where the base already has one | checked as `update` on `enum.<id>` | GAP |
| G13 | provides `data/enums/<id>.toml` where the base declares the enum but has no members file | checked as `add` on `enum.<id>` | GAP |
| G14 | restates a base variable's `[resolve]` under an `override` grant | the load succeeds and the overlay resolution wins | `governed_base_admits_the_granted_overlay` |
| G15 | restates a base variable with no `override` grant | the load fails: "governance denies override on variable.<id>" | `governed_base_denies_ungranted_operations` |
| G16 | adds a brand-new variable id | always allowed; new ids mint freely | `governed_base_denies_ungranted_operations` |
| G17 | restates a base namespaced variable (`variables/acme/foo.toml`) | the governance target is `variable."acme/foo"` (path separators become `/`) | GAP |
| G18 | restates a base `layers/<id>.toml` | checked as `override` on `layer.<id>` | GAP |
| G19 | restates a `lint/*.lua` file the base owns | the load fails: replacing a base lint file is not modeled | GAP |
| G20 | introduces its own new catalog (schema + entries) | ungoverned; a catalog the overlay declares is its own to fill | GAP |
| G21 | grants its own sub-overlays an operation the base never granted it | the load fails: "governance grant exceeds the inherited ceiling" | `governance_grants_cannot_exceed_the_inherited_ceiling` |
| G22 | re-grants a subset with more denies | narrowing is legal; the load succeeds | `governance_grants_cannot_exceed_the_inherited_ceiling` |
| G23 | carries a governance.toml that does not parse | the load fails at enforcement time: "failed to parse the overlay governance.toml" | GAP |
| G24 | (via diamond ancestry) restates a governed base file byte-identically | not treated as a governed update; the load succeeds | `diamond_ancestry_composes_the_shared_base_once` |

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
| B7 | one provides an entry, the other a patch for it | the load fails on the shared entry key (same mechanism as B6, untested spelling) | GAP |
| B8 | carry diverging schemas for the same catalog | the load fails on "catalog <id> schema" | GAP |
| B9 | one declares `model/enums/<id>.toml`, the other provides `data/enums/<id>.toml` | conflicts today: both files map to one "enum <id>" key. design: members are additive between overlay and base (E1), so should sibling member sets union too? | GAP |
| B10 | each add a different sample for the same evaluation context | conflicts today: samples share the context's key. design: samples are additive under an overlay (S2, G11b), so siblings sharing a context's sample directory is arguably B4, not B2 | GAP |
| B11 | declare the same experimentation layer id | the load fails on "layer <id>" | GAP |
| B12 | carry the same non-entity file (for example `lint/checks.lua`) with different content | the load fails per file path | GAP |

## 4. Depth: chains longer than two

| # | Given / When | Then | Coverage |
|---|---|---|---|
| D1 | A chain of three (app extends mid, mid extends core), each contributing to one catalog and one variable | markers and patches apply bottom-up; the app sees mid's edits to core before its own | GAP |
| D2 | A base that is itself a flattened projection with a provenance sidecar | the finer per-variable labels from the sidecar win over the layer's single label in traces | GAP |
| D3 | Governance declared at the bottom of a three-deep chain | the contract binds the top overlay, not just the immediate child | GAP |

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
- Catalog patch merge is associative over a chain: patch(patch(e, p1), p2)
  equals applying the flattened composition of p1 then p2.

## Current gap tally

25 GAP rows. The clusters, in the order I would close them:

1. Governance kind sweep (G9, G10, G11a/b, G12, G13, G17, G18, G19, G20,
   G23): one test per row, all cheap, all exercising real dispatch arms in
   `check_governed_file` that today have zero coverage.
2. Governance granted-path sweep (G3, G4, G6): the PLANS_GOVERNANCE fixture
   already encodes these policies; no test walks the allowed side of update
   or delete, nor deny-wins on entries.
3. Sibling symmetry (B7, B8, B11, B12): same guard, untested keys.
4. Depth (D1, D2, D3, C9): no test composes more than two authored layers.
5. Merge details (C3, C8, S1, S2, X1, L1).
6. Design questions before testing (E6, C8, B9, B10, L1): decide the
   behavior first, then pin it. C8 is the sharpest: a same-layer patch plus
   deleted marker is accepted today and the result depends on file
   application order.
