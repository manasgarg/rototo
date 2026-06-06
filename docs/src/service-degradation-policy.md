# Service Degradation Policy

Incidents rarely recover because of one perfect switch. A team may reduce
concurrency, watch the queue, pause non-critical work, try a fallback provider,
then tighten or relax the posture as the system responds. I want those moves to
be reviewed and reversible, but I do not want to redeploy the service for every
adjustment during recovery.

Rototo fits the policy layer in that loop. The service still owns metrics,
queues, retries, provider health, and enforcement. Rototo selects the reviewed
operating policy from the runtime facts the service supplies.

This example uses `degradation-config` and a variable named
`service-degradation-policy`.

## Start With The Recovery Boundary

The runtime question is not "is the service healthy?" Rototo should not decide
that. The service and observability system already know queue pressure,
provider health, error rates, and retry behavior.

The useful runtime question is:

```text
Given the service state we already measured, which reviewed operating policy
should this request use?
```

The first version of that policy can be small: run normally while pressure is
normal, and reduce load when queue pressure is high.

## Create The Workspace

Create the workspace with a variable and a resource template:

```sh
rototo init degradation-config --variable service-degradation-policy
rototo init degradation-config --resource service-degradation-policy
```

Replace `degradation-config/variables/service-degradation-policy.toml`:

```toml
schema_version = 1

description = "Operating policy selected while the service is under pressure"
type = "resource:service-degradation-policy"

[resolve]
default = "normal"
```

Replace `degradation-config/resources/service-degradation-policy.toml`:

```toml
schema_version = 1

description = "Service degradation policy objects"
schema = "../schemas/service-degradation-policy.schema.json"
```

The variable chooses a policy key. The resource validates the policy object
behind that key. That split matters during an incident because the app should
not have to trust a half-shaped object while operators are making fast changes.

## Define The Policy Shape

Before adding policies, define the knobs the service is willing to apply.
Replace `degradation-config/schemas/service-degradation-policy.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": [
    "mode",
    "max_concurrency",
    "background_jobs_enabled",
    "non_critical_fanout",
    "fallback_provider"
  ],
  "properties": {
    "mode": { "type": "string", "enum": ["normal", "degraded", "severe"] },
    "max_concurrency": { "type": "integer", "minimum": 1, "maximum": 200 },
    "background_jobs_enabled": { "type": "boolean" },
    "non_critical_fanout": { "type": "string", "enum": ["send", "defer", "pause"] },
    "fallback_provider": { "type": "string", "enum": ["primary", "secondary"] }
  },
  "additionalProperties": false
}
```

The schema is deliberately operational. It says which fields the service will
honor, which modes are allowed, and how far concurrency can be pushed. If
someone tries to set `max_concurrency = 0` during an incident, lint catches
that before the workspace is released.

## Add The First Policies

Rename the generated object file from
`degradation-config/resources/service-degradation-policy-objects/default.toml`
to `degradation-config/resources/service-degradation-policy-objects/normal.toml`,
then replace its contents:

```toml
mode = "normal"
max_concurrency = 100
background_jobs_enabled = true
non_critical_fanout = "send"
fallback_provider = "primary"
```

Create
`degradation-config/resources/service-degradation-policy-objects/degraded.toml`:

```toml
mode = "degraded"
max_concurrency = 30
background_jobs_enabled = false
non_critical_fanout = "defer"
fallback_provider = "primary"
```

Create
`degradation-config/resources/service-degradation-policy-objects/severe.toml`:

```toml
mode = "severe"
max_concurrency = 10
background_jobs_enabled = false
non_critical_fanout = "pause"
fallback_provider = "secondary"
```

The `severe` policy is not selected yet. I still like defining it early because
it gives reviewers a concrete recovery posture to inspect before the team needs
it under pressure.

## Select Degraded During High Pressure

Now add the condition that moves the service from normal to degraded mode.

Create `degradation-config/qualifiers/high-queue-pressure.toml`:

```toml
schema_version = 1
description = "Service reports high queue pressure"

[[predicate]]
attribute = "service.queue_pressure"
op = "eq"
value = "high"
```

