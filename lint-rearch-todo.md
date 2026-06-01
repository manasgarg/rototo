# lint-rearch todo list

Source: `lint-rearch-review.md`.

This checklist converts the review findings into actionable work. Items are
deduplicated when the same issue appears in multiple review sections, but the
work should still cover the full affected surface.

## P0: ship blockers

- [x] Restrict the Lua custom-lint sandbox.
  - Affected: `src/lua_lint.rs:55`, `src/lua_lint.rs:144`.
  - Replace `Lua::new()` with a limited `Lua::new_with(...)` stdlib that excludes
    `os`, `io`, and `package`.
  - Explicitly remove or nil dangerous globals that remain reachable.
  - Add defense tests proving `os.execute`, `os.getenv`, `io.open`, and
    `require` are unavailable to workspace lint scripts.

- [x] Add Lua execution limits.
  - Affected: `src/lua_lint.rs:44-50`, `src/lua_lint.rs:133-165`.
  - Add instruction limits, memory limits, and a bounded task timeout around
    registration and handler execution.
  - Add an integration test where a custom lint handler loops forever and the
    loader returns a bounded diagnostic/error instead of hanging.

- [x] Make TOML parse diagnostics safe for non-ASCII syntax errors.
  - Affected: `src/lint/syntax/diagnostics.rs:36-42`,
    `src/lint/syntax/location.rs`.
  - Snap parser span starts/ends to valid UTF-8 char boundaries before slicing.
  - Use `str::get(...)` or equivalent safe helpers for parse-error range
    expansion.
  - Add malformed TOML fixtures with multibyte text adjacent to the error and
    assert lint returns diagnostics instead of panicking.

- [x] Fix LSP and rendered source positions for non-ASCII text.
  - Affected: `src/lint/source/line_index.rs:34-49`,
    `src/lsp/protocol.rs:83-107`, `src/lsp/convert.rs:111-122`,
    `src/lint/symbols/common.rs`, `src/output.rs:491-501`.
  - Choose one position contract: advertise and honor UTF-8, or convert byte
    offsets to and from UTF-16 code units.
  - Apply the same contract to inbound hover/definition/references positions,
    outbound diagnostics, document symbols, hover ranges, references, and human
    `path:line:col` labels.
  - Add multibyte LSP/symbols tests with exact start and end ranges.

- [x] Stop logging to stdout during stdio LSP sessions.
  - Affected: `src/main.rs:280`, `src/main.rs:457-459`,
    `src/main.rs:653-661`.
  - Configure tracing to write to stderr.
  - Add a small regression check or manual protocol smoke test proving warnings
    cannot corrupt stdout JSON-RPC frames.

- [x] Isolate LSP per-message errors.
  - Affected: `src/lsp/server.rs:36-149`.
  - For request handler failures, write a JSON-RPC error response and continue
    serving.
  - For notification failures, log and continue.
  - Reserve loop-breaking errors for transport failure and valid shutdown/exit.
  - Add `tokio::io::duplex` tests for initialize, unknown method, shutdown,
    exit, malformed request, and out-of-workspace request.

- [x] Add result-asserting resolution tests before refactoring resolution.
  - Affected: `src/resolve.rs`.
  - Cover true and false outcomes for `eq`, `neq`, `in`, `not_in`, `gt`, `gte`,
    `lt`, and `lte`.
  - Cover bucket evaluation, half-open `[start, end)` boundaries, AND semantics,
    qualifier indirection true/false, `_` environment fallback, and qualifier
    cycle errors.
  - Add a golden test for `bucket_value("known-salt", json!("user-123"))` and
    a determinism test.

## P1: security hardening

- [x] Add decompressed archive extraction limits.
  - Affected: `src/source.rs:610-697`.
  - Keep the existing compressed download cap, and also track cumulative
    decompressed bytes and entry count during extraction.
  - Abort extraction when either cap is exceeded.
  - Add real `.tar.gz` tests for compressed-size cap, decompressed-size cap,
    and entry-count cap.

