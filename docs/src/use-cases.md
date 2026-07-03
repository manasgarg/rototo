# Rototo Use Cases

The [concepts](./concepts.md) page explains what the pieces are. This page
answers the question that comes before that one: what do teams actually put in
a runtime configuration package?

The honest answer is that "runtime configuration" is one mechanism doing ten
different jobs. A feature flag, a pricing table, a tenant override, and a
provider failover feel like different problems, but they all reduce to the same
move: the application asks a reviewed package for a value, with runtime facts
attached. This page walks the ten jobs. For each one: the production problem,
the smallest rototo mechanism that answers it, and a worked example package in
the repository you can lint, inspect, and resolve yourself.

Five example packages carry the tour, each a fictional but recognizable SaaS:

- `examples/release-ops` - a team shipping a collaborative editor: flags,
  rollouts, experiments, kill switches, and operational knobs.
- `examples/billing` - an API platform selling tiered plans: entitlements,
  pricing, and quotas.
- `examples/tenancy-decisioning` - a multi-tenant platform whose tenants
  customize content and decisions within governed limits.
- `examples/regional-policy` - a regulated messaging SaaS choosing providers
  and policies per jurisdiction.
- `examples/environments` - one small service across dev, staging, and prod.

Every package is lint-clean and its resolutions are pinned by tests in CI, so
what the READMEs claim is what the packages do. Try one:

```sh
rototo resolve examples/regional-policy \
  --variable sms_marketing_enabled \
  --context '{"account": {"id": "a1", "jurisdiction": "eu", "plan_tier": "free"},
              "message": {"channel": "sms"}}'
```

Each package README also names the hard parts it does *not* solve yet; those
map to the roadmap section of the repository README. The point of the examples
is trust, and trust comes from stating the boundary, not hiding it.

## 1. Release control

The problem: shipping code and releasing behavior are welded together. The only
way to turn something on is a deploy, and the only way to turn it off is
another one. That makes every release an all-or-nothing event, and every
incident a race to roll back a binary.

The smallest mechanism is a `bool` variable. The code path ships dark, gated on
a variable whose default is `false`. Releasing is a one-line config change,
reviewed and merged like any other. A kill switch is the same variable used in
reverse: a safe default, flipped by a commit during an incident. A ring
release adds [rules](./concepts.md): employees first, then beta accounts, then
everyone, each ring one rule keyed on an account fact from the
[context](./concepts.md).

A gradual rollout needs one more piece, because "5% of users" is not a fact the
application can send - it is an assignment rototo must make, and make the same
way every time. That is a layer and an allocation: hash a stable unit like
`user.id` into buckets, claim a range, and grow the range by editing one line.
The [concepts page](./concepts.md) covers layers; `examples/release-ops` shows
the whole family - the rollout, the ring release
(`variables/enable_realtime_preview.toml`), the kill switch
(`variables/disable_pdf_export.toml`), and a deprecation running in reverse.

## 2. Experimentation

The problem: an A/B test is a rollout with a stricter contract. Assignment must
be deterministic (the same user sees the same arm on every request), the
control arm must behave exactly like the baseline, and two experiments touching
the same surface must never share a user, or they contaminate each other's
measurement.

Layers give you all three structurally. Allocations in one layer claim disjoint
buckets, so mutual exclusion is bucket arithmetic a reviewer can check in a
diff. Unclaimed buckets are a holdback: users who see pure baseline behavior.
And an allocation-driven variable maps arms to values, so "control equals
default" is a visible, checkable equality in one file.

In `examples/release-ops`, `layers/editor.toml` holds a rollout at 20%, an
experiment at 70%, and a 10% holdback on one line of 100 buckets;
`variables/toolbar_layout.toml` reads the experiment. One boundary to know up
front: rototo assigns arms and explains the assignment in the trace. Logging
exposures, computing statistics, and picking a winner belong to your
experimentation loop, not to rototo.

## 3. Operational tuning

