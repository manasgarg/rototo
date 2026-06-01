# rototo `lint-rearch` — Code-Quality Review

## 1. Executive summary

The `lint-rearch` branch is a competent, well-factored re-architecture. The staged pipeline (`discover → parse → project → references → project/reference/value/graph/policy`) is a clean separation of concerns, the per-stage modules are small and cohesive, the builtins emit stable `rototo/<flat-id>` rule ids exactly as the project requires, and the table-driven canonical-rule fixture test (`canonical_rule_fixture_table_covers_every_rototo_rule`) is a genuinely good guardrail that forces a fixture per rule. The build is green and `just check` passes. The domain vocabulary is honored almost everywhere.

Against the user's four explicit goals, the verdict is mixed and honest:

**(i) No bugs — NOT met.** There is one reproducible **critical** panic: a single malformed TOML file containing a non-ASCII byte near the syntax error crashes the entire lint run (and, because the LSP re-lints on every keystroke and tears the server down on any handler error, the language server too). There is also a **critical** security hole: workspace-supplied `lint/*.lua` runs with the full `os`/`io`/`package` stdlib, so loading any untrusted remote workspace is remote code execution. Beyond these, a pervasive byte-vs-UTF-16 position bug mislocates every diagnostic/hover/definition on non-ASCII lines, several diagnostics point at the wrong span, the LSP corrupts its own protocol stream via stdout logging, any out-of-workspace request kills the server, and the human `variable get` and human diagnostic output silently drop data that the JSON path includes.

**(ii) Confidence to change — NOT met for the highest-risk code.** The most safety- and correctness-critical logic is exactly the least tested: runtime resolution semantics (`neq/in/not_in/gt/lt/lte`, qualifier ANDing, `_`-env fallback, the `bucket_value` FNV hash that pins every rollout cohort) have no result-asserting tests; span/position math, the Lua sandbox boundary, the LSP transport/dispatch loop, and the source-loading archive/subdir paths have no direct tests. The suite stays green through a panic and through an RCE-capable sandbox. Coverage is broad at the *rule-id presence* level but thin at the *value/span/message* level.

**(iii) Modular, well-named, easy to reason about — largely met.** Clusters are small and single-purpose. The main drags are dead scaffolding left over from the rearch (the `GateIndex` gating subsystem that is built, threaded, and tested but never consulted; the orphaned `src/lint/source/span.rs`; the dead public `Diagnostic` struct; `DiagnosticLocation::span`), duplicated logic (position math in two files, `declared_workspace_environments` in two files, context-schema resolution in `sdk.rs`/`resolve.rs`, custom-rule extraction in three places), and a few misleading signatures.

**(iv) Right data structures for scale — NOT met on the runtime hot path.** The SDK re-reads and re-parses every variable/qualifier TOML from disk on **every** `resolve_*` call (the documented per-request hot path), and several lint helpers are quadratic in workspace size (`external_value_keys`, `referenced_variable_value_keys`, per-variable manifest re-parse, qualifier-graph rebuilt 2×/run). None of this bites `examples/basic`, but all of it is a real cliff for large workspaces and for the per-keystroke LSP.

Net: the architecture is sound and worth keeping; it is not yet safe to ship. The critical panic, the Lua sandbox, the stdout/LSP corruption, and the out-of-workspace-request crash should be fixed before this is trusted to load untrusted or non-ASCII workspaces, and the resolution/bucket test gap should be closed before the resolution code is refactored further.

## 2. Top issues

| Severity | Category | File:lines | Summary |
|---|---|---|---|
| critical | security | `src/lua_lint.rs:55,144` | Custom `lint/*.lua` runs with full `os`/`io`/`package` stdlib → RCE from any untrusted workspace |
| critical | error-handling | `src/lint/syntax/diagnostics.rs:36-42` | TOML parse-error slices `text` by raw byte offsets → panic on non-char-boundary span; crashes lint + LSP |
| high | bug | `src/lint/source/line_index.rs:34-49` | LSP positions are byte offsets but protocol defaults to UTF-16; every range wrong on non-ASCII lines |
| high | security | `src/source.rs:610-697` | Archive extraction caps only the compressed download, not decompressed size/entry count → gzip-bomb DoS |
| high | security | `src/lua_lint.rs:44-50,133-165` | No instruction/time/memory limit on custom Lua → infinite loop hangs / OOMs the loader |
| high | error-handling | `src/lsp/server.rs:36-149` | Any handler error (incl. out-of-workspace request) propagates out of `serve()` and kills the server |
| high | bug | `src/main.rs:280,653-661` | `tracing` defaults to stdout; a warn during an LSP session corrupts the JSON-RPC stream |
| high | data-structure | `src/resolve.rs:181-249` / `src/workspace.rs:60-77` | Every `resolve_*` re-reads + re-parses variable/qualifier TOML from disk (documented per-request hot path) |
| high | test-coverage | `src/resolve.rs:345-403` | Resolution semantics (`neq/in/not_in/gt/lt/lte`, bucket, AND, `_`-fallback) and `bucket_value` hash have no result-asserting tests |
| medium | bug | `src/lint/references.rs:422-454` | Find-references on a variable env value resolves the Environment target (span-overlap shadowing) |
| medium | bug | `src/output.rs:267-293` | Human `variable get` omits external values that `--json` includes |
| medium | bug | `src/diagnostics.rs:501-509` | Catalog hardcodes `entity: variable` for every custom rule regardless of its real target |
| medium | bug | `src/lint/references.rs:423-435` | "Undeclared environment" diagnostic points at the rule value, not the environment name |
| medium | bug | `src/lint/symbols/completion.rs:20-35` | Completion ignores cursor position; returns the full undifferentiated symbol set everywhere |
| medium | bug | `src/lint/project/external_value.rs:12-39` | External value silently unwraps any single-key `{value: X}` object, making such values unrepresentable |
| medium | bug | `src/lint/stages/discover.rs:52-77` | Overlay-only (unsaved) files never discovered; an unsaved manifest blocks the whole workspace |
| medium | security | `src/source.rs:330,402-405,791-812` | Git-checked-out symlink in a `#ref:subdir` can point the staged root outside the checkout |
| medium | security | `src/source.rs:350-361` | `reqwest` default redirect policy allows https→http downgrade, defeating the no-`http://` rule |
| medium | architecture | `src/lint/index/gates.rs:1-47` | `GateIndex` is built, threaded, tested, but `is_blocked` has zero callers — gating is unimplemented |

