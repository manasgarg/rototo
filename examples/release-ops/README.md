# release-ops

A team ships a collaborative document editor and wants to stop tying behavior
changes to deploys. This package is their control plane: what is released to
whom, what is being tested, and the knobs operations turns during incidents.

Use cases demonstrated (numbers refer to `../../design/use-cases.md`):

| Use case | Where |
| --- | --- |
| 1 Feature flag + gradual rollout | `variables/enable_new_editor.toml` over `layers/editor.toml` |
| 1 Ring / cohort release | `variables/enable_realtime_preview.toml` |
| 1 Kill switch | `variables/disable_pdf_export.toml` |
| 1 Deprecation gate | `variables/enable_legacy_export.toml` |
| 2 A/B test, mutual exclusion, holdback | `layers/editor.toml` + `variables/toolbar_layout.toml` |
| 3 Performance knob | `variables/autosave_interval_ms.toml` |
| 3 Observability dial | `variables/log_level.toml` |
| 9 Scheduled activation | `variables/enable_v3_branding.toml` |

## The shape of the package

One layer (`editor`) hashes `user.id` into 100 buckets. The rollout claims 20,
the experiment claims 70, and 10 are a deliberate holdback. The rollout and the
experiment are mutually exclusive because they share the layer, and the layer
exists because both change the same surface. Every release decision is a small
TOML diff: ramping the rollout edits one bucket range, concluding the experiment
deletes an allocation and folds the winner into the variable's default, going GA
on the ring release flips a default and deletes two rules.

## What the tests assert

Covered by `tests/examples.rs` and `tests/package_lint.rs`:

- The same `user.id` gets the same arm on every resolution (deterministic
  assignment), and no `user.id` is in both the rollout and the experiment: a
  user in the rollout's buckets sees the experiment's default, and vice versa.
- An employee resolves `enable_realtime_preview` true, `log_level` "debug", and
  `enable_legacy_export` false; a non-beta customer gets the defaults.
- `enable_v3_branding` is false before 2026-09-15T14:00:00Z (its rule reads
  only `env.now`, so the flip needs no human present).
- The package stays lint-clean; the shape lints this package relies on
  (overlapping bucket ranges, an assign referencing an unknown arm, an arm
  value outside the variable's enum) are covered by the failure fixture at
  `tests/fixtures/packages/lint-failures`.

## Hard parts

Demonstrated here:

- **Mutual exclusion and holdback** are bucket arithmetic, reviewable in a diff.
- **Control equals baseline**: `toolbar_layout`'s control arm and default carry
  the same value, and that equality is lintable.

Open design questions this package is waiting on (tracked in the roadmap
section of `../../design/use-cases.md`):

- **Flag lifecycle.** Nothing here says who owns `enable_new_editor` or when it
  should die. Owner and expiry metadata plus staleness lint are roadmap item 4.
- **Ramp stability.** Growing `on` from `0-19` to `0-39` keeps existing users
  stable; moving it to `40-59` reshuffles everyone silently. The
  assignment-stability lint (roadmap item 3) is what would catch that in review.
- **Break-glass.** `disable_pdf_export` is only as fast as the merge path.
  Roadmap item 2.
- **Canarying a value change.** Changing `autosave_interval_ms` hits all matching
  traffic at once. Roadmap item 1.

Boundaries (deliberate, not gaps): exposure logging and experiment analysis
belong to the consumer's experimentation loop; rototo assigns arms and explains
its decisions in the trace. Identity is the app's job: `user.id` must be stable,
and if an anonymous session becomes a logged-in user, assignment changes with it.