- [x] Reject HTTPS redirects that downgrade to HTTP.
  - Affected: `src/source.rs:350-361`, `src/source.rs:446-458`.
  - Configure `reqwest` redirect policy to reject non-HTTPS follow-up URLs.
  - Ensure bearer authorization cannot be forwarded to downgraded or otherwise
    unsafe destinations.
  - Add redirect-policy tests if practical, otherwise isolate the policy builder
    for unit coverage.

- [x] Prevent symlink escape through source subdirectories.
  - Affected: `src/source.rs:330`, `src/source.rs:402-405`,
    `src/source.rs:791-812`.
  - Canonicalize selected `#ref:subdir` and `#:subdir` targets and assert they
    remain inside the canonical staged root.
  - Cover git checkout symlinks and archive subdir selection.

- [x] Prevent context-schema path symlink escape.
  - Affected: `src/sdk.rs`, `src/resolve.rs`, context-schema path helpers.
  - Canonicalize resolved schema paths and assert they remain inside the
    canonical workspace root.
  - Add fixture coverage for schema refs that are syntactically safe but point
    through a symlink outside the workspace.

- [x] Harden git ref handling.
  - Affected: `src/source.rs:291-300`, `src/source.rs:530-570`.
  - Reject ref fragments beginning with `-`.
  - Pass refs after `--` to `git ls-remote`.
  - Scrub `GIT_*` environment variables for `git ls-remote` consistently with
    `rev-parse` and `checkout`.
  - Add tests for leading-dash refs and environment scrubbing behavior where
    feasible.

- [x] Cap LSP `Content-Length` allocations.
  - Affected: `src/lsp/transport.rs:31-39`.
  - Reject messages larger than a configured maximum before allocating the body
    buffer.
  - Add transport framing tests for oversized, missing, malformed, and valid
    `Content-Length`.

- [x] Replace 64-bit `DefaultHasher` source fingerprints.
  - Affected: `src/source.rs:595-608`.
  - Use a stable wide digest for content-hash refresh fingerprints.
  - Add a golden digest test so refresh behavior is reproducible.

## P1: correctness and user-visible behavior

- [x] Fix reference target shadowing on variable environment values.
  - Affected: `src/lint/references.rs:422-454`.
  - Point the environment reference edge at the `[env.<name>]` header/key span,
    not the `value = "..."` span, or give value edges higher precedence.
  - Add a references test clicking the environment value and asserting it
    resolves value references, not environment references.

- [x] Fix undeclared-environment diagnostic span.
  - Affected: `src/lint/references.rs:423-435`.
  - Capture the environment-name span during projection.
  - Use that span for `rototo/variable-unknown-environment`.
  - Update the canonical fixture range.

- [x] Make completion context-aware.
  - Affected: `src/lint/symbols/completion.rs:20-35`,
    `src/lsp/server.rs:261-281`.
  - Thread cursor position into completion computation.
  - Gate environments, qualifiers, values, predicate operators, and custom-lint
    field selectors based on syntactic context.
  - Add completion tests for `[values]`, predicate fields, env rules, and custom
    lint files.

- [x] Disambiguate external value wrapper syntax.
  - Affected: `src/lint/project/external_value.rs:12-39`.
  - Make a schema-backed value whose intended JSON is `{"value": X}`
    representable.
  - Choose an unambiguous wrapper rule or explicit syntax.
  - Add a fixture that validates a value object containing the key `value`.

- [x] Discover overlay-only files in LSP/snapshot lint.
  - Affected: `src/lint/stages/discover.rs:52-77`,
    `src/lint/source/discover.rs`.
  - Enumerate overlay keys after disk discovery.
  - Add synthetic documents for overlay-only manifest, qualifier, variable,
    schema, external value, and custom lint files where valid.
  - Add overlay-only diagnostics and symbols tests.

