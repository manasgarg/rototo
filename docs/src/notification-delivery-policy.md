# Notification Delivery Policy

Some configuration is policy for another runtime system. Notification delivery
is a good example. The notification service needs to know whether to deliver a
message immediately or in a digest, which channels to try, whether quiet hours
matter, and what fallback channel is allowed.

I do not want those rules scattered across notification code. I also do not
want rototo to become the notification system. The boundary I want is smaller:
rototo selects a reviewed delivery policy from runtime facts, and the
notification service enforces that policy while it owns recipients,
subscriptions, consent, retries, and delivery logs.

We will model that as `notification-config`, with one variable named
`notification-delivery-policy`.

## Start With The Boundary

The runtime question is not "is notification delivery enabled?" It is:

```text
Given this notification, account, and operating state, which reviewed delivery
policy should the notification service use?
```

The app will supply context like this:

```json
{
  "account": {
    "plan": "enterprise"
  },
  "notification": {
    "kind": "incident_update"
  },
  "incident": {
    "active": true
  }
}
```

Rototo will return a policy object. The notification service still decides
which recipients are eligible, whether a user has opted out, whether an email
address is verified, how quiet hours map to the recipient's timezone, and how
provider retries are handled.

That is why this example uses a resource-backed variable. The variable owns
resolution. The resource owns the validated policy objects the app can consume.

## Create The Workspace

Create the workspace with a variable and a resource template:

```sh
rototo init notification-config --variable notification-delivery-policy
rototo init notification-config --resource notification-delivery-policy
```

Replace `notification-config/variables/notification-delivery-policy.toml`:

```toml
schema_version = 1

description = "Delivery policy selected for outbound notifications"
type = "resource:notification-delivery-policy"

[resolve]
default = "product_digest"
```

Replace `notification-config/resources/notification-delivery-policy.toml`:

```toml
schema_version = 1

description = "Notification delivery policy objects"
schema = "../schemas/notification-delivery-policy.schema.json"
```

The default is a digest policy. The notification service gets a valid answer
before any special runtime conditions are introduced.

## Define The Policy Shape

Before adding policy objects, define what the notification service is willing
to consume. Replace
`notification-config/schemas/notification-delivery-policy.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["delivery", "channels", "respect_quiet_hours", "fallback_channel"],
  "properties": {
    "delivery": { "type": "string", "enum": ["immediate", "digest"] },
    "channels": {
      "type": "array",
      "items": { "type": "string", "enum": ["email", "in_app", "slack"] },
      "minItems": 1,
      "uniqueItems": true
    },
    "digest_interval_hours": { "type": "integer", "minimum": 1, "maximum": 168 },
    "respect_quiet_hours": { "type": "boolean" },
    "fallback_channel": { "type": "string", "enum": ["email", "in_app", "slack"] }
  },
  "additionalProperties": false,
  "allOf": [
    {
      "if": {
        "properties": { "delivery": { "const": "digest" } },
        "required": ["delivery"]
      },
      "then": {
        "required": ["digest_interval_hours"]
      }
    }
  ]
}
```

The schema is doing production work. A digest policy must say how often the
digest runs. Every policy must declare channels, quiet-hours behavior, and a
fallback channel. Rototo validates those objects during lint, before the app
loads them.

## Add The Policy Objects

Rename the generated object file from
`notification-config/resources/notification-delivery-policy-objects/default.toml`
to
`notification-config/resources/notification-delivery-policy-objects/product_digest.toml`,
then replace its contents:

```toml
delivery = "digest"
channels = ["email", "in_app"]
digest_interval_hours = 24
respect_quiet_hours = true
fallback_channel = "in_app"
```

Create
`notification-config/resources/notification-delivery-policy-objects/security_alert.toml`:

```toml
delivery = "immediate"
channels = ["email", "in_app"]
respect_quiet_hours = false
fallback_channel = "email"
```

Create
`notification-config/resources/notification-delivery-policy-objects/enterprise_incident.toml`:

```toml
delivery = "immediate"
channels = ["email", "slack", "in_app"]
respect_quiet_hours = false
fallback_channel = "email"
```

These files are not notification messages. They are delivery policies. Product
updates can wait for a digest. Security alerts should go immediately. Active
incident updates for enterprise accounts can use Slack as one of the delivery
channels.

## Name The Runtime Conditions

Now add the conditions that select those policies.

Create `notification-config/qualifiers/security-alerts.toml`:

```toml
schema_version = 1
description = "Security notifications that should be delivered immediately"

[[predicate]]
attribute = "notification.kind"
op = "eq"
value = "security_alert"
```

Create `notification-config/qualifiers/enterprise-accounts.toml`:

```toml
schema_version = 1
description = "Enterprise plan accounts"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"
```

Create `notification-config/qualifiers/active-incidents.toml`:

```toml
schema_version = 1
description = "Requests made while an operational incident is active"

[[predicate]]
attribute = "incident.active"
op = "eq"
value = true
```

Create `notification-config/qualifiers/incident-updates.toml`:

```toml
schema_version = 1
description = "Notifications about an operational incident"

[[predicate]]
attribute = "notification.kind"
op = "eq"
value = "incident_update"
```

Those qualifiers name the raw facts. The delivery policy cares about a composed
condition: enterprise accounts receiving incident updates while an incident is
active.

Create `notification-config/qualifiers/enterprise-incident-updates.toml`:

