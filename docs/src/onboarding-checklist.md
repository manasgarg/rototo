# Onboarding Checklist

Some configuration is a collection, not a single value. A SaaS app might show
different onboarding steps for different account classes: a standard account
gets the usual setup path, while an enterprise account needs SSO, billing, and
data-processing steps.

That logic can grow quietly in app code. One branch handles the account plan,
another handles the operating region, and another handles a test account path
for trying the new checklist in production. I prefer putting the reviewed
policy in the workspace and letting the app render whatever step IDs rototo
selects.

We will model that as `onboarding-config`, with one variable named
`onboarding-steps`. The example covers
[list values](reference-variable-values.html),
[qualifier composition](reference-qualifiers.html), rule ordering, and a live
test path that only affects accounts marked for testing.

## Start With The Default Checklist

Create the workspace:

```sh
rototo init onboarding-config --variable onboarding-steps
```

Replace `onboarding-config/variables/onboarding-steps.toml`:

```toml
schema_version = 1

description = "Onboarding step IDs shown to an account"
type = "list"

[resolve]
default = ["create_project", "invite_teammate", "configure_profile"]
```

The values are step IDs, not display text. That is an important boundary. The
workspace selects the reviewed checklist; the app still owns labels, layout,
completion state, and per-user progress.

Lint and resolve the default:

```sh
rototo lint onboarding-config
rototo resolve onboarding-config --variable onboarding-steps
```

With no runtime context, rototo selects `standard`:

```text
source: literal
value: ["create_project","invite_teammate","configure_profile"]
```

## Add The Conditions

The enterprise checklist should not go straight to every enterprise account.
First, we want to see it live for accounts marked as test accounts. Support,
sales, and product teams can exercise the real runtime path without changing
the experience for regular accounts.

Create
[`onboarding-config/qualifiers/test-accounts.toml`](reference-qualifiers.html):

```toml
schema_version = 1
description = "Accounts marked for live configuration testing"

[[predicate]]
attribute = "account.kind"
op = "eq"
value = "test"
```

Create `onboarding-config/qualifiers/enterprise-accounts.toml`:

```toml
schema_version = 1
description = "Enterprise plan accounts"

[[predicate]]
attribute = "account.plan"
op = "eq"
value = "enterprise"
```

Create `onboarding-config/qualifiers/eu-accounts.toml`:

```toml
schema_version = 1
description = "Accounts operating in the European region"

[[predicate]]
attribute = "account.region"
op = "eq"
value = "eu"
```

Those qualifiers name raw runtime facts. Now
[compose them](reference-qualifiers.html) into the conditions the checklist
actually cares about.

Create `onboarding-config/qualifiers/test-enterprise-accounts.toml`:

```toml
schema_version = 1
description = "Test accounts on the enterprise plan"

[[predicate]]
attribute = "qualifier.test-accounts"
op = "eq"
value = true

[[predicate]]
attribute = "qualifier.enterprise-accounts"
op = "eq"
value = true
```

Create `onboarding-config/qualifiers/eu-enterprise-accounts.toml`:

```toml
schema_version = 1
description = "Enterprise accounts operating in the European region"

[[predicate]]
attribute = "qualifier.enterprise-accounts"
op = "eq"
value = true

[[predicate]]
attribute = "qualifier.eu-accounts"
op = "eq"
value = true
```

Composition keeps the vocabulary readable. The variable can talk about
`test-enterprise-accounts` and `eu-enterprise-accounts` without repeating the
raw `account.*` predicates.

Create `onboarding-config/qualifiers/test-eu-enterprise-accounts.toml`:

```toml
schema_version = 1
description = "Test accounts on the enterprise plan in the European region"

[[predicate]]
attribute = "qualifier.test-accounts"
op = "eq"
value = true

[[predicate]]
attribute = "qualifier.eu-enterprise-accounts"
op = "eq"
value = true
```

