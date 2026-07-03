# regional-policy

A customer-messaging SaaS sends email and SMS on behalf of its customers across
four jurisdictions. What is legal, how long data is kept, which vendor carries
each message, and which AI model answers support tickets all vary by region and
change faster than deploys. This package is that policy, reviewable.

Use cases demonstrated (numbers refer to `../../design/use-cases.md`):

| Use case | Where |
| --- | --- |
| 7 Jurisdiction gates | `variables/sms_marketing_enabled.toml` |
| 7 Regional policy values | `variables/data_retention_days.toml` |
| 7 Safety thresholds | `variables/fraud_review_threshold.toml` |
| 8 Provider selection and failover | `catalogs/providers` + `variables/message_provider.toml` |
| 8 Traffic migration | `layers/delivery_migration.toml` + `variables/use_new_delivery_pipeline.toml` |
| 8 AI model configuration | `catalogs/models` + `variables/assistant_model.toml` |

## What the tests assert

Covered by `tests/examples.rs` and `tests/package_lint.rs`:

- An EU account resolves `sms_marketing_enabled` false and
  `data_retention_days` 30; a US account gets true and 90.
- `(email, us)` resolves `message_provider` to `email_us_primary` because the
  query filters to `status == "active"` and sorts by priority; a commit marking
  the primary `disabled` (or out-ranking it with the backup) flips the same
  context to `email_us_backup` with no other change.
- A paid account resolves `assistant_model` to `premium`; the resolved entry
  carries `model_id`, `prompt_version`, and parameters as one coherent value.
- The same `account.id` always lands on the same side of the migration.
- A provider entry with an unknown channel or jurisdiction is a lint failure
  (the enum reference machinery is covered by the shared lint fixtures).

## Hard parts

Demonstrated here:

- **Failover is a data edit, not a deploy**: flip `status` or `priority`, and
  the query picks the backup. The full audit trail is the git log.
- **AI config as a catalog**: the model, prompt version, and parameters travel
  together as one entry, so a resolution can never mix prompt v14 with the
  wrong model.

Open design questions this package is waiting on:

- **Jurisdiction dominance** (roadmap item 7). The gate must dominate: no tenant
  override, no experiment arm, no lower layer may re-enable marketing SMS in
  the EU. Today's governance narrows edit rights but cannot pin an outcome.
  This package is the reason that item exists.
- **Break-glass** (roadmap item 2). Provider failover competes with review
  latency exactly as kill switches do.
- **Big-tenant skew** (roadmap item 11). The migration hashes `account.id`, so
  10% of accounts is not 10% of traffic when one account sends a third of all
  messages. Weighted units, or documented honesty that ramps are account-counted.
- **Totality** (roadmap item 6). Every (channel, jurisdiction) cell needs an
  active provider; that is the same cross-product completeness lint pricing
  needs.

Boundaries: secrets never live here (config names the provider, the app holds
the credential); enforcing AI spend budgets is metering, the application's job;
choosing to fail over automatically on vendor errors is the application's
circuit breaker, rototo just makes the switch reviewable.