- [x] Align human `variable get` with JSON output.
  - Affected: `src/output.rs:267-293`.
  - Render merged inline and external values for directory-backed variables in
    human output, or clearly show the expanded external values alongside the
    primary TOML.
  - Add a CLI test for `variable get directory-backed-message` without
    `--json`.

- [x] Render related diagnostic information in human output and LSP.
  - Affected: `src/output.rs:478-501`, `src/lsp/convert.rs:24-37`.
  - Print `diagnostic.related` as indented notes in human output.
  - Add LSP `relatedInformation` mapping.
  - Add tests for qualifier-cycle and rule-shadowing related locations.

- [x] Fix custom diagnostic catalog entity reporting.
  - Affected: `src/diagnostics.rs:501-509`.
  - Carry the registered entity into catalog entries, or remove `entity` for
    custom entries if it cannot be represented accurately.
  - Add JSON and human catalog tests for non-variable custom rules.

- [ ] Make un-linted CLI resolution fail closed for malformed config.
  - Affected: `src/resolve.rs`.
  - Either lint before CLI resolve, or make resolution return errors for unknown
    predicate ops, malformed bucket ranges, empty predicate lists, matching
    rules with non-string `qualifier`/`value`, and missing required shapes.
  - Add CLI tests for each malformed case.

- [ ] Decide and document numeric comparison semantics.
  - Affected: `src/resolve.rs:334-383`.
  - Decide whether JSON integer `100` and float `100.0` should compare equal for
    `eq`/`in`.
  - Decide whether missing context paths should make `neq`/`not_in` false or
    true.
  - Decide how to handle large integers without lossy `f64` conversion.
  - Implement and document the chosen behavior with tests.

- [x] Improve qualifier-cycle diagnostics.
  - Affected: `src/lint/builtins/graph.rs:65-74`.
  - Do not render an alphabetically sorted SCC with `->` as though it were the
    real cycle path.
  - Render either a real edge path or a neutral set-style message.
  - Update fixtures.

- [x] Remove duplicate self-cycle plus unreferenced diagnostics.
  - Affected: `src/lint/references.rs`, `src/lint/builtins/graph.rs:76-97`.
  - Treat a self-referencing qualifier as referenced for unreferenced warnings,
    or otherwise suppress the secondary warning when a cycle diagnostic exists.
  - Add a fixture assertion.

- [x] Avoid double-validating duplicate inline/external variable values.
  - Affected: `src/lint/builtins/variable.rs:430-486`.
  - After emitting the duplicate-value diagnostic, avoid emitting redundant type
    or schema diagnostics for both copies unless there is a clear user benefit.
  - Add duplicate-value fixture assertions for diagnostic count and rules.

- [ ] Improve context-schema attribute validation.
  - Affected: `src/lint/builtins/schema.rs:132-148`.
  - Decide how much JSON Schema support is required for context path checking.
  - Handle `$ref`, `additionalProperties`, and common combinators, or document
    the flat-`properties` limitation and avoid false positives.
  - Add fixtures for supported and intentionally unsupported schema constructs.

- [x] Fix bucket range diagnostic precedence.
  - Affected: `src/lint/project/fields.rs:114-136`.
  - A float bucket range should report "must contain two integers" rather than
    the bounds diagnostic.
  - Add a fixture for float bounds.

- [x] Fix diagnostic sorting/location edge cases.
  - Affected: `src/lint/output.rs:13-31`, `src/model.rs:57-71`.
  - Stop mixing byte offset and line number in the same sort tuple slot.
  - Ensure workspace-root diagnostics are represented in grouped diagnostics
    where callers need them.
  - Add tests for workspace-root diagnostics and range-only constructor cleanup.

- [x] Do not enroll directory symlinks as file documents.
  - Affected: `src/lint/source/discover.rs:143-154`.
  - Distinguish symlink-to-file from symlink-to-directory.
  - Emit a clear diagnostic or skip unsupported symlinked directories.
  - Add discovery tests for symlinked directories named `*.toml` or `*.json`.