## Enable The Checklist For Test Accounts

The first live change should be narrow. Update
`onboarding-config/variables/onboarding-steps.toml` so only test enterprise
accounts receive the enterprise checklists:

```toml
schema_version = 1

description = "Onboarding step IDs shown to an account"
type = "list"

[resolve]
default = ["create_project", "invite_teammate", "configure_profile"]

[[resolve.rule]]
qualifier = "test-eu-enterprise-accounts"
value = ["create_project", "invite_teammate", "configure_sso", "review_data_processing", "add_billing_contact"]

[[resolve.rule]]
qualifier = "test-enterprise-accounts"
value = ["create_project", "invite_teammate", "configure_sso", "add_billing_contact"]
```

This is the first PR I would ship. The production service can refresh the
workspace, and test accounts exercise the same SDK resolution path as everyone
else, including the EU-specific variant. Regular enterprise accounts still get
the default checklist until the team is ready to widen the rule.

## Generate The Context Contract

The qualifiers introduced three runtime facts: `account.kind`, `account.plan`,
and `account.region`. Generate the
[context schema](reference-context.html) after those paths exist:

```sh
rototo init onboarding-config --context
```

On this workspace, rototo writes
`onboarding-config/schemas/context.schema.json`:

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
        "kind": { "type": "string" },
        "plan": { "type": "string" },
        "region": { "type": "string" }
      }
    }
  }
}
```

Lint the workspace:

```sh
rototo lint onboarding-config
```

Now resolve the paths that matter before the app relies on the change.

A standard account receives the standard checklist:

```sh
rototo resolve onboarding-config \
  --variable onboarding-steps \
  --context account.kind=regular \
  --context account.plan=standard \
  --context account.region=us
```

```text
source: literal
```

A regular enterprise account still receives the standard checklist:

```sh
rototo resolve onboarding-config \
  --variable onboarding-steps \
  --context account.kind=regular \
  --context account.plan=enterprise \
  --context account.region=us
```

```text
source: literal
```

A test enterprise account receives the enterprise checklist:

```sh
rototo resolve onboarding-config \
  --variable onboarding-steps \
  --context account.kind=test \
  --context account.plan=enterprise \
  --context account.region=us
```

```text
source: literal
```

A test EU enterprise account receives the EU-specific checklist:

```sh
rototo resolve onboarding-config \
  --variable onboarding-steps \
  --context account.kind=test \
  --context account.plan=enterprise \
  --context account.region=eu
```

```text
source: literal
```

That is the live test loop: the workspace is deployed, the application resolves
real runtime context, and only accounts marked for testing see the new
checklist.

## Promote With Rule Ordering

After the test accounts prove the checklist works in the running service, widen
the policy in a second PR. I would keep the test-account rules in place as an
ongoing canary path, then add the wider rules after them:

```toml
[resolve]
default = "standard"

[[resolve.rule]]
qualifier = "test-eu-enterprise-accounts"
value = "eu_enterprise"

[[resolve.rule]]
qualifier = "test-enterprise-accounts"
value = "enterprise"

[[resolve.rule]]
qualifier = "eu-enterprise-accounts"
value = "eu_enterprise"

[[resolve.rule]]
qualifier = "enterprise-accounts"
value = "enterprise"
```

[Rules are evaluated in order](reference-variable-resolution.html). The more
specific conditions come first because an EU enterprise account also matches
`enterprise-accounts`. If the general enterprise rule came first, rototo would
select `enterprise` and never reach the EU-specific checklist. The same
ordering protects the test-account canary path.

Resolve the promoted paths:

```sh
rototo resolve onboarding-config \
  --variable onboarding-steps \
  --context account.kind=regular \
  --context account.plan=enterprise \
  --context account.region=us
```

```text
source: literal
```

```sh
rototo resolve onboarding-config \
  --variable onboarding-steps \
  --context account.kind=regular \
  --context account.plan=enterprise \
  --context account.region=eu
