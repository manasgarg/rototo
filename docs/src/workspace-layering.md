# Workspace Layering

Enterprise configuration usually has more than one owner. The product team may
own the policy contract. A customer administrator may own the customer-wide
defaults. A team administrator may own a narrow local preference.

Putting all of that in one workspace forces the wrong tradeoff. Either every
owner can edit too much, or the product has to fork configuration for every
customer. [Workspace layering](reference-workspace-layering.html) gives us a
better boundary: each owner edits the workspace they are responsible for, while
the final workspace inherits the contracts and values below it.

I use inference provider routing here because the ownership split is concrete:

- the product team defines the policy contract and supported providers;
- the customer administrator chooses the customer-wide fallback posture;
- the team administrator tries a narrower routing policy for summarization;
- the app loads the team workspace and resolves the final policy.

## Start With Ownership

Layering is not a substitute for authorization. Git permissions, review rules,
CI, and deployment policy still decide who may edit each repository. Rototo
does the configuration work after those controls have done theirs: it projects
the layers into one workspace, lints the result, and resolves variables from
that final workspace.

The layers look like this:

```text
product-config/
  rototo-workspace.toml
  schemas/
  catalogs/
  variables/

customer-config/
  rototo-workspace.toml  # extends product-config
  catalogs/
  variables/

team-config/
  rototo-workspace.toml  # extends customer-config
  qualifiers/
  catalogs/
  variables/
  schemas/
```

The application should load the most specific
[workspace source](reference-workspace-sources.html) it is allowed to use. In
this example, that is `team-config`. Rototo follows the `extends` chain and
builds the inherited workspace before lint and resolution.

## Product Owns The Contract

The product layer owns the policy schema, the
[catalog declaration](reference-catalogs.html), and the product default.
Create `product-config/rototo-workspace.toml`:

```toml
schema_version = 1
```

Create `product-config/variables/inference-routing-policy.toml`:

```toml
schema_version = 1

description = "Inference provider routing policy"
type = "catalog:inference-routing-policy"

[resolve]
default = "product_default"
```

Create `product-config/catalogs/inference-routing-policy.toml`:

```toml
schema_version = 1

description = "Inference routing policy entries"
schema = "../schemas/inference-routing-policy.schema.json"
```

Create `product-config/schemas/inference-routing-policy.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["mode", "primary_provider", "fallback_provider", "allowed_tasks", "timeout_ms"],
  "properties": {
    "mode": { "type": "string", "enum": ["primary", "fallback", "hold"] },
    "primary_provider": { "type": "string", "enum": ["openai", "anthropic", "none"] },
    "fallback_provider": { "type": "string", "enum": ["openai", "anthropic", "none"] },
    "allowed_tasks": {
      "type": "array",
      "items": { "type": "string", "enum": ["summarization", "classification", "extraction"] },
      "minItems": 1,
      "uniqueItems": true
    },
    "timeout_ms": { "type": "integer", "minimum": 500, "maximum": 10000 }
  },
  "additionalProperties": false
}
```

This schema is the product team's guardrail. Customer and team layers can add
their own policy entries, but those entries still have to use supported
providers, supported task names, and a timeout range the product is prepared to
operate.

Create
`product-config/catalogs/inference-routing-policy-entries/product_default.toml`:

```toml
mode = "primary"
primary_provider = "openai"
fallback_provider = "none"
allowed_tasks = ["summarization", "classification"]
timeout_ms = 4000
```

Lint and resolve the product layer:

```sh
rototo lint product-config
rototo resolve product-config --variable inference-routing-policy
```

```text
value key: product_default
value: {"allowed_tasks":["summarization","classification"],"fallback_provider":"none","mode":"primary","primary_provider":"openai","timeout_ms":4000}
```

At this point the product team has published a valid base policy. No customer
or team-specific rule exists yet.

## Customer Owns The Default

Now create a customer workspace that
[extends](reference-workspace-manifest.html) the product layer:

```toml
# customer-config/rototo-workspace.toml
schema_version = 1
extends = ["../product-config"]
```

The customer administrator wants a fallback provider for the whole account.
They do not need to copy the product schema or catalog declaration. They add a
new policy entry and override the variable default.

Create
`customer-config/catalogs/inference-routing-policy-entries/customer_default.toml`:

```toml
mode = "fallback"
primary_provider = "openai"
fallback_provider = "anthropic"
allowed_tasks = ["summarization", "classification"]
timeout_ms = 5000
```

Create `customer-config/variables/inference-routing-policy.toml`:

```toml
schema_version = 1

description = "Customer-owned inference provider routing policy"
type = "catalog:inference-routing-policy"

[resolve]
default = "customer_default"
```

This file replaces the inherited variable file. That is an important rule of
thumb for [layered workspaces](reference-workspace-layering.html): when a later
layer writes the same path, it owns the whole file at that path. Reviewers
should read that as an ownership change, not as a tiny patch to a hidden parent
file.

Lint and resolve the customer layer:

```sh
rototo lint customer-config
rototo resolve customer-config --variable inference-routing-policy
```

```text
value key: customer_default
value: {"allowed_tasks":["summarization","classification"],"fallback_provider":"anthropic","mode":"fallback","primary_provider":"openai","timeout_ms":5000}
```

The selected value changed, but the policy entry still passed the product
schema inherited from the base layer.

## Team Owns A Narrow Rule

Now create a team workspace that extends the customer layer:

```toml
# team-config/rototo-workspace.toml
schema_version = 1
extends = ["../customer-config"]
```