## 3. Correctness & bugs

### 3.1 (critical) TOML parse-error diagnostic panics on a non-char-boundary span
`src/lint/syntax/diagnostics.rs:36-42`. `parse_error_end` does `document.text[start..bounded_end].find('\n')` with `start`/`end` taken from `toml_span::Error.span` and clamped only by `.min(len)`/`.max(start)`. toml-span can return an error span whose boundary lands inside a multi-byte UTF-8 char; the `str` index then panics. This was reproduced end-to-end: `rototo workspace lint` on a workspace with `ké y = 1` panics with `byte index 2 is not a char boundary; it is inside 'é'`. The same byte-slicing risk exists in `src/lint/syntax/location.rs` (`value_span`). **Impact:** one malformed non-ASCII file crashes the whole lint run; because the LSP re-lints per keystroke and any handler error tears the server down, typing a non-ASCII char into a broken file crashes the language server. **Fix:** snap `start`/`end` to char boundaries (`str::is_char_boundary` / `floor_char_boundary` / `text.get(start..end)`) before slicing, and add a multibyte parse-failure fixture.

### 3.2 (high) LSP positions are byte offsets, not UTF-16 code units
Root cause: `src/lint/source/line_index.rs:34-49` computes `character = offset - line_start` in **bytes**, and `offset_for_line_character` treats the inbound `character` as a byte offset. These `SourcePosition` values flow unchanged through `src/lsp/convert.rs:111-122` into `LspPosition`, and `src/lsp/protocol.rs:83-107` never advertises `positionEncoding`, so clients default to UTF-16 (LSP 3.17). Inbound hover/definition positions are mirror-mistreated and compared against byte ranges in `src/lint/symbols/common.rs:30-40`. This single defect was independently reported across `line_index`, `convert`, `protocol`, `symbols/common`, and `output.rs:491-501`. **Impact:** on any line with non-ASCII content before a span, every published diagnostic range, hover/definition/references range, document-symbol range, and the human `path:line:col` label is off by the byte-vs-code-unit delta; ASCII happens to work because byte == UTF-16 unit. **Fix (do both halves together):** advertise `positionEncoding: "utf-8"` in `initialize_result` and honor the client's `general.positionEncodings`, **or** convert byte offsets ↔ UTF-16 code units in `LineIndex`. Add a multibyte LSP/symbols test.

### 3.3 (medium) Find-references on a variable env value resolves the Environment target
`src/lint/references.rs:422-454`. For each env block two edges are pushed at the **same** span: an `Environment` edge using `block.value.location()` (423/433) and an `EnvironmentValue` edge using `value.location` (448). `target_at_position` sorts only by `(priority, span_size)`, both are priority 0 with equal span size, so insertion order wins and the Environment edge (pushed first) shadows the value edge. **Impact:** clicking `value = "..."` inside `[env.prod]` returns references to the *environment* across the workspace instead of the value; definition and references disagree for the same token. **Fix:** point the Environment edge at the `[env.<name>]` header span (`block.location` / the key span), not the value span; or give the value edge precedence. Add a references test clicking the value.

### 3.4 (medium) "Undeclared environment" diagnostic points at the rule value, not the env name
`src/lint/references.rs:423-435`. The `VariableEnvironment` edge feeding `VariableUnknownEnvironment` uses `block.value.location()` — the `value = "..."` field, not the environment name. The fixture `variable-unknown-environment` enshrines the wrong range (the `"control"` literal). **Impact:** the diagnostic highlights an unrelated value literal, actively misdirecting the fix. **Fix:** capture the environment-name key span in projection into a dedicated `EnvironmentBlockNode` field and use it; update the fixture.

### 3.5 (medium) Catalog reports every custom rule with `entity: variable`
`src/diagnostics.rs:501-509`. `DiagnosticCatalogEntry::from_custom` hardcodes `DiagnosticEntity::Variable`; `CustomRuleDefinition` carries no entity, and the real target lives in the Lua registration (workspace/qualifier/variable/value/schema). **Impact:** `rototo diagnostics list`/`get` and `catalog_for_workspace` mislabel a workspace- or schema-targeting custom rule as `variable`. **Fix:** carry the registered entity through to the catalog (or drop the field for custom entries); add a test asserting the rendered/JSON `entity` for a non-variable custom rule.