The problem: timeouts, batch sizes, retry budgets, and log levels are the
values operations actually changes at 3am, and in most systems changing them
means a redeploy under pressure.

These are ordinary variables - an `int` with a default and a couple of rules is
usually the whole story. The value of putting them in a package is the review
and the blast-radius honesty: `autosave_interval_ms` and `log_level` in
`examples/release-ops` show per-condition dials (debug logging for employees,
a gentler autosave for large documents), and `examples/environments` shows the
same knobs varying per environment.

One caution this page owes you: a changed knob hits 100% of matching traffic on
the next refresh. Config changes cause outages at the same rate as code
changes, and canarying a value change is still an open roadmap item, so treat
a hot-path knob edit with deploy-level care.

## 4. Plans, entitlements, and pricing

The problem: what each plan includes, what it costs in each currency, and what
each account may do tends to be scattered across code constants, a billing
system, and tribal knowledge. Money config is where a typo is a revenue
incident.

This is catalog territory. Plans, features, and prices are
[catalogs](./concepts.md) - typed tables with JSON Schemas - and the
relationships between them are references, so a plan pointing at a feature
that does not exist is a lint failure, not a production surprise. Selection is
a [catalog query](./concepts.md): `active_price` in `examples/billing` filters
`prices` to the account's `(tier, currency)` key and takes the newest entry
whose `effective_from` is in the past. That last clause is effective dating
with no new machinery: prices are append-only, a change is a new entry
authored ahead of its date, and the October increase in
`data/catalogs/prices/team_usd_2026_10.toml` takes force at its instant with
no human present.

At resolve time references hydrate, so `active_plan` returns the whole plan
entry, feature list included, and application code checks membership against
resolved data instead of string constants.

## 5. Tenant customization

The problem: every serious customer wants something different, and the naive
answers are both bad. Fork the config per tenant and the forks drift from the
base forever; put tenant conditionals in the base and it grows a rule per
customer until nobody can review it.

Rototo's answer is [composition with governance](./concepts.md). The vendor
ships a base package; each tenant is a thin overlay that `extends` it. The
overlay can add a catalog entry, patch fields of one, delete one, or
replace a variable's `[resolve]` block wholesale - but only where the base's
`governance.toml` grants it, and grants are enforced at compose time as load
failures, not review conventions. The base's schemas and types stay the
contract: a tenant value that breaks them is a lint failure.

`examples/tenancy-decisioning` is the canonical pair: a platform `base/` and
one tenant's `acme-tenant/` overlay that exercises every composition mechanism
exactly once, including the overlay narrowing governance further for the
tenant's own teams.

## 6. Personalization and decisioning

The problem: which banner, which offer, which algorithm - per-request
decisions that product teams want to change weekly, and that harden into
unmaintainable code when they ship as `if` chains.

The mechanism is ranked selection: a query that filters candidates and sorts
by priority, so the decision logic is data in one reviewable file. In
`examples/tenancy-decisioning`, `homepage_banner` filters the `banners` catalog
by campaign window and audience, sorts by priority, and returns the top entry.
Audiences there are worth a look: each is a catalog entry holding condition
bounds over visitor facts, referenced from banners by id, so a typo'd audience
is a lint failure and the definition is reused across banners.

The boundary that keeps this healthy: an audience is a condition over context
facts, never a list of ids. "These 5,000 visitors" belongs in your database,
surfaced as a context fact the condition can test.

## 7. Compliance and regional policy

The problem: what is legal varies by jurisdiction and changes on a regulator's
schedule, not yours. And when the auditor asks what the retention policy was on
March 3rd, "someone changed a database row" is not an answer.

The mechanism is deliberately plain: variables with rules keyed on a
jurisdiction fact. `examples/regional-policy` turns marketing SMS off in the
EU (`sms_marketing_enabled`), sets `data_retention_days` per region, and holds
fraud thresholds as reviewed numbers. The part that makes it compliance-grade
is not a feature, it is the medium: the package is a git repository, so the
config in force on any date is a `git log` question, with the author and the
review attached.