Update `degradation-config/variables/service-degradation-policy.toml`:

```toml
schema_version = 1

description = "Operating policy selected while the service is under pressure"
type = "resource:service-degradation-policy"

[resolve]
default = "normal"

[[resolve.rule]]
qualifier = "high-queue-pressure"
value = "degraded"
```

The app still decides when queue pressure is high. Rototo only turns that
runtime fact into the reviewed policy object.

## Generate The First Context Contract

The qualifier introduced `service.queue_pressure`. Generate the context schema
after that path exists:

```sh
rototo init degradation-config --context
```

On this workspace, rototo writes
`degradation-config/schemas/context.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": true,
  "properties": {
    "service": {
      "additionalProperties": true,
      "properties": {
        "queue_pressure": { "type": "string" }
      },
      "type": "object"
    }
  },
  "type": "object"
}
```

Lint the workspace:

```sh
rototo lint degradation-config
```

Then resolve both paths.

Normal pressure selects `normal`:

```sh
rototo resolve degradation-config \
  --variable service-degradation-policy \
  --context service.queue_pressure=normal
```

```text
value key: normal
value: {"background_jobs_enabled":true,"fallback_provider":"primary","max_concurrency":100,"mode":"normal","non_critical_fanout":"send"}
```

High pressure selects `degraded`:

```sh
rototo resolve degradation-config \
  --variable service-degradation-policy \
  --context service.queue_pressure=high
```

```text
value key: degraded
value: {"background_jobs_enabled":false,"fallback_provider":"primary","max_concurrency":30,"mode":"degraded","non_critical_fanout":"defer"}
```

This is the first recovery move: reduce work everywhere that reports high
pressure.

## Try A Stronger Policy On A Bucket

Sometimes the first move is not enough. Queue depth keeps climbing, the
primary provider stays slow, or deferred work is still taking too much capacity.
The next move might be `severe`, but applying it to every account at once can
be more disruption than the team needs.

A bucket gives us a stable trial path. The same account stays in or out of the
trial while the salt and range stay the same, so logs and support cases remain
explainable.

Create `degradation-config/qualifiers/degradation-trial-bucket.toml`:

```toml
schema_version = 1
description = "Stable account bucket for trying a stronger recovery policy"

[[predicate]]
attribute = "account.id"
op = "bucket"
salt = "service-degradation-recovery-2026-06"
range = [0, 1000]
```

The bucket range is on a 0 to 10000 scale, so `[0, 1000]` is ten percent.

Now compose the bucket with high pressure.

Create `degradation-config/qualifiers/severe-recovery-trial.toml`:

```toml
schema_version = 1
description = "High-pressure requests in the severe recovery trial bucket"

[[predicate]]
attribute = "qualifier.high-queue-pressure"
op = "eq"
value = true

[[predicate]]
attribute = "qualifier.degradation-trial-bucket"
op = "eq"
value = true
```

Update the variable so the severe trial wins before the broader degraded rule:

```toml
schema_version = 1

description = "Operating policy selected while the service is under pressure"
type = "resource:service-degradation-policy"

[resolve]
default = "normal"

[[resolve.rule]]
qualifier = "severe-recovery-trial"
value = "severe"

[[resolve.rule]]
qualifier = "high-queue-pressure"
value = "degraded"
```

Rule order carries the recovery intent. High-pressure requests in the trial
bucket get `severe`; the rest of the high-pressure traffic stays on
`degraded`.

## Regenerate The Context Contract

The bucket introduced `account.id`. Regenerate the context schema and review
the diff:

```sh
rototo init degradation-config --context --force
```

On this workspace, the regenerated
`degradation-config/schemas/context.schema.json` includes both runtime facts:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": true,
  "properties": {
    "account": {
      "additionalProperties": true,
      "properties": {
        "id": { "type": ["boolean", "number", "string"] }
      },
      "type": "object"
    },
    "service": {
      "additionalProperties": true,
      "properties": {
        "queue_pressure": { "type": "string" }
      },
      "type": "object"
    }
  },
  "type": "object"
}
```

Lint again:

```sh
rototo lint degradation-config
```

## Resolve The Trial Paths

`acct-0001` is outside the severe trial bucket, so high pressure still selects
`degraded`:

```sh
rototo resolve degradation-config \
  --variable service-degradation-policy \
  --context service.queue_pressure=high \
  --context account.id=acct-0001