### 3.6 (medium) Completion ignores cursor position
`src/lint/symbols/completion.rs:20-35`. `completion_items(index, path)` takes no position; the LSP handler (`server.rs:261-281`) never reads `params.position`. It unconditionally concatenates all environments + all qualifiers + the file's values + 9 operators + 13 lint field selectors. **Impact:** completion inside `[values]` offers `json.`/`value.`/`bucket`; inside a predicate offers value keys — noisy and misleading everywhere. **Fix:** thread the position through and gate each group on the cursor's syntactic context.

### 3.7 (medium) External value silently unwraps any single-key `{value: X}` object
`src/lint/project/external_value.rs:12-39`. The file is treated as wrapped whenever the root is a table of `len == 1` with key `"value"`, storing the inner value. **Impact:** a schema-backed value whose intended JSON *is* `{"value": X}` is silently collapsed to `X`, with no escape syntax and no diagnostic, so it is unrepresentable and validates against the wrong shape. **Fix:** disambiguate the wrapper structurally (e.g. on document kind) so it cannot collide; add a fixture for a value whose content is `{"value": ...}`.

### 3.8 (medium) Overlay-only (unsaved) files are never discovered
`src/lint/stages/discover.rs:52-77` and `src/lint/source/discover.rs`. Discovery is entirely disk-driven; overlays are consulted only in `add_disk_document` *after* a path is enumerated from disk. **Impact:** a new unsaved qualifier/variable/schema is invisible to lint/symbols/hover, and an overlay-only `rototo-workspace.toml` makes discover emit "manifest is missing" and stop the whole workspace. **Fix:** after disk enumeration, also enumerate overlay keys and add overlay-only paths as synthetic documents of the inferred kind; add an overlay-only LSP/snapshot test.

### 3.9 (medium) Human `variable get` and human diagnostics drop data the JSON path keeps
- `src/output.rs:267-293`: JSON `variable get` merges external `<id>-values/*.toml` via `read_variable_toml`; the human branch prints only the main file. Directory-backed variables show an incomplete value picture in the default human output.
- `src/output.rs:478-501`: `print_diagnostics` never reads `diagnostic.related`, so qualifier-cycle and rule-shadowing secondary locations (`builtins/graph.rs:50,136`) are lost in human output but present in `--json`.
- `src/lsp/convert.rs:24-37`: `LspDiagnostic` has no `relatedInformation` field, so the same `related` data never reaches the editor.

**Fix:** render the merged value set in human `variable get`; print `related` as indented `note:` lines; add `relatedInformation` to `LspDiagnostic` and map `diagnostic.related`.

### 3.10 (medium) `tracing` writes to stdout, corrupting the LSP stream
`src/main.rs:280,457-459,653-661`. `init_tracing()` is called unconditionally and builds `tracing_subscriber::fmt()` with no `.with_writer(...)`, defaulting to stdout. The LSP writes protocol frames to stdout; default filter is `warn`, so any warn/error event during a session injects non-protocol bytes and desyncs Content-Length framing. **Fix:** `.with_writer(std::io::stderr)` (safe for both CLI and LSP).

### 3.11 (medium) Resolution on the un-linted CLI path degrades silently instead of erroring
`src/resolve.rs`. The CLI direct-resolve path uses `inspect_workspace` (no lint), so resolve is the only validation. Yet an unknown op falls to `_ => false` (358); a malformed `range` becomes `[0,0)` via `unwrap_or(0)` and is always false (321-328); an empty `[predicate]` array is vacuously true (286-292); a matching rule with a non-string `value`/`qualifier` is skipped instead of selected (210-228), violating "first matching rule wins". **Impact:** typos and malformed shapes silently produce wrong-but-plausible booleans / wrong value selection with no diagnostic. **Fix:** either lint before CLI resolution, or have `evaluate_predicate`/`resolve` return errors for unknown ops, non-2-element ranges, empty predicate lists, and matching-but-non-string rule values.