- [x] Fix LSP shutdown/exit and incremental change handling.
  - Affected: `src/lsp/server.rs:145`, `src/lsp/server.rs:185-200`.
  - Implement the LSP rule that `exit` before `shutdown` exits with failure.
  - Reject or correctly apply incremental `didChange` ranges; do not store a
    fragment as the full overlay.
  - Add dispatch tests for full and incremental changes.

- [x] Fix workspace-root canonicalization fallback.
  - Affected: `src/lsp/uri.rs:31-34`.
  - Do not silently fall back to a non-canonical root when canonicalization
    fails in a way that later breaks `strip_prefix`.
  - Return a clear initialization error or keep root/path canonicalization
    consistent.
  - Add URI/root tests for missing and non-canonical workspace roots.

- [x] Fix docs dev-server edge cases.
  - Affected: `src/docs.rs`.
  - HEAD responses should return the same `Content-Length` as GET where
    applicable.
  - Route `/cli.html/` consistently.
  - Do not exit the whole serve loop on a single `accept()` error.
  - Add docs server tests or isolate request handling for unit coverage.

## P2: test coverage backlog

- [x] Add direct unit tests for `line_index.rs`.
  - Cases: empty text, first line, last line, past-end clamp, CRLF if supported,
    and explicit multibyte examples such as `e-acute` and an emoji.

- [ ] Add direct tests for syntax parse diagnostics.
  - Cases: malformed TOML with multibyte nearby, malformed JSON with multibyte
    nearby, zero-width spans, and end-of-file spans.

- [ ] Add source URI and path safety tests.
  - Cases: `#ref:subdir`, `#ref`, `#:subdir`, bare URLs, empty fragment parts,
    absolute paths, `..`, empty subdirs, single-subdir archive inference, and
    ambiguous archive roots.

- [x] Add `resolve_workspace_relative_path` and `resolve_workspace_root_path`
    tests.
  - Cases: pop-to-escape, absolute path, empty path, deep `..`, normalized safe
    paths, schema refs, and lint file refs.

- [ ] Add reference tie-break tests.
  - Affected: `src/lint/references.rs`.
  - Cover overlapping spans, priority ordering, span-size ordering, and equal
    priority/size ties.

- [x] Add LSP URI tests.
  - Cases: `%20`, multibyte percent-encoded paths, trailing `%`, non-hex escape,
    invalid UTF-8, unsupported schemes, and file paths with `#` or `%`.

- [ ] Add diagnostics model tests.
  - Cases: `CustomRuleId::parse` accept/reject, custom severity parsing,
    invalid severity dropping, and non-variable custom catalog entity.

- [ ] Add branch fixtures for untested lint arms.
  - Cases: `type = <non-string>`, `env = <scalar>`, `rule = <scalar>`,
    workspace path is not a directory, variable schema ref escaping path,
    schema ref pointing at a non-schema document, and entity-scoped
    `lint_variable` attribution for value/rule diagnostics in sibling files.

- [ ] Add exact LSP range tests.
  - Cover at least one hover, references result, definition result, and document
    symbol range.
  - Assert both start and end line/character.
  - Include a multibyte line.

- [ ] Extend canonical diagnostics to assert messages where useful.
  - Add an optional expected message or substring to `ExpectedDiagnostic`.
  - Cover dynamic-content rules such as unknown environment, unknown value,
    unknown qualifier, type mismatch, and schema mismatch.

- [ ] Add custom-lint integration tests.
  - Sandbox-denied globals.
  - Non-terminating script bounded by timeout/instruction limit.
  - Script with no `register` global emits a diagnostic or intentionally
    documented result.
  - Silent dropped registrations/targets become diagnostics.

- [ ] Add LSP transport and dispatch tests.
  - Drive `serve()` or equivalent through `tokio::io::duplex`.
  - Cover unknown method error `-32601`, shutdown/exit, malformed message,
    out-of-workspace request, and notification errors that should not terminate
    the loop.

