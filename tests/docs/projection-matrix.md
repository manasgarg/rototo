# Package projection test matrix

`rototo package` (`src/pack.rs`) turns a package source into a distributable
artifact: a deterministic `.tar.gz` for object stores, or with `--unpacked`,
the same flattened projection as a plain directory. This file inventories
those promises, in the same form as `tests/docs/composition-matrix.md`. Unless a
row says otherwise, tests are the inline suite in `src/pack.rs`.

What flattening does to `extends` semantics (markers consumed, governance
enforced, sibling conflicts) is the composition matrix's business; these
rows are about the artifact.

## 1. The archive

| # | Given / When | Then | Coverage |
|---|---|---|---|
| A1 | packing the same source twice | byte-identical archives with the same `sha256:` content-addressed name: safe to cache hard, safe to promote | `pack_package_is_deterministic_and_content_addressed` (`src/pack.rs`), `package_writes_a_deterministic_content_addressed_archive` (`tests/cli.rs`) |
| A2 | a source with `extends` | the packed manifest carries no `extends`: the archive is self-contained by construction | `pack_package_strips_extends_from_the_manifest` |
| A3 | a source that fails lint | packing is refused: an artifact that would fail its consumers never gets built | `pack_package_rejects_lint_failures` |
| A4 | a source with custom lint under `lint/` | the artifact leaves `lint/` out, in both encodings: custom lint is a review-time gate already enforced by A3, not something the runtime reads | `pack_package_leaves_custom_lint_out_of_the_archive`, `project_package_leaves_custom_lint_out_of_the_projection` |

## 2. The unpacked projection

| # | Given / When | Then | Coverage |
|---|---|---|---|
| U1 | `--unpacked <dir>` on a composed source | the flattened tree is written: markers consumed, parent files merged, `extends` stripped from the manifest | `project_package_writes_the_flattened_tree` (`src/pack.rs`), `package_unpacked_writes_the_flattened_projection` (`tests/cli.rs`) |
| U2 | a non-empty target directory | refused: the projection never writes over existing files | `project_package_refuses_a_non_empty_target` (`src/pack.rs`), `package_unpacked_refuses_a_non_empty_target` (`tests/cli.rs`) |
| U3 | the same source packed and projected | the archive's entries are exactly the projection's files with identical bytes: two encodings of one artifact | `unpacked_projection_matches_the_archive_contents` (`tests/sdk.rs`) |
| U4 | the JSON output | `UnpackedOutput` camelCase shape (package, directory, files); quiet mode prints the target path | `package_unpacked_writes_the_flattened_projection` (`tests/cli.rs`) |

## 3. Meaning is preserved

| # | Given / When | Then | Coverage |
|---|---|---|---|
| M1 | loading the projection instead of the composed source | every variable resolves to the same value (or the same error): flattening is a change of representation, never of meaning | `projected_package_resolves_identically_to_the_composed_source` (`tests/sdk.rs`) |
| M2 | a composed source | the provenance sidecar (`.rototo-provenance.json`) travels in both the archive and the projection (U3 compares the full file set), and a package extending the projection gets the finer per-variable trace labels from it | U3 for presence; `a_three_deep_chain_composes_bottom_up` (`tests/composition.rs`, composition matrix D2) for consumption |
| M3 | the projected package as a base for further `extends` | it composes like any other package (the flattened-projection-as-base shape) | composition matrix D2 |

## Current gap tally

0 GAP rows.

When you change what the artifact contains or how it is written, add the
row and the test together; an empty Coverage cell is a regression in this
file's contract.