### 3.12 (low, grouped) Smaller correctness items
- `src/lint/builtins/graph.rs:65-74,244`: qualifier-cycle message joins an **alphabetically sorted** SCC with `->`, implying a traversal order that does not match the real edges.
- `src/lint/builtins/graph.rs:201-229` / `src/lint/builtins/qualifier.rs:86-100`: Tarjan SCC recurses unbounded (stack-overflow risk on a long qualifier chain); an unknown operator also triggers a redundant "predicate must contain value".
- `src/lint/references.rs:219,225-235` & `src/lint/builtins/graph.rs:76-97`: a qualifier referencing only itself is flagged as both self-cycle and unreferenced.
- `src/lint/builtins/variable.rs:430-486`: a duplicated inline+external value key is validated twice, producing redundant type/schema diagnostics on top of the duplicate diagnostic.
- `src/lint/builtins/schema.rs:132-148`: `context_schema_declares_path` only understands flat nested `properties`; `$ref`/`additionalProperties`/combinators produce false-positive "attribute not declared" failures (and reject `Workspace::load`).
- `src/lint/project/fields.rs:114-136`: a float bucket range yields the wrong of two diagnostics (range-bounds message instead of "must contain two integers").
- `src/lint/output.rs:13-31` & `src/model.rs:57-71`: `diagnostic_sort_key` packs a byte offset and a line number into the same tuple slot (safe only because no diagnostic uses the range-only constructor); `diagnostics_by_document` silently drops `doc=None` workspace-root diagnostics (manifest-missing / workspace-not-found).
- `src/lint/source/discover.rs:143-154`: directory symlinks named `*.toml`/`*.json` are enrolled as file documents, producing a confusing "failed to read" diagnostic.
- `src/lsp/server.rs:145,185-200`: `exit` before `shutdown` is ignored (LSP spec violation); `change_document` stores an incremental fragment as the full overlay if a client sends a `range`.
- `src/lsp/uri.rs:31-34`: `canonicalize_workspace_root`'s `unwrap_or(path)` fallback can break later `strip_prefix`, reporting every in-workspace file as "outside workspace".
- `src/resolve.rs:346-383,334-336`: `eq`/`in` compare `serde_json::Value` strictly while `gt`/`lt` coerce via `as_f64`, so int `100` ≠ float `100.0` for equality but `==` for ordering; a missing context path makes `neq`/`not_in` false (may surprise authors); large i64 lose precision through f64.
- `src/docs.rs:530-560,483-491`: HEAD responses report `Content-Length: 0`; `/cli.html/` 404s; a single `accept()` error exits the serve loop.

## 4. Test coverage & confidence to change

The suite is strong on *rule-id presence* (the canonical table forces a fixture per `rototo/<rule>`) but weak on *values, spans, and messages*, and has near-zero coverage of the runtime resolution semantics and the LSP plumbing. Concretely:

- **Resolution semantics are unverified.** `resolve.rs` has no `#[cfg(test)]`. Integration tests only assert `eq` on TRUE paths and only assert *presence* of qualifier ids in `resolve-all` JSON. `neq/in/not_in/gt/gte/lt/lte`, bucket evaluation, qualifier ANDing (one predicate false), qualifier-`<id>` indirection producing FALSE, the env-block-absent `→ "_"` fallback, and cycle detection have **no result-asserting test**. A refactor can invert any operator and stay green.
- **`bucket_value` has no golden test.** `src/resolve.rs:385-403` is the FNV-1a hash that pins every rollout cohort; nothing asserts a concrete output or even drives `op == "bucket"`. Any change to the separator byte, FNV constants, modulo, or `canonical_context_value` silently reshuffles every bucketed user on a green build.
- **Span/position math is untested.** No `#[test]` in `line_index.rs`, `syntax/location.rs`, `syntax/diagnostics.rs`; no non-ASCII/CRLF byte anywhere in `src/` or `tests/`. This is exactly why the critical panic shipped green.
- **LSP plumbing is untested.** No test drives `serve()`/`handle_message`, the transport framing (`read_message`/`write_message`/error responses), shutdown/exit, the `-32601` branch, percent-decode, or out-of-workspace handling. LSP tests call handlers directly and assert only substring contents or a single `start.line` — never exact start/end character ranges.
- **Source-loading is untested for its hardest paths.** No test for `stage_https_archive`, the `max_archive_bytes` cap, `extract_archive` size limits, `infer_archive_workspace_root`, `select_subdir` success/escape, `SourceUri::parse` fragment forms (`#ref:subdir`/`#:subdir`), or symlink-escape on schema/external-value/git paths.
- **Per-branch lint gaps.** Several distinct shape/reference arms have no fixture despite distinct messages: `type = <non-string>`, `env = <scalar>`, `rule = <scalar>` (`builtins/variable.rs`); the "workspace path is not a directory" discover branch; variable-schema-ref path-escape and non-schema-document branches. The SDK `lint_variable` entity-scoped attribution (value/rule-entity in sibling files) is only covered by the `|| path == path` fallback.
- **Catalog/diagnostics model has no unit tests.** `CustomRuleId::parse` edge cases, the `from_custom` entity bug, and invalid-severity dropping are only indirectly exercised.

**Highest-value tests to add (prioritized):**

Unit:
1. `resolve.rs`: assert TRUE and FALSE outcomes for every operator (`eq/neq/in/not_in/gt/gte/lt/lte`), bucket half-open `[start,end)` boundaries, qualifier-`<id>` indirection true/false, AND short-circuit, `_`-env fallback, and a two-qualifier cycle error.
2. `resolve.rs`: pin at least one golden `bucket_value("known-salt", json!("user-123"))` value plus a determinism check.
3. `line_index.rs`: position/offset round-trip, empty text, last-line/past-end clamp, and an explicit multibyte case (`é` = 2 bytes, `😀` = 4 bytes) documenting/locking the chosen encoding.
4. `syntax/diagnostics.rs`: a malformed TOML and a malformed JSON with a multibyte char adjacent to the error assert a diagnostic (not a panic) at the right range.
5. `source.rs`: `SourceUri::parse` for `#ref:subdir`/`#ref`/`#:subdir`/bare; `relative_path_is_safe`/`select_subdir` reject `..`/absolute/empty.
6. `path.rs`: `resolve_workspace_relative_path` / `resolve_workspace_root_path` for `..` pop-to-escape, absolute, empty, deep `..`.
7. `references.rs`: `target_at_position` tie-break over overlapping spans (priority + span size).
8. `lsp/uri.rs`: `percent_decode_path` (`%20`, multibyte, trailing `%`, non-hex) and `path_from_file_uri` scheme rejection.
9. `diagnostics.rs`: `CustomRuleId::parse` accept/reject; catalog `entity`/`severity` for a non-variable custom rule.