- [ ] Add archive loading tests.
  - Build real `.tar.gz` inputs.
  - Cover compressed download cap, decompressed cap, entry count cap, unsafe
    paths, special entries, single-root inference, ambiguous-root error, and
    subdir success.

## P2: runtime data structures and scale

- [ ] Cache parsed workspace state for SDK resolution.
  - Affected: `src/workspace.rs`, `src/resolve.rs`, `src/sdk.rs`.
  - Store parsed qualifier configs, parsed variable configs, merged external
    values, id maps, and compiled context validator in the loaded workspace
    snapshot.
  - Resolve from in-memory structures rather than reading TOML on every
    `resolve_*`.
  - Ensure `RefreshingWorkspace` swaps the entire parsed snapshot on refresh.

- [ ] Build by-key indices once for lint and LSP.
  - Add maps for path to entity ids, variable id to value keys, id to qualifier,
    id to variable, targets with references, and qualifier graph.
  - Use prefix/range queries over tuple-key maps instead of full scans where
    practical.

- [ ] Remove `external_value_keys` full-document scan per variable.
  - Affected: `src/lint/source/mod.rs:79-90`,
    `src/lint/project/variable.rs`.
  - Precompute external value keys by variable during discovery.

- [ ] Remove `referenced_variable_value_keys` full-map scan per variable.
  - Affected: `src/lint/references.rs:240-246`,
    `src/lint/builtins/graph.rs:155`.
  - Maintain a direct `variable -> set(value_key)` map.

- [ ] Stop reparsing/revalidating workspace environments per variable.
  - Affected: `src/lint/references.rs`.
  - Reuse projected manifest environments.

- [ ] Reuse qualifier graph results.
  - Affected: `src/lint/references.rs`, `src/lint/builtins/graph.rs`.
  - Build once per lint run and share between cycle and referenced-qualifier
    checks.

- [ ] Index reference targets for LSP requests.
  - Affected: `src/lint/references.rs`.
  - Avoid full edge scans for `target_at_position`, `reference_locations`, and
    nested schema `has_references` checks.

- [ ] Avoid full workspace rescans for single-file document symbols.
  - Affected: `src/lint/symbols/document_symbols.rs`.
  - Use path-to-entity indices.

- [ ] Replace linear `qualifier_for_id` and `variable_for_id` lookups on hot
    paths.
  - Affected: `src/workspace.rs:109-129`.
  - Add id maps to `WorkspaceInspection` or the loaded runtime snapshot.

- [ ] Reduce redundant allocation in lint output and LSP conversion.
  - Affected: `src/lint/syntax/mod.rs`, `src/lint/output.rs`,
    `src/diagnostics.rs`, `src/lsp/convert.rs`, `src/lint/source/path.rs`.
  - Review the `ParsedToml` owned clone strategy.
  - Expose borrowed rule id strings or `Cow` to avoid repeated
    `DiagnosticRule::as_string()` allocation in sorting/scanning.
  - Reuse the LSP diagnostic rule string instead of allocating twice.
  - Cache canonical workspace root for containment checks.
  - Confirm whether the rule-string allocation impact is meaningful before
    prioritizing it.

- [ ] Drop duplicate `ValuesNode.inline_keys`.
  - Affected: `src/lint/index/nodes.rs`.
  - Use `inline_values.keys()` unless a measured need exists.

- [ ] Decide whether string aliases in `ids.rs` are acceptable long-term.
  - Affected: `src/lint/index/ids.rs`.
  - Either keep as a documented current-slice tradeoff or introduce newtypes if
    id confusion becomes a real bug source.

## P2: async and blocking behavior

- [ ] Move synchronous CPU-heavy lint phases off the async runtime.
  - Affected: `src/lint/stages/mod.rs:36-62`, parse/projection/value stages.
  - Wrap TOML/JSON parse, projection construction, JSON Schema compilation, and
    value validation in `spawn_blocking` where appropriate.
  - Ensure errors and diagnostics preserve source locations.