```

```text
source: literal
```

The important habit is not only that the final rule order is right. It is that
the team had a production-shaped test path before widening the policy.

## Use The Step IDs In The App

The app should [deserialize the selected list](application-integration.html)
and map each step ID to app-owned content and completion state.

:::sdk-snippet onboarding-checklist-app
```rust
use rototo::{ResolveContext, Workspace};

async fn onboarding_steps_for_account(
    workspace: &Workspace,
    kind: &str,
    plan: &str,
    region: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let context = ResolveContext::from_json(serde_json::json!({
        "account": {
            "kind": kind,
            "plan": plan,
            "region": region
        }
    }))?;

    let resolution = workspace
        .resolve_variable("onboarding-steps", &context)
        .await?;
    let source = resolution.source.clone();
    let steps: Vec<String> = serde_json::from_value(resolution.value)?;

    println!(
        "selected onboarding-steps `{}` from {:?}",
        source,
        workspace.source_fingerprint()
    );

    Ok(steps)
}
```

```python
async def onboarding_steps_for_account(
    workspace: rototo.Workspace,
    kind: str,
    plan: str,
    region: str,
) -> list[str]:
    context = {
        "account": {
            "kind": kind,
            "plan": plan,
            "region": region,
        },
    }
    resolution = await workspace.resolve_variable("onboarding-steps", context)
    steps = list(resolution.value)

    print(f"selected onboarding-steps `{resolution.source}`")
    return steps
```

```typescript
async function onboardingStepsForAccount(
  workspace: Workspace,
  kind: string,
  plan: string,
  region: string,
): Promise<string[]> {
  const context = {
    account: { kind, plan, region },
  };
  const resolution = await workspace.resolveVariable(
    "onboarding-steps",
    context,
  );

  console.log(`selected onboarding-steps \`${resolution.source}\``);
  return resolution.value as string[];
}
```

```java
List<String> onboardingStepsForAccount(
    Workspace workspace,
    String kind,
    String plan,
    String region
) throws Exception {
    VariableResolution resolution = workspace
        .resolveVariable(
            "onboarding-steps",
            Map.of("account", Map.of(
                "kind", kind,
                "plan", plan,
                "region", region
            ))
        )
        .get();

    @SuppressWarnings("unchecked")
    List<String> steps = (List<String>) resolution.value();
    System.out.printf("selected onboarding-steps `%s`%n", resolution.source());
    return steps;
}
```

```go
func onboardingStepsForAccount(
    ctx context.Context,
    workspace *rototo.Workspace,
    kind string,
    plan string,
    region string,
) ([]string, error) {
    resolution, err := workspace.ResolveVariable(
        ctx,
        "onboarding-steps",
        map[string]any{
            "account": map[string]any{
                "kind":   kind,
                "plan":   plan,
                "region": region,
            },
        },
        nil,
    )
    if err != nil {
        return nil, err
    }

    payload, err := json.Marshal(resolution.Value)
    if err != nil {
        return nil, err
    }

    var steps []string
    if err := json.Unmarshal(payload, &steps); err != nil {
        return nil, err
    }

    fmt.Printf("selected onboarding-steps `%s`\n", resolution.Source)
    return steps, nil
}
```
:::

Rototo selects a reviewed list of step IDs. The app still owns the step labels,
the UI, completion state, and whether a user has already finished a step.

## Keep State Out Of The Workspace

This pattern fits rototo when the checklist policy changes through review and
should be explainable at runtime.

Use it for:

- plan-specific onboarding paths;
- region-specific setup requirements;
- test-account rollout before a wider enablement;
- reviewed changes to which step IDs the app should offer.

Keep these out of rototo:

- per-user onboarding progress;
- account records;
- whether a user dismissed a step;
- analytics events;
- rollout assignments owned by a separate system.

The workspace should answer which checklist applies. The application should own
what each user has done with that checklist.