Integration:
1. Lua sandbox: a lint script calling `os.execute`/`io.open`/`require` asserts a clean failure (and, after the fix, that those globals are nil); a non-terminating handler is bounded; a file with no `register` global produces a diagnostic.
2. `serve()` round-trip over `tokio::io::duplex`: initialize/shutdown/exit, an unknown method → `-32601`, an out-of-workspace request → error response (not loop exit).
3. Archive: build a real `.tar.gz` and assert the download-size cap and (post-fix) the decompressed-size/entry cap trigger; assert single-subdir inference and the ambiguous-root error.
4. Overlay-only file (new variable and new manifest) → asserted diagnostics/symbols.
5. Exact start AND end line/character for at least one hover, one references, and several definition/document-symbol ranges, including a multibyte line.
6. Add `message` (exact or substring) to the canonical `ExpectedDiagnostic` for rules carrying dynamic content (unknown env/value/qualifier, type/schema mismatch).

## 5. Modularity, naming & readability

- **Dead `GateIndex` gating subsystem.** `src/lint/index/gates.rs:1-47` carries `#![allow(dead_code)]`; `block()` is called from `parse.rs:21` and `custom/registry.rs:29,54` and preserved across projection rebuild (`project.rs:8-10`), but `is_blocked` has **zero callers** and `blocked_at`/`diagnostic` are read only by `engine.rs` tests. Builtins already implicitly skip unparsed entities (they aren't in the `SemanticIndex`), so the gate changes no behavior. It misleads a reader into assuming downstream suppression. **Fix:** wire `is_blocked` into the checked stages (and add suppression tests), or delete the machinery and keep only snapshot metadata with a doc comment saying it does not gate.
- **Orphaned `src/lint/source/span.rs`.** Declared via `mod span;` but never used; defines a second `Spanned<T>` distinct from the live one in `index/nodes.rs:283`, hidden behind `#![allow(dead_code, unused_imports)]`. **Fix:** delete the module and the `mod span;` line.
- **Dead public `Diagnostic` struct** (`src/diagnostics.rs:733-767`) and **dead `DiagnosticLocation::span`** (`616-624`). `Diagnostic`/`Diagnostic::rototo`/`::custom` have no instantiation; `LintDiagnostic` is the live type. `span()` is the only producer of a Span-kind location with `span: None`, which is the lone reason `output.rs` needs a fallback sort branch. **Fix:** remove both; make `source_span` the single span constructor.
- **Duplicated logic.** Position math (`location_contains_position`, `source_range_size` with the magic `10_000`, etc.) is copied verbatim in `symbols/common.rs:19-49` and `references.rs:572-602`. `declared_workspace_environments` exists in both `builtins/workspace.rs:167` and `references.rs:561` (name shadowing). Context-schema resolution (`context_schema_path` + read/compile) is byte-identical in `sdk.rs:679-720` and `resolve.rs:132-179` — a security-relevant path-safety rule maintained twice. Manifest custom-rule extraction exists in three places (`workspace.rs`, `catalog.rs`, `project/mod.rs`). **Fix:** extract each to one shared helper.
- **`lua_lint` is a `pub mod` at the crate root** (`lib.rs:7`) exposing internal mlua types, consumed only by `lint::custom`. **Fix:** move under `lint::custom::lua` or make `pub(crate)`.
- **Misleading signatures / dead params.** `project_inline_values(_toml)`, `project_rule_from_table_like(_variable_id, _environment, invalid_shape: bool)` where `invalid_shape` is always `false` (`project/variable.rs:126-285`); the no-op `let _ = &schema.value;` in `lint_type_source` (`builtins/variable.rs:41-46`); `valid_context_schema`'s dead `contains_key`+`get`+`get` triple lookup with an unreachable "file not found" branch (`builtins/schema.rs:97-115`); the unreachable trailing `Err` in `insert_context_path` and the dead `merge_context` guard (`main.rs:550-579`). **Fix:** inline/collapse and drop the inert params.
- **Vocabulary leak (low).** `WorkspaceLint.documents: Vec<SourceDocumentSummary>` and `DocumentDiagnostics`/`SourceDocumentSummary` are public and serialized as a top-level `"documents"` key in `workspace lint --json`. CLAUDE.md allows `document` internally but lists it among generic nouns to avoid in the public SDK/CLI surface. **Fix:** rename to a domain term (`sources`/`SourceSummary`) or carve an explicit exception in CLAUDE.md.

## 6. Data structures & efficiency at scale

- **(high) SDK resolve re-reads disk every call.** `WorkspaceInspection` stores only ids/paths; `resolve_variable_with_state` → `read_variable_toml` (`workspace.rs:60`) and `QualifierState::resolve` → `read_toml` (`resolve.rs:280`) read + parse TOML (and scan the external-values dir via `merge_external_variable_values`) on **every** resolution. This is the documented per-request runtime hot path and the `RefreshingWorkspace` model. **Fix:** parse and cache variable/qualifier values (and merged external values, and a compiled context validator — `validate_context_schema` also recompiles the JSON Schema per call, `resolve.rs:132-164`) once at `load`/`inspect`; resolve against in-memory structures; refresh swaps the snapshot.
- **(medium) O(n²) helpers in lint and resolve, repeated per keystroke in the LSP.**
  - `external_value_keys` scans all documents per variable (`source/mod.rs:79-90`, called per variable from `project/variable.rs:86`).
  - `referenced_variable_value_keys` linearly scans the whole `value_referenced_by` map per variable (`references.rs:240-246`, called per variable from `graph.rs:155`).
  - `declared_workspace_environments` deep-clones and re-validates the entire manifest TOML **per variable** inside `add_variable_references` (`references.rs:421,561-570`).
  - `qualifier_reference_graph` is rebuilt (cloning every edge) at least twice per run; `referenced_qualifier_ids` rebuilds it internally (`references.rs:178-238`, `graph.rs:13,77`).
  - `target_at_position`/`reference_locations` do full edge scans per LSP request, with a nested `has_references` edge re-scan per Schema declaration (`references.rs:104-176,540-542`).
  - `document_symbols` rescans all qualifiers/variables/external values per single-file request (`document_symbols.rs:5-37`).
  - `qualifier_for_id`/`variable_for_id` are linear `Vec` scans called per id and per predicate hop, making deep `qualifier.<id>` chains O(n²) (`workspace.rs:109-129`).

  **Fix pattern:** build the by-key indices once (a `path → entity-ids` map, `BTreeMap<VariableId, BTreeSet<ValueKey>>`, an id→entity `HashMap`, a `Set<ReferenceTarget>` of targets-with-edges, and the qualifier graph) and share them; use `BTreeMap::range` over tuple-key prefixes instead of full scans.
- **(low) Redundant per-run allocations.** `ParsedToml::new` deep-clones every parsed TOML node into a `'static` owned tree on every lint (`syntax/mod.rs:83-119`), on top of `to_plain_toml()` conversions and the LSP's per-keystroke full re-lint. `sort_diagnostics`/`diagnostic_sort_key` allocate a fresh `String` per comparison via `rule.as_string()` (O(n log n) allocs, `output.rs:3-31`, `diagnostics.rs:459-466`); `ValuesNode.inline_keys` duplicates `inline_values.keys()` (`index/nodes.rs:158-164`); `lsp_diagnostic` allocates the rule string twice (`convert.rs:29-32`); `path_containment_error` re-canonicalizes the already-canonical root per document (`path.rs:4`). **Fix:** expose a borrowed `as_str()/Cow` for rule ids and sort with `sort_by_key`; drop `inline_keys`; reuse one rule string; cache the canonical root. The `Id` aliases (`= String`, `ids.rs:1-5`) provide no newtype safety — acceptable for the current slice, recorded as a tradeoff.

## 7. Other

### Security
- **(critical) Lua sandbox is wide open.** `Lua::new()` (`lua_lint.rs:55,144`) loads `StdLib::ALL_SAFE` which in non-luau mlua 0.10.5 **includes** IO/OS/PACKAGE (only FFI/DEBUG excluded). Workspace `lint/*.lua` can call `os.execute`, `io.open`, `os.getenv`, `require`. `Workspace::load(remote-source)` runs this automatically in the register stage before validation → RCE from any untrusted git/https workspace. **Fix:** `Lua::new_with(StdLib::TABLE | STRING | MATH | UTF8, LuaOptions::default())` (no OS/IO/PACKAGE), nil out dangerous globals, add a defense test.
- **(high) No Lua execution limits.** No `set_hook`, `set_memory_limit`, or task timeout (`lua_lint.rs:44-50,133-165`); `while true do end` hangs the loader / refresh worker indefinitely (DoS). **Fix:** instruction hook + memory limit + bounded `spawn_blocking`/timeout.
- **(high) Gzip/tar bomb.** Only the compressed download is capped; `extract_archive` (`source.rs:661-697`) has no decompressed-size or entry-count limit. **Fix:** track cumulative uncompressed bytes/entries and abort past a cap.
- **(medium) https→http redirect downgrade.** `reqwest::Client::builder()` uses the default follow-up-to-10 policy with no scheme check (`source.rs:350-361,446-458`), so a malicious https endpoint can 30x-redirect to http, defeating the no-`http://` rule and potentially leaking the bearer header. **Fix:** a redirect policy that rejects non-https hops.
- **(medium) Symlink escape via `#ref:subdir`.** `select_subdir` validates only the subdir *string*, then `root.join(subdir)` follows symlinks; a git checkout restores committed symlinks (the archive guard does not cover the git path), so a `#ref:rototo` where `rototo` → `/etc` points the staged root out of tree (`source.rs:330,791-812`). The SDK context-schema path (`sdk.rs:707-719`) similarly rejects `..`/absolute but follows symlinks. **Fix:** canonicalize the resolved target and assert `starts_with` the canonical staged root / workspace root.
- **(medium) `git ls-remote` option injection / env leak.** The ref fragment is passed positionally with no `--` separator and not rejected for a leading `-` (`source.rs:530-570,291-300`); `git_ls_remote` also omits the `GIT_*` env scrubbing that `rev-parse`/`checkout` apply. **Fix:** `git ls-remote <url> -- <ref>`, reject refs starting with `-` at parse time, scrub `GIT_*` consistently.
- **(low) Unbounded `Content-Length` allocation** in `read_message` (`transport.rs:31-39`) — cap before allocating.
- **(low) `content_hash_fingerprint`** uses 64-bit `DefaultHasher` (not a stability/collision contract) for refresh decisions (`source.rs:595-608`) — use a stable wide digest.

### Async / blocking
- **(medium) The whole sync CPU phase runs on the async runtime.** `parse::run` and `build_projection` (TOML/JSON parse, `jsonschema::validator_for` compile, per-value `validator.validate`) are plain sync calls in the async pipeline (`stages/mod.rs:36-62`), invoked per-keystroke in the LSP and per refresh tick. **Fix:** wrap the CPU phase in `spawn_blocking`.
- **(low) `std::fs::canonicalize` on the async path.** `workspace_relative_path` (`lsp/uri.rs:100-101`) blocks the executor on every LSP request; `initialize_workspace_root` already uses `tokio::fs`. **Fix:** make it async / `tokio::fs::canonicalize`.
- JSON Schema compile/validate in `sdk.rs:96-143` and `resolve.rs:159-163` likewise run sync on async; offload or document.

### API & vocabulary
- (medium) `RefreshOptions::max_staleness` / `RefreshStatus::stale()` are exposed but never consumed (`sdk.rs:366-437`) — wire them in or remove (the project's "no behavior, no API" rule).
- (medium) Catalog parses custom rules with a second TOML reader and **opposite** duplicate policy from the lint pipeline (`catalog.rs:26-103` keeps first via `.or_insert`; projection keeps last via `.collect()`), so `diagnostics list` can disagree with emitted diagnostics. The same first/last disagreement exists between `lint_custom_rule_conflicts` (keeps first) and the runtime registry (keeps last) at `workspace.rs:108-131`. **Fix:** derive the catalog from the projection and use one precedence.
- (low) `Workspace::inspect` (the documented non-lint loader) still hard-fails on a malformed/uncompilable context schema (`sdk.rs:64-110`) — defer to first use or document.
- (low) `file_uri` emits non-percent-encoded URIs while `path_from_file_uri` percent-decodes (`path.rs:76-78` vs `lsp/uri.rs:56-89`), so paths with spaces/`#`/`%` don't round-trip and editor jumps silently fail. **Fix:** percent-encode in `file_uri`.
- (low) `--quiet` prints warnings on a passing run (`output.rs:137-198`); lint pass/fail is hand-rolled three different ways across command arms (`main.rs:318,345-348,392-395`) — add `has_errors()` to `QualifierLint`/`VariableLint`. Public `resolve_*` free functions are undocumented (`resolve.rs:14-116`).

### Error handling
- (high) `serve()` propagates `?` from every handler with no per-message isolation (`lsp/server.rs:36-149`); a request for a file outside the workspace (editors send these for any open buffer) or any malformed message kills the whole server. **Fix:** for requests, reply with `write_error_response`; for notifications, log and continue; reserve `?` for transport failures.
- (medium) Output paths panic on broken pipe — `rototo ... | head` aborts with "Broken pipe" (`output.rs`); reset SIGPIPE or treat `BrokenPipe` as clean exit.
- (low) `RototoError` is an opaque single string with no kind and no `source()` chain (`error.rs:6-25`), so SDK consumers and the refresh failure-tracking logic can only string-match. **Fix:** add an error-kind enum or a boxed cause.
- (low) `consecutive_failures`/`last_error` and `last_success` are not reset/updated on an `Unchanged`/`Immutable` probe (`sdk.rs:491-564`), so a healthy-but-static source reports stale failures forever (and will trip false staleness once `stale()` is wired).
- (low) Several silent `continue` paths in custom-lint `runner.rs`/`registry.rs` drop registrations/targets with no diagnostic; `register_pipeline_lint_script` returns zero registrations silently when a script defines no `register` global (`lua_lint.rs:61-69`).

### Architecture & cohesion
- The staged pipeline and small per-stage modules are the strongest part of the rearch. The cohesion drags are the dead `GateIndex`/`span.rs`/`Diagnostic` scaffolding (§5), the duplicated position math and context-schema/environment helpers (§5), the empty `policy::run_builtin` stage that exists only to host custom Lua (`stages/policy.rs:1-3`), and the three+ parallel `Candidate` walkers (`HoverCandidate`/`DefinitionCandidate`/`ReferenceTargetCandidate`) that independently re-walk the same node tree with identical priority+span sorting (`hover.rs:34`, `definition.rs:42`, `references.rs:551`) — adding a referenceable field requires parallel edits that can silently drift. The five per-stage `push_*_diagnostic` wrappers are defensible (compile-time stage binding) but could be a macro.

### Docs
- `docs/src/api/diagnostics.md:17` says severity is "currently error", contradicting the Warning-severity rules (`qualifier-unreferenced`, `variable-rule-shadowed`, `variable-value-unused`) and tests asserting `"severity": "warning"`. **Fix:** state `error` or `warning` and note warnings don't fail lint.

## 8. Per-module health

- **lint-engine-pipeline:** Clean staged design; weighed down by dead `GateIndex`, an uncalled inconsistent `lint_workspace_until`, and an O(docs×diags) `diagnostics_by_document` that drops workspace-root diagnostics.
- **lint-index:** Solid node model; `inline_keys` duplicates `inline_values.keys()`, `Id` aliases are docs-only, `COMPLETION_LABELS` can drift from the operator enum with no guard.
- **lint-syntax-spans:** Correctness-critical and the weakest spot — a live panic on non-char-boundary spans plus end-to-end byte-vs-UTF-16 positions, and no unit tests on any of it.
- **lint-source-discovery:** Path-safety is mostly sound but the containment guard fails open on non-NotFound canonicalize errors, directory symlinks are mis-enrolled, and there are no unit tests.
- **lint-builtins-variable:** Good rule coverage; redundant double-validation of duplicate keys, flat-`properties`-only context-schema walker, several untested shape arms.
- **lint-builtins-qualifier-graph:** Correct results; misleading cycle-path message, unbounded Tarjan recursion, double-emitted unknown-op diagnostic, and the qualifier graph rebuilt 2×/run.
- **lint-project:** Reasonable projection; the silent `{value: X}` unwrap is a real correctness gap, manifest-environment validation bypasses the projection, and there are no direct unit tests.
- **lint-custom-lua:** Functionally works but is the security epicenter — open sandbox, no execution limits, full-script recompile per target.
- **lint-references:** Rich reverse-index model but several quadratic queries, a write-only `qualifier_referenced_by`, and the env-value span-overlap shadowing bug; no unit tests.
- **lint-symbols-hover:** Useful hovers; eagerly formats all candidates before the position filter, hardcodes the field-selector grammar, and is thinly tested.
- **lint-symbols-nav:** Definition/references work for the happy path but env navigation is dead, hit-testing is end-exclusive, and selection tie-breaks are untested.
- **lsp-core:** The dispatch is readable but fragile — any handler error kills the server, deleted-file diagnostics never clear, every request triggers a full re-lint, and `serve()`/dispatch have zero tests.
- **lsp-protocol:** The byte-vs-UTF-16 root cause (no `positionEncoding` negotiation), asymmetric URI encode/decode, dropped `relatedInformation`, and untested transport framing.
- **diagnostics-model:** Stable rule ids done right; marred by the hardcoded-`variable` catalog entity, the divergent second catalog parser, and dead `Diagnostic`/`span` API.
- **source-loading:** The most complex module and among the least tested; gzip-bomb, redirect downgrade, symlink-escape, and pinned-commit-on-side-branch all need attention.
- **sdk-workspace:** Clean public shape but the runtime hot path re-reads disk every resolve, `max_staleness` is inert, and refresh status isn't reset on `Unchanged`.
- **resolve:** Correct on the lint-validated path but silently degrades on the un-linted CLI path, and its core semantics are entirely unverified by tests.
- **cli:** Mostly tidy; the stdout/LSP tracing corruption is the one serious item, plus some dead/duplicated context-parsing branches.
- **output-fmt:** Functional but human output diverges from JSON (external values, `related`) and panics on broken pipe.
- **docs-module:** Minor dev-server warts (HEAD length, routing, fatal accept) and a doc inaccuracy; entirely untested serving path.

## 9. Appendix

### Counts by severity (after exclusion/dedup)
- critical: 2
- high: 6
- medium: 26
- low: 48
- info: 12

Total reported: **94** (merged from ~135 raw findings; the pervasive byte/UTF-16 issue, the per-target Lua recompile, `external_value_keys`, the SDK resolve re-read, `declared_workspace_environments` re-parse, the qualifier-graph rebuild, the parse panic, the Lua sandbox, and the LSP error-kills-server issue were each reported by multiple reviewers and collapsed to one finding).

### Counts by category (approximate, post-dedup)
- test-coverage: ~21
- data-structure-efficiency: ~14
- bug: ~22
- security: ~9
- modularity-naming / architecture: ~16
- error-handling: ~7
- api-consistency / vocabulary: ~9
- async-blocking: ~4
- docs: ~2

### Refuted / low-confidence (excluded or flagged)
Excluded as **refuted** with no overturning deep verdict: `json_from_toml_value` swallows non-finite floats to `null` (`fields.rs:154-156` — toml-span has no NaN/datetime variant, so the fallback is dead); schema-backed values left unvalidated when the schema fails to compile (`builtins/variable.rs:458-486` — judged defensible design, not a bug); `handler_exists` redundant pre-check (`lua_lint.rs:92-97` — runtime fetch already validates); `DiagnosticLocation.span` "written but never read" framing (`diagnostics.rs:589-642`) — the *dead public `span()` constructor* finding is retained, but the broader "drop the field entirely" claim was refuted; the resolve/load symlink-escape **test-gap** finding (the lint-stage lua symlink case is in fact covered). Retained but flagged **needs confirmation / low-confidence:** the duplicate manifest custom-rule first-vs-last precedence claim (`workspace.rs:108-131`, confidence low — registry/conflict winner mismatch is real but the user-visible effect was rated uncertain); `DiagnosticRule::as_string` allocation in sort/scan paths (uncertain verdict — the optimization is valid but the impact at current scale is modest); the variable-schema-ref path-escape coverage gap (uncertain). The numeric int/float equality, missing-context `neq`/`not_in`, and large-i64-precision items are confirmed but low-confidence on whether the current behavior is a bug versus an undocumented intentional choice — they are reported as "document or fix".
