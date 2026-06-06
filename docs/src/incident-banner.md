# Incident Banner

The first two examples used scalar values: an integer account limit and a
boolean operational switch. The next useful modeling move is a structured value.
Some runtime configuration is not a single number or true/false decision; it is
a small object the app needs to trust before it renders it.

An incident banner is a good example. During a regional support incident, the
service may need to show a banner to affected accounts. A malformed banner is
user-visible, so I want the workspace to validate the object before the app
ever loads it.

In this example, rototo owns the reviewed decision and selected payload. The
app still owns placement, styling, localization, and whether a given page should
render a banner at all.

## Start With A Resource-Backed Variable

Create a workspace with a variable and a resource template:

```sh
rototo init communications-config --variable support-banner
rototo init communications-config --resource support-banner
```

The variable will select a named banner object. The resource will define the
schema and hold the objects the variable can select.

Replace `communications-config/variables/support-banner.toml`:

```toml
schema_version = 1

description = "Support banner shown during operational incidents"
type = "resource:support-banner"

[resolve]
default = "none"
```

Then replace `communications-config/resources/support-banner.toml`:

```toml
schema_version = 1

description = "Support banner payloads"
schema = "../schemas/support-banner.schema.json"
```

The variable now has a type, but its values live as resource objects. That split
is useful: resolution stays in the variable, while object validation belongs to
the resource.

## Define The Object Shape

Before writing banner objects, define the shape the app is willing to consume.
Replace `communications-config/schemas/support-banner.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["enabled"],
  "properties": {
    "enabled": { "type": "boolean" },
    "severity": { "type": "string", "enum": ["info", "warning", "critical"] },
    "title": { "type": "string", "minLength": 1 },
    "message": { "type": "string", "minLength": 1 },
    "link": { "type": "string" }
  },
  "additionalProperties": false,
  "allOf": [
    {
      "if": {
        "properties": { "enabled": { "const": true } },
        "required": ["enabled"]
      },
      "then": {
        "required": ["severity", "title", "message"]
      }
    }
  ]
}
```

The schema makes two production expectations explicit. A disabled banner can be
small. An enabled banner must include the fields the app needs to render
something coherent.

## Add The Banner Objects

Rename the generated object file from
`communications-config/resources/support-banner-objects/default.toml` to
`communications-config/resources/support-banner-objects/none.toml`, then replace
its contents:

```toml
enabled = false
```

Create `communications-config/resources/support-banner-objects/eu-incident.toml`:

```toml
enabled = true
severity = "warning"
title = "Delayed support responses"
message = "Support response times are slower than usual in your region."
link = "https://status.example.com"
```

These file stems, `none` and `eu-incident`, are the value keys the variable can
select. Rototo validates both objects against the resource schema during lint.

Run lint and resolve the default path:

```sh
rototo lint communications-config
rototo resolve communications-config --variable support-banner
```

With no runtime context, the workspace selects `none`:

```text
value key: none
value:
  enabled: false
```

That default matters. The app can ask for `support-banner` on every request and
receive a valid object, even when there is nothing to show.

## Name The Affected Condition

Now add the runtime condition. In this incident, only accounts operating in the
European region should see the banner.

Create `communications-config/qualifiers/eu-accounts.toml`:

```toml
schema_version = 1
description = "Accounts operating in the European region"

[[predicate]]
attribute = "account.region"
op = "eq"
value = "eu"
```

Then update the variable so the named condition selects the incident payload:

```toml
schema_version = 1

description = "Support banner shown during operational incidents"
type = "resource:support-banner"

[resolve]
default = "none"

[[resolve.rule]]
qualifier = "eu-accounts"
value = "eu-incident"
```

The variable now says the operational policy directly: no banner by default;
show the incident banner for the affected account region.

## Generate The Context Contract

The qualifier introduced a context path, `account.region`. Generate the context
schema after that path exists:

```sh
rototo init communications-config --context
```

On this workspace, rototo writes
`communications-config/schemas/context.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": true,
  "properties": {
    "account": {
      "type": "object",
      "additionalProperties": true,
      "properties": {
        "region": { "type": "string" }
      }
    }
  }
}
```

Now lint again:

```sh
rototo lint communications-config
```

This catches both sides of the contract: the app context shape used by the
qualifier, and the selected banner object shape used by the app.

## Resolve The Two Paths

Unaffected accounts get the valid empty banner:

```sh
rototo resolve communications-config \
  --variable support-banner \
  --context account.region=us
```

```text
value key: none
value:
  enabled: false
```

Affected accounts get the incident payload:

```sh
rototo resolve communications-config \
  --variable support-banner \
  --context account.region=eu
```

```text
value key: eu-incident
value:
  enabled: true
  severity: warning
  title: Delayed support responses
```

The important part is not the text itself. The important part is that the
selection and the payload are both reviewable, validated, and explainable.

## Deserialize In The App

The app should deserialize the selected value into the type it renders. That
keeps the boundary crisp: rototo returns a validated JSON value, and the app
turns that value into application behavior.

```rust
use serde::Deserialize;

use rototo::{ResolveContext, Workspace};

#[derive(Debug, Deserialize)]
struct SupportBanner {
    enabled: bool,
    severity: Option<String>,
    title: Option<String>,
    message: Option<String>,
    link: Option<String>,
}

async fn support_banner_for_request(
    workspace: &Workspace,
    account_region: &str,
) -> Result<Option<SupportBanner>, Box<dyn std::error::Error>> {
    let context = ResolveContext::from_json(serde_json::json!({
        "account": {
            "region": account_region
        }
    }))?;

    let resolution = workspace
        .resolve_variable("support-banner", &context)
        .await?;
    let value_key = resolution.value_key.clone();
    let banner: SupportBanner = serde_json::from_value(resolution.value)?;

    println!(
        "selected support-banner `{}` from {:?}",
        value_key,
        workspace.source_fingerprint()
    );

    if banner.enabled {
        Ok(Some(banner))
    } else {
        Ok(None)
    }
}
```

In a real service, I would emit the selected value key and workspace
fingerprint through the same observability path I use for the request. When a
customer asks why a banner appeared, the answer should point back to the
workspace version and rule that selected it.

## Keep Rendering In The App

Rototo should not become a content management system. It is useful here because
the banner changes production behavior and needs release discipline.

Use this pattern when:

- the payload is small enough to review in git;
- bad shape would break or degrade application behavior;
- the selected value depends on runtime context;
- rollback should be a workspace change.

Keep these concerns in the app:

- where the banner appears;
- how it is styled;
- how it is localized;
- whether a specific page has room to render it;
- request authorization and user identity.

That split keeps the model practical. The workspace owns the validated
operational payload. The app owns the product experience around it.