```

```text
test: bucket salt=service-degradation-recovery-2026-06 range=[0,1000] bucket=5274
value key: degraded
```

`acct-001` is inside the bucket, so the same high-pressure state selects
`severe`:

```sh
rototo resolve degradation-config \
  --variable service-degradation-policy \
  --context service.queue_pressure=high \
  --context account.id=acct-001
```

```text
test: bucket salt=service-degradation-recovery-2026-06 range=[0,1000] bucket=540
value key: severe
value: {"background_jobs_enabled":false,"fallback_provider":"secondary","max_concurrency":10,"mode":"severe","non_critical_fanout":"pause"}
```

This is the second recovery move: try the stronger policy on a stable slice
while the rest of the pressured traffic stays on the first degraded policy.

## Iterate Through Review

Recovery may need a few variations. Because the policy lives in the workspace,
each variation can be a small reviewed diff.

To widen the severe policy without reshuffling account assignment, keep the
salt and expand the range:

```toml
[[predicate]]
attribute = "account.id"
op = "bucket"
salt = "service-degradation-recovery-2026-06"
range = [0, 3000]
```

To make the severe policy stronger without widening it, change the policy
object:

```toml
mode = "severe"
max_concurrency = 5
background_jobs_enabled = false
non_critical_fanout = "pause"
fallback_provider = "secondary"
```

To roll back the trial, remove the `severe-recovery-trial` rule or move the
bucket range back down. The service can refresh the workspace and apply the new
policy to future resolutions while the last successfully loaded workspace
stays active if a refresh fails.

Rototo does not decide whether the variation worked. The service metrics,
alerts, dashboards, and incident process still answer that. Rototo makes the
policy change reviewed, typed, reproducible, and reversible.

## Use The Policy In The App

The app should resolve the policy where it is about to apply concurrency,
fanout, background work, or provider routing. It should pass facts it already
knows: current service pressure and the account ID used for stable assignment.

```rust
use serde::Deserialize;

use rototo::{ResolveContext, Workspace};

#[derive(Debug, Deserialize)]
struct DegradationPolicy {
    mode: String,
    max_concurrency: u64,
    background_jobs_enabled: bool,
    non_critical_fanout: String,
    fallback_provider: String,
}

async fn degradation_policy(
    workspace: &Workspace,
    queue_pressure: &str,
    account_id: &str,
) -> Result<DegradationPolicy, Box<dyn std::error::Error>> {
    let context = ResolveContext::from_json(serde_json::json!({
        "service": {
            "queue_pressure": queue_pressure
        },
        "account": {
            "id": account_id
        }
    }))?;

    let resolution = workspace
        .resolve_variable("service-degradation-policy", &context)
        .await?;
    let value_key = resolution.value_key.clone();
    let policy: DegradationPolicy = serde_json::from_value(resolution.value)?;

    println!(
        "selected service-degradation-policy `{}` from {:?}",
        value_key,
        workspace.source_fingerprint()
    );

    Ok(policy)
}
```

The selected policy is not the incident state. It is one input to the service's
own backpressure and routing behavior.

## Keep The Control Loop Clear

Use rototo for:

- reviewed degradation modes;
- concurrency and fanout policy;
- fallback-provider selection;
- stable buckets for trying a stronger recovery posture;
- reversible changes during recovery.

Keep these in the service, observability system, or incident process:

- queue depth measurement;
- provider health detection;
- retry scheduling;
- per-request execution;
- metrics that show whether recovery is working;
- incident ownership and customer communication.

That boundary is what makes the model useful. The service keeps running the
live control loop. Rototo gives that loop a reviewed policy surface the team can
change, observe, widen, tighten, and roll back without changing the application
binary.
