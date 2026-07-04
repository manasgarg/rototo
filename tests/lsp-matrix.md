# LSP test matrix

The language server (`src/lsp/`) is one thin protocol layer over the lint
snapshot: every feature (diagnostics, completion, hover, definition,
references, symbols) reads the same `PackageLintSnapshot` the CLI lint
builds. This file is the executable inventory of the server's promises, in
the same form as `tests/composition-matrix.md`. Unless a row says
otherwise, tests are the inline suite in `src/lsp/mod.rs`.

The foundation invariant, one snapshot feeds all features, is an index
property: its rows live in `tests/semantic-index-matrix.md` section 6
(consumer consistency) and are not re-proven per feature here.

## 1. Session lifecycle and transport

| # | Given / When | Then | Coverage |
|---|---|---|---|
| L1 | `initialize` | the response advertises exactly what the server implements: UTF-16 positions, incremental sync (kind 2), completion without resolve, hover, definition, references | `initialize_advertises_completion_provider` |
| L2 | a request that fails (for example, before initialize) | an error response returns and the session stays alive for later requests | `lsp_request_errors_do_not_stop_the_server` |
| L3 | `shutdown` then `exit` | shutdown answers null, exit ends the loop cleanly | `lsp_request_errors_do_not_stop_the_server` (its closing sequence) |
| L4 | `exit` with no `shutdown` first | the session ends with a protocol error, so a crash is distinguishable from an orderly close | `exit_before_shutdown_is_a_protocol_error` |
| L5 | message framing | oversized Content-Length is rejected before allocation; missing/malformed headers are rejected; valid framing reads | `read_message_rejects_oversized_content_length_before_allocation`, `read_message_rejects_missing_and_malformed_content_length`, `read_message_accepts_valid_framing` (`src/lsp/transport.rs`) |
| L6 | file URIs | percent-decoding handles special and multibyte bytes; bad encodings are rejected | `file_uri_paths_percent_decode_special_and_multibyte_bytes`, `file_uri_paths_reject_bad_percent_encoding_and_utf8` (`src/lsp/uri.rs`) |

## 2. Document sync

| # | Given / When | Then | Coverage |
|---|---|---|---|
| D1 | ranged `didChange` edits | the overlay applies incrementally | `applies_incremental_document_changes` |
| D2 | positions in multibyte text | offsets count UTF-16 code units, per the advertised encoding | `incremental_change_positions_count_utf16_code_units` |
| D3 | an unsaved buffer differing from disk | every feature answers from the overlay, and disk is never written | `lsp_diagnostics_use_unsaved_overlay_and_clear_by_document`, `snapshot_lints_overlay_without_writing_to_disk_and_groups_empty_documents` (`src/lint/engine.rs`) |
| D4 | `didClose` | the overlay drops and diagnostics recompute from disk | close handling inside `lsp_diagnostics_use_unsaved_overlay_and_clear_by_document` |
| D5 | repeated reads with unchanged overlays | the lint snapshot is cached and reused; an overlay change invalidates it | `lsp_snapshot_cache_reuses_until_overlays_change` |

## 3. Diagnostics

| # | Given / When | Then | Coverage |
|---|---|---|---|
| G1 | opening or editing a document with problems | diagnostics publish for that document and clear when the problem is fixed, per document | `lsp_diagnostics_use_unsaved_overlay_and_clear_by_document` |
| G2 | rapid successive edits | publication is asynchronous and a newer edit supersedes a stale in-flight computation; the editor never sees diagnostics for text it no longer has | `lsp_publishes_diagnostics_asynchronously_and_supersedes_stale_edits` |

## 4. Features over the snapshot

| # | Feature | Then | Coverage |
|---|---|---|---|
| F1 | document symbols | rooted at the entity id, children per section, from the snapshot index and unsaved overlays | `lsp_document_symbols_use_snapshot_index_and_unsaved_overlay` |
| F2 | completion | every cursor situation is a data-driven scenario under `tests/fixtures/lsp/scenarios/completion/` (26 scenarios: TOML field positions, resolve/rule blocks, `when` expression contexts, context paths, enum and catalog operands, env members, bucket arguments, functions, operators, partial `&&`/`||`, multibyte text, namespaced refs in unsaved buffers, query entry paths, custom lint fields) | `completion_scenarios` via `src/lsp/scenario.rs` |
| F3 | hover | entity and field documentation from the snapshot, overlays included, query expressions included | `lsp_hover_uses_snapshot_index_and_unsaved_overlays`, `lsp_query_expressions_use_snapshot_index_and_unsaved_overlays` |
| F4 | go-to-definition | reference sites resolve to declarations across files, overlays included | `lsp_definition_uses_snapshot_index_and_unsaved_overlays`; the walker/definition agreement is index matrix CC3 |
| F5 | find-references | declaration sites list their reference locations, with and without the declaration | `lsp_references_use_snapshot_index_and_unsaved_overlays` |

## 5. Concurrency and cancellation

| # | Given / When | Then | Coverage |
|---|---|---|---|
| C1 | several read requests in flight | answers may arrive in any order; each request gets its own correct response | `lsp_answers_concurrent_reads_in_any_order` |
| C2 | `$/cancelRequest` for an in-flight request | the request answers with the RequestCancelled code | `lsp_cancelled_request_returns_request_cancelled` |
| C3 | cancellation bookkeeping | cancellations are recorded distinctly from queued work; numeric and string ids never collide | `intake_records_cancellations_and_queues_other_messages`, `cancellations_distinguish_numeric_and_string_ids` (`src/lsp/server.rs`) |

## Explicitly not promised yet

From the LSP roadmap, deferred by decision, not gaps: type-aware argument
completion (completing enum members inside function arguments by declared
type), concurrent request *execution* beyond the current intake model, and
fully async diagnostics beyond the supersede behavior in G2. When one of
these lands, it gets rows here first.

## Current gap tally

0 GAP rows.

When you add or change server behavior, add the row and the test together;
a new completion context belongs in a scenario file, not a hand-rolled
test. An empty Coverage cell is a regression in this file's contract.