- [ ] Avoid blocking filesystem calls in async LSP handlers.
  - Affected: `src/lsp/uri.rs:100-101`.
  - Replace `std::fs::canonicalize` with async canonicalization or move it to
    blocking work.

- [ ] Offload or document synchronous JSON Schema compile/validate in SDK and
    resolve.
  - Affected: `src/sdk.rs:96-143`, `src/resolve.rs:159-163`.
  - Prefer precompiled validators on loaded snapshots; otherwise use
    `spawn_blocking` for expensive validation paths.

## P3: architecture, cohesion, and cleanup

- [ ] Resolve the `GateIndex` design.
  - Affected: `src/lint/index/gates.rs`.
  - Either wire `is_blocked` into checked stages and test suppression behavior,
    or delete the gating subsystem.
  - If snapshot metadata remains, document that it does not gate downstream
    lint behavior.

- [x] Delete orphaned `src/lint/source/span.rs`.
  - Remove the module declaration and the unused `Spanned<T>` type.

- [ ] Remove dead public `Diagnostic` and `DiagnosticLocation::span`.
  - Affected: `src/diagnostics.rs`.
  - Make `source_span` the only span constructor.
  - Simplify output sorting once range-only span construction disappears.

- [ ] Consolidate duplicated position math.
  - Affected: `src/lint/symbols/common.rs`, `src/lint/references.rs`.
  - Share `location_contains_position`, range size calculations, and tie-break
    helpers.

- [ ] Consolidate workspace environment helpers.
  - Affected: `src/lint/builtins/workspace.rs`, `src/lint/references.rs`.
  - Use one projected source of truth for declared environments.

- [ ] Consolidate context schema path/read/compile helpers.
  - Affected: `src/sdk.rs`, `src/resolve.rs`.
  - Keep path-safety rules in one place.

- [ ] Consolidate custom rule extraction.
  - Affected: `src/workspace.rs`, `src/catalog.rs`, `src/lint/project/mod.rs`.
  - Use the lint projection or one shared parser so catalog and emitted
    diagnostics cannot diverge.

- [ ] Reduce `lua_lint` public API exposure.
  - Affected: `src/lib.rs`, `src/lua_lint.rs`.
  - Move Lua execution under `lint::custom` or make it `pub(crate)`.

- [ ] Remove misleading signatures and dead params.
  - Affected: `src/lint/project/variable.rs`,
    `src/lint/builtins/variable.rs`, `src/lint/builtins/schema.rs`,
    `src/main.rs`.
  - Clean up `project_inline_values(_toml)`.
  - Remove or make meaningful `invalid_shape` where it is always false.
  - Remove `let _ = &schema.value`.
  - Collapse `valid_context_schema` duplicate lookups and unreachable branches.
  - Remove unreachable `insert_context_path` and `merge_context` guards where
    control flow already proves them impossible.

- [ ] Decide the public vocabulary for lint source summaries.
  - Affected: `WorkspaceLint.documents`, `DocumentDiagnostics`,
    `SourceDocumentSummary`, `workspace lint --json`.
  - Rename to `sources`/`SourceSummary`, or document an explicit exception for
    `document` in this public surface.

- [ ] Decide whether `policy::run_builtin` should exist.
  - Affected: `src/lint/stages/policy.rs`.
  - If policy is only a custom-lint host, rename/simplify the stage machinery or
    add a short comment explaining the empty built-in stage.

- [ ] Consolidate hover/definition/reference candidate walkers.
  - Affected: `src/lint/symbols/hover.rs`,
    `src/lint/symbols/definition.rs`, `src/lint/references.rs`.
  - Share node traversal and priority/span sorting so new referenceable fields
    do not require parallel edits.

- [ ] Consider a macro or helper for stage-bound diagnostic push wrappers.
  - Affected: `src/lint/stages/*`.
  - Keep current wrappers if compile-time stage binding remains clearer.

- [ ] Review `lint_workspace_until`.
  - Affected: `src/lint.rs`, `src/lint/engine.rs`.
  - Either make the partial pipeline behavior consistent and used by tests/tools,
    or remove it.

