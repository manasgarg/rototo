# billing

An API platform sells three plan tiers in two currencies. This package is the
commercial source of truth: what each plan includes, what it costs, and what
each account may do. Money config is where mistakes are revenue incidents, so
this package is as much about the invariants as the values.

Use cases demonstrated (numbers are the use-case groups on the `use-cases` docs page):

| Use case | Where |
| --- | --- |
| 4 Plan to entitlements | `variables/active_plan.toml` (keyed query over `plans`) |
| 4 Plan limits and quotas | `variables/active_plan.toml` (`seat_limit`), `variables/api_rate_limit_per_min.toml` |
| 4 Pricing tables | `variables/active_price.toml` (keyed query over `prices`) |
| 4 Effective-dated changes | `data/catalogs/prices/team_usd_2026_10.toml` |
| 5 Per-contract override (grant only) | `governance.toml` on `api_rate_limit_per_min` |

## The discipline this package encodes

- **Prices are append-only.** A change is a new entry with a later
  `effective_from`, never an edit. `active_price` sorts by `effective_from`
  descending and filters to the past, so the newest in-force price wins and a
  future-dated increase flips at its instant with no human present.
- **Entitlements are references.** Plans point at the `features` catalog, so a
  typo'd feature key is a lint failure, and application code checks membership
  against resolved data, not string constants scattered through the codebase.
  At resolve time the references hydrate: `active_plan`'s value carries the
  full feature entries, ids included.
- **Pricing is locked to overlays.** Governance grants nothing on `prices`,
  `plans`, or `features`. The only grant is resolution override on the rate
  limit, the narrow door a negotiated contract actually needs.

## What the tests assert

Covered by `tests/examples.rs` and `tests/package_lint.rs`:

- `(team, eur)` resolves `active_price` to `team_eur_2025`; `(team, usd)`
  resolves to `team_usd_2025` today, and `team_usd_2026_10` will win from
  2026-10-01 with no edit to the package.
- `active_plan` for business includes `sso` in its hydrated `features`; the
  free plan carries exactly `api_access` and `seat_limit` 3.
- A price entry referencing an unknown tier or currency is a lint failure; a
  plan referencing an unknown feature is a lint failure (covered by the shared
  lint fixtures).
- An overlay that adds a price entry is rejected by governance ("governance
  denies add on catalog.prices"); an overlay that overrides
  `api_rate_limit_per_min` composes and resolves to its negotiated value.

## Hard parts

Demonstrated here:

- **Effective-dating** with settled machinery: append-only entries plus a
  time-filtered, time-sorted query. No new primitive needed for the happy path.

Simplified against the design:

- **Query projection.** The design resolved `plan_entitlements` and
  `seat_limit` as queries with a `map` projection over the selected plan. The
  engine's query supports `from`, `filter`, `sort`, `order`, and `limit`, but
  no projection yet, so those two variables became one `active_plan` variable
  that resolves the whole plan entry; application code reads `features` and
  `seat_limit` from it.

Open design questions this package is waiting on:

- **Totality** (roadmap item 6). The real pricing invariant is not "at most one
  price per (tier, currency, date)" but "exactly one in-force price for EVERY
  cell of tier x currency". Today that is authoring discipline; a completeness
  lint over enum cross-products is what would make a missing cell unreleasable.
- **Grandfathering** (roadmap item 5). `signed_up_at` is in the context schema
  because the honest version of this package needs cohort-pinned pricing:
  accounts keep the price table as of when they signed up. Append-only entries
  get partway there (old prices remain), but selecting by account cohort rather
  than by `env.now`, and freezing old cohorts while new ones evolve, is
  undesigned. This is the hardest business-model problem in the catalog.
- **Timezone semantics** (roadmap item 8). `effective_from` values here are
  explicit UTC instants. "Effective March 1" in a billing sense usually means a
  local-time boundary; whose midnight is a design question this package dodges
  by being explicit.

Boundaries: metering and enforcement of the rate limit and seat limit belong to
the application; rototo answers "what is the limit", not "how many were used".