```toml
schema_version = 1
description = "Enterprise accounts receiving active incident updates"

[[predicate]]
attribute = "qualifier.enterprise-accounts"
op = "eq"
value = true

[[predicate]]
attribute = "qualifier.active-incidents"
op = "eq"
value = true

[[predicate]]
attribute = "qualifier.incident-updates"
op = "eq"
value = true
```

Composition keeps the variable readable. The variable can select
`enterprise_incident` without repeating the raw `account.*`, `incident.*`, and
`notification.*` predicates.

## Select The Policies

Update `notification-config/variables/notification-delivery-policy.toml`:

```toml
schema_version = 1

description = "Delivery policy selected for outbound notifications"
type = "resource:notification-delivery-policy"

[resolve]
default = "product_digest"

[[resolve.rule]]
qualifier = "security-alerts"
value = "security_alert"

[[resolve.rule]]
qualifier = "enterprise-incident-updates"
value = "enterprise_incident"
```

Rule order is part of the policy. Security alerts stay first because they are
the most direct immediate-delivery path. Enterprise incident updates come next.
Everything else gets the default digest policy.

## Generate The Context Contract

The qualifiers introduced three runtime facts: `account.plan`,
`incident.active`, and `notification.kind`. Generate the context schema after
those paths exist:

```sh
rototo init notification-config --context
```

On this workspace, rototo writes
`notification-config/schemas/context.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": true,
  "properties": {
    "account": {
      "additionalProperties": true,
      "properties": {
        "plan": { "type": "string" }
      },
      "type": "object"
    },
    "incident": {
      "additionalProperties": true,
      "properties": {
        "active": { "type": "boolean" }
      },
      "type": "object"
    },
    "notification": {
      "additionalProperties": true,
      "properties": {
        "kind": { "type": "string" }
      },
      "type": "object"
    }
  },
  "type": "object"
}
```

Now lint the workspace:

```sh
rototo lint notification-config
```

Lint checks both contracts: the context facts the app must send, and the
delivery policy objects the app may receive.

## Resolve The Policy Paths

A regular product update gets the digest policy:

```sh
rototo resolve notification-config \
  --variable notification-delivery-policy \
  --context notification.kind=product_update \
  --context account.plan=standard \
  --context incident.active=false
```

```text
value key: product_digest
value: {"channels":["email","in_app"],"delivery":"digest","digest_interval_hours":24,"fallback_channel":"in_app","respect_quiet_hours":true}
```

A security alert gets immediate delivery:

```sh
rototo resolve notification-config \
  --variable notification-delivery-policy \
  --context notification.kind=security_alert \
  --context account.plan=standard \
  --context incident.active=false
```

```text
value key: security_alert
value: {"channels":["email","in_app"],"delivery":"immediate","fallback_channel":"email","respect_quiet_hours":false}
```

An enterprise account receiving an active incident update gets the incident
policy:

```sh
rototo resolve notification-config \
  --variable notification-delivery-policy \
  --context notification.kind=incident_update \
  --context account.plan=enterprise \
  --context incident.active=true
```

```text
value key: enterprise_incident
value: {"channels":["email","slack","in_app"],"delivery":"immediate","fallback_channel":"email","respect_quiet_hours":false}
```

A standard account receiving the same incident update still gets the default
digest policy. That is the part I want visible during review: the policy
changes only for the named condition.

## Use The Policy In The App

The app should resolve the delivery policy at the boundary where it is about to
enqueue or send a notification. Rototo returns the reviewed policy. The
notification service applies recipient preferences, consent, quiet-hour
calculation, provider routing, retries, and logging.

```rust
use serde::Deserialize;

use rototo::{ResolveContext, Workspace};

#[derive(Debug, Deserialize)]
struct DeliveryPolicy {
    delivery: String,
    channels: Vec<String>,
    digest_interval_hours: Option<u64>,
    respect_quiet_hours: bool,
    fallback_channel: String,
}

async fn delivery_policy(
    workspace: &Workspace,
    account_plan: &str,
    notification_kind: &str,
    incident_active: bool,
) -> Result<DeliveryPolicy, Box<dyn std::error::Error>> {
    let context = ResolveContext::from_json(serde_json::json!({
        "account": {
            "plan": account_plan
        },
        "notification": {
            "kind": notification_kind
        },
        "incident": {
            "active": incident_active
        }
    }))?;

    let resolution = workspace
        .resolve_variable("notification-delivery-policy", &context)
        .await?;
    let value_key = resolution.value_key.clone();
    let policy: DeliveryPolicy = serde_json::from_value(resolution.value)?;

    println!(
        "selected notification-delivery-policy `{}` from {:?}",
        value_key,
        workspace.source_fingerprint()
    );

    Ok(policy)
}
```

The selected value is observable. Logs and traces can show which policy key was
used, which workspace version supplied it, and which runtime facts led there.

## Keep The State Somewhere Else

This is the line I would keep coming back to in review: rototo owns reviewed
delivery rules, not notification state.

At this boundary, rototo should own:

- default delivery modes;
- channels allowed for a class of notification;
- whether a policy respects quiet hours;
- fallback behavior for a named notification path;
- policy differences by account class or operating state.

Keep these in the notification system or adjacent operational systems:

- recipient subscriptions and opt-outs;
- verified addresses and consent records;
- per-recipient quiet-hour windows;
- message IDs and delivery attempts;
- provider failures and retries;
- audit logs and customer support history.

That keeps the policy reviewable without pretending that a configuration
workspace is a delivery database. The notification service still owns the live
work. Rototo gives it a typed, versioned, explainable policy to apply.
