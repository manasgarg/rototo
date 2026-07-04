# Refresh lifecycle test matrix

`RefreshingPackage` (`src/sdk/refresh.rs`) is the state machine long-running
services live on: it loads once at startup, keeps serving last-known-good
through failures, probes for changes, recovers a degraded start from the
fallback, and narrates all of it through status and events. This file is the
executable inventory of that machine's promises, in the same form as
`tests/composition-matrix.md`. Unless a row says otherwise, tests live in
`tests/sdk.rs`.

Load semantics (what a single `Package::load` does, fallback selection
included) belong to `tests/sdk-load-matrix.md`; this file starts where the
loaded package becomes the serving state of a running process.

## 1. Startup

| # | Given / When | Then | Coverage |
|---|---|---|---|
| S1 | a healthy primary | the package serves, `last_success` is the load time, the startup event is `loaded` | `refreshing_package_snapshot_includes_identity_and_last_event` |
| S2 | a failed primary and a healthy fallback | the fallback serves; `serving_fallback()` is true on status, snapshot, and snapshot JSON (`servingFallback`); the startup event is `fallback_loaded` carrying the primary failure; `last_error` records it | `refreshing_package_starts_on_the_fallback_and_recovers_the_primary` |
| S3 | a primary pinned to an immutable ref | `status().immutable` is true and no background loop spawns: an immutable source can never produce a new refresh result | `refreshing_package_pinned_git_commit_is_immutable` |
| S4 | a fallback start whose fallback source is immutable | the refresh loop still runs: the primary's mutability is unknown while degraded (`immutable && !serving_fallback`) | `refreshing_package_starts_on_the_fallback_and_recovers_the_primary` (recovery proves the loop path stays live) |

## 2. Refresh outcomes

| # | When a refresh attempt... | Then... | Coverage |
|---|---|---|---|
| R1 | finds the source changed | the new package swaps in atomically; future resolutions see it; the event is `refreshed` with previous and current identity | `refreshing_package_manual_refresh_updates_git_source`, `refreshing_package_refresh_event_reports_previous_and_current` |
| R2 | probes the source as unchanged | no reload happens; the event is `unchanged` and the serving package is untouched | `refreshing_package_unchanged_git_source_skips_reload`, `refreshing_package_unchanged_emits_unchanged_event` |
| R3 | discovers the ref is an immutable commit | the outcome is `Immutable`, periodic refresh stands down | `refreshing_package_pinned_git_commit_is_immutable` |
| R4 | fails (fetch, parse, lint) | the previous package keeps serving; `consecutive_failures` increments; `last_error` records the failure; the event is `failed` and identity is unchanged | `refreshing_package_failed_refresh_keeps_last_loaded_git_package`, `refreshing_package_failed_refresh_emits_failed_event_and_keeps_identity` |
| R5 | fails while serving the fallback | the fallback keeps serving; there is no second fallback hop at refresh time; `serving_fallback` stays true | `refreshing_package_on_fallback_keeps_serving_while_the_primary_stays_down` |
| R6 | succeeds while serving the fallback | the refresh targeted the primary, so recovery is an ordinary `refreshed` event and `serving_fallback` clears everywhere | `refreshing_package_starts_on_the_fallback_and_recovers_the_primary` |
| R7 | runs while serving the fallback | change probing is skipped (`SourceProbe::Unknown` forced): the current fingerprint describes the fallback, and comparing the primary against it would be a lie | by construction in `refresh_once_inner`; observable through R6, where recovery happens even though the primary's bytes may equal a stale fingerprint |
| R8 | runs against a multi-layer (extends) graph | any changed layer triggers a reload; a parent changing is enough | `refreshing_package_refreshes_when_parent_layer_changes` |

## 3. The background loop

| # | Given / When | Then | Coverage |
|---|---|---|---|
| B1 | a period is configured | the loop refreshes on schedule and picks up source changes without any call from the app | `refreshing_package_background_loop_refreshes_local_source` |
| B2 | consecutive failures | attempts back off exponentially from the configured minimum, capped at the maximum, and the serving snapshot stays intact | `refreshing_package_background_failures_back_off_and_keep_snapshot` (behavior), `failure_backoff_doubles_from_the_minimum_and_caps_at_the_maximum`, `failure_backoff_with_equal_bounds_is_constant` (`src/sdk/refresh.rs`, formula incl. shift saturation at huge counts) |
| B3 | `shutdown()` is called | the loop stops; no further refreshes happen | `refreshing_package_shutdown_stops_background_refresh` |
| B4 | the handle is dropped without `shutdown()` | the task is aborted best-effort by `Drop` | by construction (`impl Drop for RefreshingPackage`); the graceful path is B3 |
| B5 | a manual `refresh_now()` overlaps in-flight resolution | resolution never blocks on the refresh lock; readers see either the old or the new package, never a torn state | `refreshing_package_resolves_while_manual_refresh_runs` |
| B6 | a local (non-temporary) source refreshes | resolution runs against a snapshot of the source, so editing files on disk mid-resolution cannot tear a read | `refreshing_package_snapshots_local_source_for_last_known_good_resolution` |

## 4. Status, staleness, events

| # | Given / When | Then | Coverage |
|---|---|---|---|
| T1 | `stale(max_staleness)` | true exactly when the last successful load is older than the window | `staleness_measures_the_age_of_the_last_success` (`src/sdk/refresh.rs`) |
| T2 | a status with no recorded success | never stale (in practice startup always records one; the arm guards hand-built statuses) | `a_status_with_no_success_is_never_stale` (`src/sdk/refresh.rs`) |
| T3 | subscribing to refresh events | subscribers receive the events the lifecycle emits | `refreshing_package_subscription_receives_refreshed_event` |
| T4 | any event serialized to JSON | the shape is stable: `schemaVersion` 1, camelCase keys, `eventType` strings (`loaded`, `refreshed`, `unchanged`, `failed`, `immutable`, `fallback_loaded`) | `refresh_event_json_shape_is_stable` |
| T5 | the snapshot | carries identity, last event, `immutable`, and `servingFallback` in both the struct and its JSON | `refreshing_package_snapshot_includes_identity_and_last_event`, `refreshing_package_starts_on_the_fallback_and_recovers_the_primary` |

Cross-language note: `serving_fallback` and the fallback startup path run in
all four language SDKs through the shared contract suite
(`tests/sdk-contract/cases.jsonl`, operation `load_package_with_fallback`);
status and snapshot JSON pass through the bindings untranslated.

Notes for the review pass:

- A fallback start records the fallback load as `last_success`, so
  `stale()` measures from the degraded start, not from when the primary
  last succeeded. The status doc comment suggests pairing `stale()` with
  `serving_fallback()` to alarm on running degraded too long; that works,
  but "time since the primary was last healthy" is not directly readable.
  Worth deciding whether that needs its own field.
- `refresh_now()` on an immutable primary returns `Immutable` but the
  background loop never runs to discover immutability when the source was
  not pinned at load time and becomes pinned later (a re-pointed channel
  URL). Probably fine; recorded for completeness.

## Current gap tally

0 GAP rows. Two design notes recorded above for the review pass.

When you add or change refresh behavior, add the row and the test together;
an empty Coverage cell is a regression in this file's contract.
