# Test matrix review findings

An adversarial read of the eleven test matrices (composition, semantic
index, source and auth, refresh, SDK load, resolution, expression, LSP,
lint identity, Lua lint, projection), done after the matrices landed. The
question was not "is there coverage" but "are the promises themselves
right": cross-matrix contradictions, tested-but-wrong behavior, behavior a
test pins that nobody decided, and whether each deliberate non-test is
still acceptable.

Findings only; nothing here changes behavior. Each item is classified
candidate-bug, needs-decision, or accepted-gap, ordered by severity within
its class. Matrix rows and pinned tests are cited so each item can be
picked up cold.

## Candidate bugs

### 1. Hydration runs only on the query path

Resolution matrix H3, pinned by `rules_selected_catalog_values_do_not_hydrate_today`
(`tests/sdk.rs`).

A catalog entry selected by `method = "query"` hydrates its `x-rototo-ref`
fields (pointer refs, dynamic refs, multi-catalog refs). The same entry
selected through `[resolve]` rules or a default comes back raw: the app
receives `"welcome#/variants/default/subject"` instead of the referenced
value. `catalog_entry_view` is called only from `resolve_catalog_query`
(`src/resolve.rs:360`); the rules path materializes entries through
`catalog_entry_value` (`src/lint/runtime.rs:744`) with no hydration.

Why it matters: the value shape an app receives depends on which resolve
method the package author picked, which is exactly the kind of internal
detail the variable abstraction is supposed to hide. The package format
reference and CLAUDE.md both say hydration applies at resolve time without
qualifying the method.

Likely fix direction: run `catalog_entry_view` wherever a catalog-typed
selected value is materialized. Needs a look at trace output and the `id`
injection (hydration also injects the entry id), which would be a visible
change for rules-path consumers.

Manas: i think we should separate the hydration for the purpose of resolution
from hydration for the app needs. for now, the app should always receive entries
that have not been hydrated. we would need to add functionality to sdk that
would help app discover catalogs + enums and load their entries. we should think
through the kind of api we should have here (package reflection + visitor +
lookup?)

### 2. Relative-file `$ref` is hydrated by lint but not by resolution

Resolution matrix H2, pinned by the `external_ref_template` assertion in
`query_resolution_hydrates_every_catalog_reference_form` (`tests/sdk.rs`).

A catalog schema field can reach its `x-rototo-ref` through `$ref`
indirection. Same-document refs (`#/$defs/...`) hydrate. A relative-file
ref (`email_template.schema.json#/$defs/...`) compiles fine at lint time,
because the schema compiler registers every catalog schema under
`rototo://catalogs/<id>.schema.json` and resolves relative URIs against
that base, but `hydrate::resolve_schema_ref` (`src/resolve/hydrate.rs:175`)
only matches the full `rototo://catalogs/` spelling or an exact `$id`, so
the ref field silently passes through unhydrated.

Why it matters: lint says the package is fine; resolution quietly returns a
different shape than the schema implies. The catalog-refs fixture is
lint-clean while exhibiting the mismatch.

Likely fix direction: resolve relative URIs in `resolve_schema_ref` against
the current catalog's base URI, mirroring what the compiler does.

Manas: we should fix this.

### 3. Batch resolve and batch trace use different evaluation state

Resolution matrix X2/X4 are the by-construction rows this sits under;
no test pins the difference today.

`resolve_variables_unchecked` (`src/resolve.rs:129`) builds one
`ResolutionState` for the whole batch: one `env.now`, one memoization
cache. `trace_variable_resolutions_unchecked` (`src/resolve.rs:143`) builds
a fresh state per variable: a new `env.now` and an empty cache each time.
So `rototo resolve --variables` and its traced twin can disagree, both on
time-sensitive rules evaluated near a boundary and on how many times a
shared condition variable is evaluated.

Why it matters: trace exists to explain what resolve did. A trace that
re-evaluates under different state can explain something resolve did not
do. Low probability in practice, but it is exactly the tool you reach for
when debugging a time-boundary problem.