One honest gap, stated in that package's README: today's governance controls
who may edit what, but it cannot yet pin an outcome so that no lower layer or
experiment can re-enable a jurisdiction gate. That dominance guarantee is an
open roadmap item.

## 8. Provider routing and migrations

The problem: the email vendor is down, the SMS carrier needs to change in one
country, and a third of your AI spend rides on which model answers support
tickets. Vendor choice changes faster than deploys and needs an audit trail.

The mechanism is a catalog of providers plus a query: filter to the active
providers for this `(channel, jurisdiction)`, sort by priority, take the top.
Failover is then a two-line data edit - mark the primary `disabled`, or
out-rank it with the backup - reviewed and logged. `examples/regional-policy`
does exactly this in `message_provider`, migrates delivery traffic tenant by
tenant through a layer (`use_new_delivery_pipeline`), and keeps AI
configuration as catalog entries where the model id, prompt version, and
parameters travel together, so a resolution can never mix prompt v14 with the
wrong model.

Secrets stay out, always: config names the provider, the application holds the
credential.

## 9. Time-based change

The problem: a campaign starts Friday at midnight, a rebrand is embargoed until
the announcement, a price takes force on October 1st. Someone should not have
to be awake for any of those.

The mechanism is `env.now`, the evaluation timestamp every
[expression](./expressions.md) can read. A campaign window is a catalog entry
with start and end fields and a query filter that tests them - the
`spring_sale` banner in `examples/tenancy-decisioning` opens and closes on its
own. A scheduled flip is one rule: `enable_v3_branding` in
`examples/release-ops` turns on at an exact UTC instant. Effective-dated
pricing in `examples/billing` is the same idea aimed at money.

The known sharp edge: resolvers that cache results must know a value changes at
the boundary instant. Time-boundary cache awareness is an open roadmap item, so
keep refresh periods short around scheduled flips.

## 10. Environment separation

The problem: dev, staging, and prod should differ in values - bucket names,
timeouts, debug endpoints - and in nothing else. In practice each environment
accumulates its own config file, and the drift between them becomes a
spreadsheet nobody trusts.

The mechanism is the same layering built for tenants, used vertically. In
`examples/environments`, `base/` carries the contract and production values -
prod is the environment that must never be wrong by omission, so it is the
layer with no overlay to forget. `dev/` and `staging/` extend the base and
carry exactly the files that differ, so environment drift is a short directory
listing. Which environment applies is a deployment-time choice: each
deployment starts with its environment's [package
source](./package-sources.md), so environment never becomes a context fact
that rules can branch on, and the three environments stay structurally
identical.

## What rototo leaves to the application

Some hard things nearby are deliberately not rototo's job. Naming them here
means you meet a boundary instead of a wall:

- **Exposure logging and experiment analysis.** Rototo assigns and explains;
  logging exposures, computing stats, and deciding winners is your
  experimentation loop.
- **Metric-driven automatic rollback.** Rototo makes state reviewable and
  revertable; watching metrics and deciding to revert is your deploy loop.
- **Enumerated ID lists as targeting.** Conditions over context facts, yes.
  "These 5,000 tenant ids", no: that set belongs in your database, surfaced as
  a context fact.
- **Secrets.** Config names the provider; the application holds the credential.
- **Identity resolution.** The application supplies a stable unit; rototo
  hashes it.
- **Enforcement and reconciliation.** Rototo is the desired-state source that
  read-only consumers poll; Terraform-style enforcement is a downstream
  consumer, not a rototo subsystem.

## Where to go next

If a section above matched your problem, open its example package and read the
README first - each one states what its tests pin and which hard parts are
still open. The [concepts](./concepts.md) page defines the vocabulary the
examples use, the [package format](./package-format.md) reference specifies
every file shape, and [Using Rototo](./adoption.md) covers getting a package
reviewed, released, and refreshed in production.