- [x] Add a guard that `COMPLETION_LABELS` stays in sync with predicate ops.
  - Affected: `src/lint/index/nodes.rs`, `src/lint/symbols/completion.rs`.
  - Add a unit test or derive completions from the operator enum.

- [x] Fix source discovery containment behavior.
  - Affected: `src/lint/source/path.rs`, `src/lint/source/discover.rs`.
  - Do not fail open on non-`NotFound` canonicalize errors.
  - Add direct unit tests for containment errors.

- [ ] Bring manifest environment validation through the projection.
  - Affected: `src/lint/project`, `src/lint/builtins`.
  - Avoid bypassing projected manifest data for environment validation.

- [ ] Avoid full Lua script recompile per target.
  - Affected: `src/lua_lint.rs`, `src/lint/custom`.
  - Reuse compiled/registered script state where safe with sandbox limits, or
    document why per-target recompilation is retained.

- [ ] Remove or use write-only `qualifier_referenced_by`.
  - Affected: `src/lint/references.rs`.
  - Delete if unnecessary, or use it to avoid recomputing referenced qualifier
    data.

- [ ] Improve symbol hover/navigation internals.
  - Affected: `src/lint/symbols/hover.rs`,
    `src/lint/symbols/definition.rs`, `src/lint/symbols/references.rs`.
  - Avoid formatting all hover candidates before position filtering.
  - Avoid hardcoding custom-lint field-selector grammar in multiple places.
  - Decide whether hit-testing should be end-exclusive or inclusive and test it.
  - Implement or remove dead environment navigation paths.

- [ ] Reduce LSP full relint churn.
  - Affected: `src/lsp/server.rs`.
  - Cache snapshots between requests and invalidate on relevant overlay changes,
    or document the current full-relint design as temporary.

- [ ] Clear diagnostics for deleted files.
  - Affected: `src/lsp/server.rs`, diagnostic publishing.
  - Publish empty diagnostics for files removed from discovery after close/save.

- [ ] Investigate source-loading pinned-commit-on-side-branch behavior.
  - Affected: `src/source.rs`.
  - Reproduce the review note, define expected behavior, and add a test before
    changing checkout/probe logic.

- [ ] Remove duplicated/dead CLI context parsing branches.
  - Affected: `src/main.rs`.
  - Simplify context parsing once unreachable guards are removed.

## P3: API, diagnostics, and error handling

- [ ] Wire or remove `RefreshOptions::max_staleness` and
    `RefreshStatus::stale()`.
  - Affected: `src/sdk.rs:366-437`.
  - If wired, define how stale status affects refresh users.
  - If removed, keep the SDK API smaller until behavior exists.

- [ ] Derive workspace diagnostic catalog from the same data as lint.
  - Affected: `src/catalog.rs`, `src/lint/project/mod.rs`, `src/workspace.rs`.
  - Eliminate the second TOML reader.
  - Use one duplicate custom-rule precedence policy everywhere.
  - Confirm the low-confidence duplicate-policy finding with a fixture before
    changing behavior.

- [ ] Decide `Workspace::inspect` behavior for malformed context schemas.
  - Affected: `src/sdk.rs:64-110`.
  - Either defer context schema parse/compile errors to first use, or document
    that inspect validates context schema even when lint is skipped.

- [x] Percent-encode emitted file URIs.
  - Affected: `src/lint/source/path.rs`, `src/lsp/uri.rs`.
  - Make `file_uri` and `path_from_file_uri` round-trip spaces, `#`, `%`, and
    multibyte paths.
  - Add URI tests.

- [ ] Fix `--quiet` behavior for warning-only lint.
  - Affected: `src/output.rs:137-198`.
  - Decide whether quiet suppresses warnings on passing lint.
  - Add tests for clean, warning-only, and error lint outputs.