Likely fix direction: one shared state per batch in both functions, or per
variable in both; shared-per-batch matches the single-resolution semantics
of `env.now` least surprisingly for a batch labeled "one resolution run".

Manas: fix this.

### 4. A Lua file in a lint subdirectory is ignored with no warning

Lua lint matrix D2, pinned by `nested_lua_files_are_silently_ignored_today`
(`tests/package_lint.rs`).

Discovery reads `lint/*.lua` non-recursively, and the unrecognized-file
walker covers `model/`, `data/`, `variables/`, and `layers/` but not
`lint/`. So `lint/payments/budget.lua` neither runs nor warns. Every other
rototo-owned directory either namespaces recursively or warns about files
nothing claims; `lint/` does neither.

Why it matters: custom lint is where teams put their own governance checks.
A rule file that silently stops participating is a policy hole nobody sees.
The cheapest fix is adding `lint` to the unrecognized-file walker; deciding
whether `lint/` should namespace recursively like everything else is the
larger question.

Manas: fix this. lint/ can namespace like other entities.

### 5. The staged-package escape docs omit `git+file://`

Source matrix E2 note, pinned by
`a_staged_package_may_not_extend_a_local_git_source` (`tests/composition.rs`).

The behavior is right: a fetched package extending `git+file://` is refused
as a filesystem escape, same as `file://` and absolute paths. The
package-sources reference says "every format works" in `extends` and lists
only `file://`, absolute paths, and escaping `../` as staged-package
escapes. The docs bullet should name `git+file://`.

Manas: fix this

## Needs a decision

### 6. Namespaced ids cannot be addressed by custom lint

Semantic index matrix section 4 note, pinned by
`unsupported_lint_addresses_are_rejected` (`src/lint/custom/registry.rs`).

A namespaced id contains `/`, which the address grammar reads as a path
separator: `/variables/acme/in_trial` is rejected, so a namespaced variable
cannot be targeted at all. Widening the grammar naively would collide with
the structural segments: a variable named `acme/values` would be ambiguous
against `/variables/<id>/values`. If namespaced entities should be
addressable, the grammar needs an escaping decision (percent-encoding the
id, a different separator, or a bracketed segment). If not, the docs should
say so where custom lint targets are described.

Update 2026-07-05: superseded by a broader direction. design/addressing.md
drafts one addressing grammar (class:id steps, # for JSON pointers into
documents, prefix and relative forms) that resolves this finding as its
step 2; the earlier #-only patch idea is folded into it.

### 7. Staleness resets when a fallback start succeeds

Refresh matrix note. A fallback start records the fallback load as
`last_success`, so `stale(max_staleness)` measures from the degraded start,
not from when the primary last produced config. The status doc suggests
pairing `stale()` with `serving_fallback()` to alarm on running degraded
too long, which works, but "time since the primary was last healthy" is not
directly readable from the status. Decide whether that needs its own field
or whether the pairing guidance is enough.

Resolved 2026-07-05: the pairing guidance plus the event stream is enough.
A fallback start emits a fallback_loaded refresh event carrying the
primary failure reason (pinned by
refreshing_package_starts_on_the_fallback_and_recovers_the_primary), each
subsequent attempt emits a failed event, and recovery emits refreshed, so
degradation onset and duration are reconstructible from events and logs.
One nuance recorded: the startup event predates any subscriber, so it
reaches consumers via snapshot().last_event and the warn log, not the
broadcast subscription; event-stream-only monitoring sees the failed
events but not the onset event.

### 8. A bare token's origin binding spans the primary and fallback loads

Untested hypothesis from reading `BearerOriginBinding` and the fallback
path together; no pinned test.

The binding lives on `SourceOptions` and is shared by every load the
options reach. If the primary is an https archive that binds origin A and
then fails (bad archive, lint failure), a fallback that is an https archive
at origin B should then fail with the second-origin error instead of
loading. Bundled local fallbacks, the recommended shape, are unaffected,
which is why this is a decision rather than a bug: either the fallback load
should reset the binding (the primary attempt is over), or scoped tokens
are simply the answer for dual-archive setups and the error message is
acceptable. Worth a test either way once decided.