The team wants to try a faster route only for summarization. They can add a
team policy entry:

```toml
# team-config/catalogs/inference-routing-policy-entries/team_fast_summarization.toml
mode = "primary"
primary_provider = "anthropic"
fallback_provider = "openai"
allowed_tasks = ["summarization"]
timeout_ms = 2500
```

Then they name the [runtime condition](reference-qualifiers.html):

```toml
# team-config/qualifiers/summarization-trial.toml
schema_version = 1
description = "Team summarization requests routed through the trial policy"

[[predicate]]
attribute = "task.kind"
op = "eq"
value = "summarization"
```

Because the qualifier introduced `task.kind`, the team workspace needs the
[context contract](reference-context.html) for that fact:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": true,
  "properties": {
    "task": {
      "type": "object",
      "additionalProperties": true,
      "properties": {
        "kind": { "type": "string" }
      }
    }
  }
}
```

Finally, the team overrides the variable file. Notice that it keeps the
customer default while adding the team rule:

```toml
# team-config/variables/inference-routing-policy.toml
schema_version = 1

description = "Team-owned inference provider routing policy"
type = "catalog:inference-routing-policy"

[resolve]
default = "customer_default"

[[resolve.rule]]
qualifier = "summarization-trial"
value = "team_fast_summarization"
```

Lint the team workspace:

```sh
rototo lint team-config
```

Classification keeps the customer default:

```sh
rototo resolve team-config \
  --variable inference-routing-policy \
  --context task.kind=classification
```

```text
value key: customer_default
value: {"allowed_tasks":["summarization","classification"],"fallback_provider":"anthropic","mode":"fallback","primary_provider":"openai","timeout_ms":5000}
```

Summarization gets the team rule:

```sh
rototo resolve team-config \
  --variable inference-routing-policy \
  --context task.kind=summarization
```

```text
value key: team_fast_summarization
value: {"allowed_tasks":["summarization"],"fallback_provider":"openai","mode":"primary","primary_provider":"anthropic","timeout_ms":2500}
```

That is the ownership model in action. Product owns the contract. Customer owns
the account-wide default. Team owns a narrow condition. The app resolves one
variable from the final workspace.

## What The App Loads

The app should [load the most specific workspace source](reference-sdk-loading.html):

:::sdk-snippet workspace-layering-app
```rust
use rototo::{ResolveContext, Workspace};

async fn route_for_task(
    workspace_source: &str,
    task_kind: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let workspace = Workspace::load(workspace_source).await?;
    let context = ResolveContext::from_json(serde_json::json!({
        "task": {
            "kind": task_kind
        }
    }))?;

    let resolution = workspace
        .resolve_variable("inference-routing-policy", &context)
        .await?;

    Ok(resolution.value)
}
```

```python
async def route_for_task(
    workspace_source: str,
    task_kind: str,
) -> object:
    workspace = await rototo.Workspace.load(workspace_source)
    resolution = await workspace.resolve_variable(
        "inference-routing-policy",
        {"task": {"kind": task_kind}},
    )
    return resolution.value
```

```typescript
async function routeForTask(
  workspaceSource: string,
  taskKind: string,
): Promise<unknown> {
  const workspace = await Workspace.load(workspaceSource);
  const resolution = await workspace.resolveVariable(
    "inference-routing-policy",
    { task: { kind: taskKind } },
  );
  return resolution.value;
}
```

```java
Object routeForTask(String workspaceSource, String taskKind) throws Exception {
    try (Workspace workspace = Workspace.load(workspaceSource).get()) {
        VariableResolution resolution = workspace
            .resolveVariable(
                "inference-routing-policy",
                Map.of("task", Map.of("kind", taskKind))
            )
            .get();
        return resolution.value();
    }
}
```

```go
func routeForTask(
    ctx context.Context,
    workspaceSource string,
    taskKind string,
) (any, error) {
    workspace, err := rototo.Load(ctx, workspaceSource, nil)
    if err != nil {
        return nil, err
    }
    defer workspace.Close()

    resolution, err := workspace.ResolveVariable(
        ctx,
        "inference-routing-policy",
        map[string]any{"task": map[string]any{"kind": taskKind}},
        nil,
    )
    if err != nil {
        return nil, err
    }
    return resolution.Value, nil
}
```
:::

For this example, `workspace_source` would point at `team-config`. In a hosted
setup, it could be a git source for the team workspace. That source can extend
the customer workspace, which can extend the product workspace. Rototo loads
the graph, projects the inherited files, lints the projected workspace, and
then resolves from that result.

[Refresh](reference-sdk-refresh.html) follows the same model. If the product
schema or customer default changes, a long-running service that refreshes the
team workspace can pick up the new projected workspace after a successful
refresh. If refresh fails, the last successfully loaded workspace stays active.

## Keep The Boundaries Honest

Use workspace layering when separate owners need to share one configuration
model:

- product defaults with customer-specific policy;
- customer-wide settings with team-level preferences;
- regional policy layered over global product policy;
- private deployment values layered over a public base;
- temporary incident policy layered over a normal operating workspace.

Do not use layering to hide authorization problems. Rototo will merge and lint
the workspace graph, but it does not decide who is allowed to edit each layer.
That belongs in repository permissions, review policy, CI, and the deployment
path.

The practical question for every layer is: what does this owner have the right
to change? If the answer is clear, layering gives that owner a workspace
boundary. If the answer is unclear, fix the ownership model before adding more
layers.