- [ ] Add `has_errors()` helpers to qualifier and variable lint results.
  - Affected: `src/main.rs`, model types.
  - Replace hand-rolled pass/fail checks in CLI command arms.

- [ ] Document public `resolve_*` free functions.
  - Affected: `src/resolve.rs:14-116`, public SDK docs.
  - Explain lint assumptions, context validation, environment validation, and
    when to prefer `Workspace::load`.

- [ ] Treat broken pipe as a clean CLI exit.
  - Affected: output paths in `src/output.rs` and CLI printing.
  - Avoid panicking when output is piped to `head`.
  - Add a smoke test if practical.

- [ ] Add structured error information.
  - Affected: `src/error.rs`.
  - Add error kinds and/or source chaining so SDK consumers and refresh logic do
    not need string matching.

- [ ] Reset refresh status on healthy unchanged/immutable probes.
  - Affected: `src/sdk.rs:491-564`.
  - Clear `consecutive_failures` and `last_error` or otherwise update
    `last_success` when probes prove the source is healthy but unchanged.
  - Add refresh status tests for `Unchanged` and `Immutable` after a failure.

- [ ] Turn silent custom-lint drops into diagnostics.
  - Affected: `src/lint/custom/runner.rs`, `src/lint/custom/registry.rs`,
    `src/lua_lint.rs:61-69`.
  - Emit diagnostics for dropped registrations/targets and scripts with no
    `register` global, or document intentionally silent behavior.

## P3: documentation

- [x] Fix diagnostics reference severity text.
  - Affected: `docs/src/api/diagnostics.md:17`.
  - State that severity can be `error` or `warning`.
  - Note that warnings do not fail lint.

- [x] Add docs-server test coverage or request-handler unit tests.
  - Affected: `src/docs.rs`.
  - Cover HEAD, trailing slash routes, 404s, and accept-loop error behavior.

## P4: low-confidence or confirm-first items

- [ ] Confirm duplicate custom-rule precedence behavior before changing it.
  - Affected: `workspace.rs`, `catalog.rs`, `lint/project`.
  - The review rates the user-visible effect as uncertain; write a fixture that
    demonstrates the current disagreement, then fix if observable.

- [ ] Confirm variable-schema-ref path-escape coverage gap.
  - Add a focused test before deciding whether code changes are required.

- [ ] Confirm whether `DiagnosticRule::as_string()` allocation matters at target
    workspace sizes.
  - If not measurable, leave it as a cleanup item rather than a priority.

- [x] Document or fix numeric edge cases.
  - Covered under P1, but treat as design-sensitive: int/float equality,
    missing-context `neq`/`not_in`, and large integer precision may be intended
    once specified.

## No-action notes from the review

These were explicitly refuted or judged not actionable in the review. Keep them
out of the active backlog unless new evidence appears.

- [ ] No action: `json_from_toml_value` non-finite-float fallback to `null`.
  - Reason: `toml-span` has no NaN/datetime variant, so the fallback is dead.

- [ ] No action: schema-backed values are unvalidated when schema compile fails.
  - Reason: review judged current behavior defensible because schema failure is
    already reported.

- [ ] No action: `handler_exists` pre-check as a standalone bug.
  - Reason: runtime handler fetch validates the handler.

- [ ] No action: broad removal of `DiagnosticLocation.span` field.
  - Reason: only the dead public constructor is actionable; the field itself is
    still used by live source-span locations.

- [ ] No action: resolve/load symlink-escape test gap for lint-stage Lua.
  - Reason: review says that specific case is already covered.

## Suggested sequencing

- [x] First pass: P0 items plus the archive, redirect, symlink, and git-source
    security hardening.
- [ ] Second pass: resolution tests and position/span tests, then refactor
    runtime resolution to use cached loaded state.
- [x] Third pass: LSP robustness, context-aware completion, related diagnostics,
    and output parity.
- [ ] Fourth pass: architecture cleanup and lower-risk performance work.
- [ ] Final pass: docs, low-confidence confirmations, and no-action audit.
