# environments

One small service (a thumbnail renderer) configured across dev, staging, and
production. This package exists to stress the layering model VERTICALLY: the
same composition machinery built for tenancy, used for environment separation
instead. It is deliberately tiny so the layering is the whole story.

Use cases demonstrated (numbers are the use-case groups on the `use-cases` docs page):

| Use case | Where |
| --- | --- |
| 10 Environment separation | the `base/` + `dev/` + `staging/` trio |
| 3 Knobs per environment | `origin_timeout_ms`, `log_level` |

## The shape

- `base/` carries the contract (schemas, enums, the variable set) AND production
  values. Prod is the environment that must never be wrong by omission, so it is
  the layer with no overlay to forget.
- `dev/` and `staging/` extend the base and override values only. Same shape
  everywhere; an overlay carries exactly the files that differ, so environment
  drift is a short directory listing, not a spreadsheet.
- Which environment applies is a deployment-time choice: each deployment is
  started with its environment's package source. No `env.environment` context
  fact, no environment rules inside variables. That keeps environment selection
  out of the resolution path entirely.

## What the tests assert

Covered by `tests/examples.rs` and `tests/package_lint.rs`:

- The base resolves `storage_bucket` to `thumbs-prod`, dev to `thumbs-dev`,
  staging to `thumbs-staging`, with the identical variable set and types in all
  three.
- Dev flips `enable_debug_endpoints` and gets a 10s origin timeout; staging
  stays closer to prod (no timeout override). `max_upload_mb` is untouched
  everywhere: an overlay only carries what differs.
- A dev override whose value breaks the base's contract fails lint: the type
  and enum stay with the base, so a dev `log_level` outside `log_levels` is a
  lint error.

## Findings (the point of this package)

Vertical layering mostly fits, and where it rubs, the friction is informative:

1. **Governance wants a wildcard.** "Every variable is overridable by the
   environment below" is five per-variable blocks in `base/governance.toml` and
   a sixth the day someone adds a variable, with nothing forcing the sixth.
   Tenancy governance is deliberately per-entity; environment governance wants
   `[variable."*"]` or a package-level default. Open design question.
2. **Whole-block replacement fits environments well.** Environment overrides are
   exactly "replace the value wholesale", so the override model needs no
   stretching. The one-file-per-variable cost is visible but honest: dev differs
   from prod in exactly four reviewable files.
3. **Base-as-prod is a convention worth blessing in docs.** The alternative
   (a values-less abstract base plus three overlays) means prod is an overlay
   that can drift or be forgotten. Safer to make prod the floor.
4. **The contract is not fully lockable.** Overriding a variable is governed,
   and an override cannot change the base's type or enum. But an overlay may
   introduce a brand-new variable without any grant: the engine treats an
   entity a layer introduces as that layer's own. So "environments differ in
   values, never in contract" is convention plus review, not yet a hard
   guarantee. Same open question as the wildcard grant above.
5. **Environment selection belongs to deployment, not context.** The moment
   environments become context facts, every rule can branch on them and the
   environments stop being structurally identical. Keeping the choice at the
   package-source level preserves the invariant.

Boundaries: promotion pipelines (this value graduates dev to staging to prod) is
git workflow (a PR moving a change between overlay directories), not a rototo
mechanism; secrets stay out, per the usual rule.
