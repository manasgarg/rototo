# Runtime configuration use cases

This is the demand side of rototo: the problems a SaaS company actually has that
runtime configuration solves. Everything else hangs off this list. Each
demonstration package exists to show one or more of these use cases done well,
and the docs are structured the same way: start from a problem the reader
recognizes, then show the package that handles it.

Every use case carries its happy path and its hard part. The happy path is what
a feature tour shows. The hard part is what production actually serves you, and
it is where a reader decides whether to trust the tool. A package earns that
trust by demonstrating the hard part, or by stating honestly that it is out of
scope and whose job it is instead.

Status legend:

- **demonstrated**: shown in a worked example package.
- **settled**: designed (see `tenant-configuration.md`), not yet demonstrated.
- **open**: a real design question rototo has not answered yet. Roadmap material.
- **boundary**: deliberately not rototo's job. The docs must say so and name whose job it is.

A retired prototyping exercise (the northwind packages, grounded in Adobe and
Stripe API shapes) validated much of the data model; those packages are
back-of-the-envelope reference material only and will not become tests, docs, or
examples. Nothing counts as demonstrated until a fresh, brand-neutral package
shows it, so rows the exercise covered are marked "settled, prototyped".

## 1. Release control

Decouple deploying code from releasing behavior.

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Feature flag | a `bool` variable gates a code path | flags never die: ownership, expiry, staleness debt | happy: settled, prototyped. Hard: open (lifecycle metadata + staleness lint) |
| Gradual rollout | an allocation arm grows 5% to 100% by editing a bucket range | which ramp edits keep existing users stable, and which reshuffle them | happy: settled, prototyped. Hard: open (cross-version assignment-stability lint) |
| Ring / cohort release | rules on account facts: employees, then beta, then all | where the cohort fact comes from and staying honest that it is context, not config | settled |
| Kill switch | a `bool` variable with a safe default, flipped by commit | 3am latency: review + CI takes minutes, incidents need seconds | happy: settled. Hard: open (break-glass path with post-hoc review) |
| Deprecation gate | a rollout in reverse, tenant by tenant | tracking who is still on the old path is telemetry, not config | happy: settled. Hard: boundary (consumer's telemetry) |

## 2. Experimentation

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| A/B test | layer + allocation + arms mapped to values | control must equal baseline, unenrolled must equal default; lintable | settled, prototyped |
| Mutual exclusion | allocations in one layer claim disjoint buckets | knowing when two experiments need exclusion is judgment, not mechanism | settled, prototyped |
| Holdback | leave buckets unclaimed | remembering it exists when reading "100% rolled out" | settled, prototyped |
| Concluding an experiment | fold the winner into the base, delete the allocation | the cleanup discipline; stale experiments are flag debt squared | open (a worked before/after example plus lint on `status`) |
| Identity | hash a stable unit (device id, user id) | the unit changes mid-session (anonymous becomes logged-in), splitting assignment | boundary (identity resolution is the app's job; docs must warn) |
| Measurement | rototo assigns and traces | exposure logging, stats, sample-ratio checks, auto-rollback on regression | boundary (the consumer's experimentation loop) |

## 3. Operational tuning

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Performance knobs | timeouts, batch sizes, retry budgets as variables with rules | a changed value hits 100% of traffic at once; config changes cause outages at code-change rates | happy: settled, prototyped. Hard: open (canarying a value change) |
| Degradation policy | ordered shedding: recommendations off before search off | expressing "in what order" so services agree | settled (a list variable; worth demonstrating) |
| Observability dials | log level and sampling per service or tenant | hot-path resolution cost when every request reads config | happy: settled. Hard: open (resolution caching guidance) |
| Consistency across services | one commit changes several variables atomically | consumers refresh independently, so effects are not atomic; version skew mid-change | boundary plus open (docs honesty now; version pinning in refresh later) |

## 4. Plans, entitlements, and pricing

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Plan to entitlements | a query projects a product's feature list | many-to-many joins beyond one hop | settled, prototyped; joins: open |
| Plan limits and quotas | numeric ceilings per tier, overridable per contract | contract lifecycle: start, expiry, grace periods, all dated | happy: settled, prototyped. Hard: open (effective-dating discipline) |
| Pricing tables | keyed query: exactly one price per (plan, market) | totality: a missing cell in the plan x market cross-product is a revenue incident | happy: settled, prototyped. Hard: open (completeness lint over enum cross-products) |
| Effective-dated changes | author the March 1 price in February, filter on `env.now` | timezone semantics of "March 1", and caches that must expire at the boundary | open |
| Grandfathering | cohort-pinned pricing: plans as of when you signed up | frozen old cohorts beside evolving new ones, forever | open (the hardest business-model problem on this list) |

## 5. Tenant customization

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Per-tenant overrides | an overlay package with governed grants | the vendor's base must evolve without breaking hundreds of overlays | happy: settled, prototyped. Hard: open (compatibility gate) |
| White-label / multi-brand | brand overlays add, patch, tombstone within grants | brand count scaling: review load, drift detection across overlays | settled, prototyped; scaling: open |
| Delegated administration | overlay governance narrows for the layers below it | roles within a layer (finance approves pricing) | happy: settled, prototyped. Hard: settled as a boundary (lean on CODEOWNERS and branch protection; docs must show the pattern) |

## 6. Personalization and decisioning

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Ranked selection | query: filter, sort, top entry wins | the one-hop reference dereference built-in (name, signature, one hop only) | settled, prototyped; built-in: open |
| Audience-conditioned content | audiences as a catalog with expression-typed conditions | audiences are conditions over facts, never enumerated ID lists | settled, prototyped; the ID-list boundary must be stated |
| Strategy selection | an enum variable picks the algorithm | keeping strategies data, not code | settled, prototyped |

## 7. Compliance and regional policy

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Jurisdiction gates | a rule turns a feature off in a region | the gate must dominate everything: no experiment or tenant override may re-enable it | open (a "deny above all layers" need our governance does not express) |
| Regional policy values | retention days, consent flags by market | proving to an auditor what the config was on a date: git history is the answer, docs must show it | settled (git is the audit log; demonstrate the workflow) |
| Safety thresholds | fraud and moderation cutoffs as variables | threshold changes want canarying too (see value-change canary) | settled; canary: open |

## 8. Provider routing and migrations

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Provider selection and failover | a catalog of providers, a variable picks per region | failover speed is the break-glass problem again; secrets stay out (config names the provider, the app holds the key) | settled; break-glass: open; secrets: boundary |
| Traffic migration | an allocation with tenant as the unit | big-tenant skew: one tenant can be 30% of traffic, so 10% of tenants is not 10% of load | happy: settled. Hard: open (weighted units, or docs honesty) |
| AI model configuration | model, prompt version, parameters per use case and tier | the fastest-churning config a SaaS has; cost budgets need enforcement rototo cannot do | settled (worth demonstrating); budget enforcement: boundary |

## 9. Time-based change

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Campaign windows | entries with start/end, filtered on `env.now` | caches must expire when a window opens or closes | settled, prototyped |
| Scheduled activation | a rule flips at time T with no human present | resolvers cache; something must know results change at T | open (time-boundary cache invalidation) |
| Maintenance mode | a kill switch plus banner copy | coordinating the flip across services (version skew again) | settled; skew: boundary plus open |

## 10. Environment separation

| Use case | Happy path | Hard part | Status |
| --- | --- | --- | --- |
| Dev / staging / prod | environments as layers over one contract: same shape, different values | this is layering used vertically, not for tenancy; nothing in the design has been checked against it | open (needs its own small package precisely because it stresses composition differently) |

## The packages

Independent, brand-neutral scenarios, each owning the use cases it can
demonstrate best. Each is a fictional but recognizable SaaS archetype, designed
from this catalog rather than rendered from any vendor's API shapes. Package
directories carry descriptive names; any fictional product flavor lives inside
the package's README, not in its name.

| Package | Scenario | Primary use cases | State |
| --- | --- | --- | --- |
| release-ops | a product team shipping a web app: flags, rollouts, experiments, kill switches, knobs | 1, 2, 3, 9 (scheduled activation, maintenance mode) | graduated to `../examples/` |
| billing | a SaaS selling tiered plans: entitlements, pricing, quotas, grandfathering | 4 | graduated to `../examples/` |
| tenancy-decisioning | a multi-tenant platform whose tenants customize content and decisions within governed limits | 5, 6, 9 (campaign windows) | graduated to `../examples/` |
| regional-policy | a regulated SaaS choosing providers and policies per region | 7, 8 | graduated to `../examples/` |
| environments | one small service across dev/staging/prod | 10, plus 3 (knobs per environment) | graduated to `../examples/` |

Each package must confront its share of the roadmap's hard parts, not just its
happy paths: release-ops owns flag lifecycle, ramp stability, break-glass, and
the value-change canary; billing owns grandfathering and totality;
tenancy-decisioning owns the compatibility gate and the ID-list boundary;
regional-policy owns jurisdiction dominance, big-tenant skew, and the secrets
boundary; environments owns vertical layering. Ops tuning (3) threads through
all of them rather than owning a package.

## Roadmap: hard things rototo does not do yet

The distilled list, lift-ready for a README roadmap section. Each item traces to
a hard part above.

1. **Canarying a value change.** Staged rollout for a change to an existing
   variable's value, not just for new features. Config changes cause outages at
   the same rate as code changes; this is the biggest credibility gap.
2. **A break-glass path.** Kill switches need seconds; git review takes minutes
   to hours. An emergency change mechanism with mandatory post-hoc review.
3. **Assignment-stability lint.** Classify bucket-range edits between two package
   versions as safe (growing an arm) or reshuffling (everything else), via
   `rototo diff`.
4. **Flag lifecycle.** Owner and expiry metadata on variables, staleness
   warnings, and a worked "concluding an experiment" example: winner folded into
   the base, allocation removed.
5. **Grandfathering.** A cohort-pinning pattern for "plans as of when you signed
   up": frozen old cohorts beside evolving new ones.
6. **Totality lint.** "Exactly one entry for every cell of plan x market":
   completeness over enum cross-products, not just uniqueness.
7. **Jurisdiction dominance.** A deny that no lower layer, experiment, or tenant
   override can re-enable. Today's governance narrows grants; it cannot yet pin
   an outcome.
8. **Environment separation.** Dev/staging/prod as vertical layers over one
   contract. Undesigned.
9. **Time-boundary awareness.** Timezone semantics for effective dates, and
   cache invalidation when a rule is known to flip at time T.
10. **Version-skew honesty.** Consumers refresh independently; multi-variable
    changes are not atomic in effect. Docs statement now; version pinning in
    refresh as the possible mechanism later.
11. **Weighted rollout units.** Tenant-unit migrations where one tenant is 30%
    of load.
12. **The one-hop dereference built-in.** Name, signature, and the exactly-one-hop
    rule for following a catalog reference to an expression-typed field
    (`matches_audience`).
13. **Compatibility gate.** Check base-package evolution against existing tenant
    overlays before release.

## Stated boundaries: what rototo will not do

Named deliberately, so readers meet a boundary instead of a wall.

- **Exposure logging and experiment analysis.** rototo assigns and explains;
  logging exposures, computing stats, and deciding winners is the consumer's
  experimentation loop.
- **Metric-driven automatic rollback.** rototo makes state reviewable and
  revertable; watching metrics and deciding to revert is the consumer's deploy
  loop.
- **Enumerated ID lists as targeting.** Conditions over context facts, yes.
  "These 5,000 tenant ids", no: that set belongs in the application's database,
  surfaced as a context fact.
- **Secrets.** Config names the provider; the application holds the credential.
- **Identity resolution.** The app supplies a stable unit; rototo hashes it.
- **Enforcement and reconciliation.** rototo is the desired-state source that
  read-only consumers poll; Terraform-style enforcement is a downstream consumer,
  not a rototo subsystem.