Resolved 2026-07-05: keep as-is. Resetting the binding would transmit a
token minted for the primary's origin to the fallback's origin, which is
the exact leak the guard exists to prevent; scoped tokens express the
dual-archive setup correctly, and the recommended bundled-local fallback
never touches the binding. Now pinned end to end by
bare_token_binding_spans_primary_and_fallback_archive_origins
(tests/sdk.rs) and sdk-load matrix row F6; the binding error already
names both origins and shows the scoped-token spelling to write.

### 9. An unmatched lint selector reports "ok"

Lint identity matrix S2, pinned by
`lint_selectors_filter_diagnostics_and_exit_status` (`tests/cli.rs`).

`rototo lint <pkg> --lint-rule rototo/never-fires` exits 0 and prints
`ok: <path>` even when the package has other errors. Scope-filtering
semantics are defensible (the selection passed), but the output reads like
a clean bill of health for the package. A CI pipeline that filters to a
team's authority would happily go green over broken base config. Options:
keep semantics but say "ok (selection)" or similar, or document the
behavior prominently. The pinned test makes whatever decision lands here a
deliberate change.

### 10. Immutability discovered late never stands the loop down cleanly

Refresh matrix note. If a channel URL starts mutable and later becomes
pinned content (or vice versa), the loop discovers `Immutable` per attempt
but the startup decision about spawning the loop was already made. Current
behavior is safe (extra probes, no wrong results); recorded so the behavior
is a choice rather than an accident.

## Accepted gaps, reassessed and kept

- Cross-origin redirect stripping (source matrix W5) and the https-only
  redirect policy (W6) stay pinned by the vendored reqwest 0.12.28 source
  check and code comments. An end-to-end test needs two local TLS servers
  and the CLI rightly refuses plain http. One sharpening: the comment pin
  is version-specific, so re-verify `remove_sensitive_headers` behavior
  whenever the reqwest pin moves.
- Remote-extends-remote over real `git+https://` / `git+ssh://` stays
  unit-level (the pass-through arm of `resolve_extend_source`); an
  end-to-end run needs a git server.
- Memoization and single-capture `env.now` (resolution matrix X2/X4)
  remain by-construction rows; finding 3 above is the sharp edge worth
  fixing in that area, after which a test can pin batch behavior.
- The nine pending lint rules now provably fire
  (`pending_rules_fire_from_scratch_packages`) but still lack canonical
  fixtures with asserted stage, entity, and location. Mechanical follow-up;
  the pending table in `tests/package_lint.rs` is the worklist.

## Cross-matrix consistency

No contradictions found between matrices. The seams checked: composition
E2 versus the source matrix on staged escapes (consistent; the docs are the
odd one out, finding 5); sibling sample additivity (composition B10) versus
governance sample rows (G11a/G11b, consistent); index discovery rows versus
LSP overlay rows (consistent, D11/D12 both pinned); fallback semantics
between the SDK load matrix (F1-F5) and the refresh matrix (S2/R5-R7,
consistent, including the deliberate no-second-hop rule); diagnostics
identity between the lint matrix and the Lua matrix (consistent, the
reservation is now enforced and tested from both sides).

### 11. The address grammar still offers `values` targets for a retired concept

Found while discussing finding 6 (added 2026-07-05).

`/variables/<id>/values` and `/variables/<id>/values/<key>` are accepted
registration targets, but `[values]` is the legacy variable format and
`rototo/variable-values-disallowed` rejects it as an error. The index still
projects inline values (correctly, so the disallowed diagnostic can point
at each one), which is what the target binds to; in a lint-clean package
the target can never match anything. A rule registered against it is dead
code that registers without complaint.

Also corrects finding 6's example: the live grammar collision for
namespaced ids is with `rules` (and `entries`/`samples`), not `values`.
A variable named `payments/rules` is legal today; `values` should not be
part of the collision analysis because it should likely leave the grammar
altogether.

Decide: drop the two `values` address forms (a breaking change for any Lua
rule that names them, though such rules cannot fire in valid packages), or
keep them until the legacy format's index modeling goes too. Dropping them
also simplifies the finding-6 escaping decision by one reserved word.
