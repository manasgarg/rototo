# Operational Switches

Operational switches are the values I reach for when production behavior needs
to change quickly, but not casually. During an incident, you may need to stop
new project creation while existing projects keep working. That decision should
have a diff, review, lint, history, rollback, and a clear answer to "why did
this request get blocked?"

That is a good fit for rototo. The switch is not user state, queue state, or an
authorization decision. It is reviewed operational policy that the app can
refresh and apply at runtime.

I will keep the workspace small: `operations-config`, with one service boundary
that checks whether project creation is enabled.

## Start With A Broad Switch

The first version does not need runtime context. Either project creation is
enabled for everyone, or it is disabled for everyone.

Create a workspace with one [variable](reference-variables.html):

```sh
rototo init operations-config --variable project-creation-enabled
```

Replace `operations-config/variables/project-creation-enabled.toml`:

```toml
schema_version = 1

description = "Whether accounts can create new projects"
type = "bool"

[resolve]
default = true
```

The variable has one direct [typed value](reference-variable-values.html).
Rules can add more values later without introducing a separate value table.

Lint and resolve the switch:

```sh
rototo lint operations-config
rototo resolve operations-config --variable project-creation-enabled
```

Because this switch has no rules yet, rototo evaluates it with `{}` context and
selects the default value:

```text
variable: project-creation-enabled
  pathway:
    default -> true
  result:
    source: literal
    value: true
```

## Check The Switch In The App

The app should [ask rototo for the switch](reference-sdk-resolution.html) at
the boundary where it matters. In this case, that is the project creation path.
The app still owns authorization, request validation, and database writes;
rototo only answers the operational policy question.

:::sdk-snippet operational-switch-app
```rust
use rototo::{ResolveContext, Workspace};

async fn create_project(workspace: &Workspace) -> Result<(), Box<dyn std::error::Error>> {
    let context = ResolveContext::from_json(serde_json::json!({}))?;
    let resolution = workspace
        .resolve_variable("project-creation-enabled", &context)
        .await?;

    let source = resolution.source.clone();
    let creation_enabled: bool = serde_json::from_value(resolution.value)?;

    if !creation_enabled {
        println!(
            "project creation blocked by rototo value `{}` from {:?}",
            source,
            workspace.source_fingerprint()
        );
        return Ok(());
    }

    // Validate the request, authorize the account, and create the project.
    Ok(())
}
```

```python
async def create_project(workspace: rototo.Workspace) -> None:
    resolution = await workspace.resolve_variable(
        "project-creation-enabled",
        {},
    )
    creation_enabled = bool(resolution.value)

    if not creation_enabled:
        print(
            "project creation blocked by rototo "
            f"value `{resolution.source}`"
        )
        return

    # Validate the request, authorize the account, and create the project.
```

```typescript
async function createProject(workspace: Workspace): Promise<void> {
  const resolution = await workspace.resolveVariable(
    "project-creation-enabled",
    {},
  );
  const creationEnabled = Boolean(resolution.value);

  if (!creationEnabled) {
    console.log(
      `project creation blocked by rototo value \`${resolution.source}\``,
    );
    return;
  }

  // Validate the request, authorize the account, and create the project.
}
```

```java
void createProject(Workspace workspace) throws Exception {
    VariableResolution resolution = workspace
        .resolveVariable("project-creation-enabled", Map.of())
        .get();
    boolean creationEnabled = (Boolean) resolution.value();

    if (!creationEnabled) {
        System.out.printf(
            "project creation blocked by rototo value `%s`%n",
            resolution.source()
        );
        return;
    }

    // Validate the request, authorize the account, and create the project.
}
```

```go
func createProject(ctx context.Context, workspace *rototo.Workspace) error {
    resolution, err := workspace.ResolveVariable(
        ctx,
        "project-creation-enabled",
        map[string]any{},
        nil,
    )
    if err != nil {
        return err
    }

    creationEnabled, _ := resolution.Value.(bool)
    if !creationEnabled {
        fmt.Printf(
            "project creation blocked by rototo value `%s`\n",
            resolution.Source,
        )
        return nil
    }

    // Validate the request, authorize the account, and create the project.
    return nil
}
```
:::

I like this placement because the app code stays honest about the boundary.
Rototo is not deciding who the user is or whether they have permission. It is
deciding whether this reviewed operational path is open right now.

## Disable Through Review

If you need a global pause, change the selected default:

```toml
[resolve]
default = "disabled"
```

Run lint, open the smallest PR that explains the change, and merge it through
the same path as a code change:

```sh
rototo lint operations-config
git add operations-config
git commit -m "Disable project creation during incident"
```

Long-running services that use
[`RefreshingWorkspace`](reference-sdk-refresh.html) keep serving the last
successfully loaded workspace until a refresh succeeds. After the merge reaches
the workspace source, a successful refresh affects future project creation
checks. If a refresh fails, the service keeps the last known-good workspace
active.

Rollback follows the same path: revert the change, or make a new reviewed change
that sets the default back to `enabled`.

## Scope The Switch

A global switch helps when the whole system is affected. More often, the
incident has a boundary: one region, one account class, or one integration. I
prefer adding that boundary to the workspace instead of scattering `if` checks
through app code.

Restore the default to `enabled`, then create
[`operations-config/qualifiers/eu-accounts.toml`](reference-qualifiers.html):

```toml
schema_version = 1
description = "Accounts operating in the European region"

when = 'context.account.region == "eu"'
```

Now update `operations-config/variables/project-creation-enabled.toml` so only
that named condition selects the disabled value:

```toml
schema_version = 1

description = "Whether accounts can create new projects"
type = "bool"

[resolve]
default = true

[[resolve.rule]]
when = 'qualifier["eu-accounts"]'
value = false
```

The rule says exactly what the incident response means: disable project
creation for accounts in the European region; keep the default open.

## Generate The Context Contract

The qualifier now reads `account.region`. That is the right moment to generate
the [context schema](reference-context.html) skeleton from the workspace:

```sh
rototo init operations-config --context
```

On this workspace, rototo writes
`operations-config/request-contexts/request.schema.json`:

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

The schema makes the app contract explicit. If the workspace reads
`account.region`, the app must send context with that shape when it resolves
the switch.

Lint the workspace:

```sh
rototo lint operations-config
```

Then resolve both paths:

```sh
rototo resolve operations-config \
  --variable project-creation-enabled \
  --context account.region=us
```

```text
source: literal
value: true
```

```sh
rototo resolve operations-config \
  --variable project-creation-enabled \
  --context account.region=eu
```

```text
source: literal
value: false
```

Now the app supplies a fact it already knows at the request boundary, and the
workspace owns the reviewed decision about what that fact means operationally.

## Keep The Boundary Clear

Operational switches fit rototo when the value should be reviewed, refreshed,
and explainable. They do not fit every kind of runtime decision.

Reach for this pattern when the runtime decision is:

- incident controls;
- temporary operational pauses;
- region or account-class policy;
- kill switches for risky workflows;
- reviewed behavior changes that should not require an app redeploy.

Keep these decisions somewhere else:

- per-request authorization;
- user preferences;
- counters and quotas that update on every request;
- queue state or workflow state;
- high-volume mutable data.

That is the split worth protecting. Rototo gives the app a reviewed operational
answer. The app still owns identity, permissions, state changes, and the domain
logic around the request.
