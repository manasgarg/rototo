# Bucketed Rollout

Some changes should not reach every account at once. A new search ranking mode
may be ready for production traffic, but I still want a narrow, stable rollout:
test accounts first, then a small percentage of real accounts, then a wider
range after the team has observed the behavior.

The important word is stable. The same account should get the same result on
every request while the rollout policy is unchanged. Rototo
[bucket predicates](reference-predicate-operators.html) give us that behavior
without putting random selection in app code.

We will model that as `rollout-config`, with one variable named
`search-ranking-mode`.

## Start With The Stable Mode

Create the workspace:

```sh
rototo init rollout-config --variable search-ranking-mode
```

Replace `rollout-config/variables/search-ranking-mode.toml`:

```toml
schema_version = 1

description = "Search ranking mode used for catalog queries"
type = "string"

[resolve]
default = "stable"
```

The app can support both modes, but the workspace still selects `stable` for
everyone. That is the starting point I want before adding rollout policy.

Lint and resolve the default:

```sh
rototo lint rollout-config
rototo resolve rollout-config --variable search-ranking-mode
```

```text
source: literal
value: "stable"
```

## Enable Test Accounts First

Before sending traffic to a percentage bucket, I want an explicit live test
path. Test accounts exercise the same SDK call and the same production
workspace source as regular accounts, but they do not change the regular
account experience.

Create [`rollout-config/qualifiers/test-accounts.toml`](reference-qualifiers.html):

```toml
schema_version = 1
description = "Accounts marked for live configuration testing"

[[predicate]]
attribute = "account.kind"
op = "eq"
value = "test"
```

Update `rollout-config/variables/search-ranking-mode.toml`:

```toml
schema_version = 1

description = "Search ranking mode used for catalog queries"
type = "string"

[resolve]
default = "stable"

[[resolve.rule]]
qualifier = "test-accounts"
value = "hybrid"
```

This is the first PR I would ship. The service can refresh the workspace, and
test accounts can use `hybrid` while every regular account stays on `stable`.

Generate the first [context schema](reference-context.html):

```sh
rototo init rollout-config --context
```

On this workspace, rototo writes `rollout-config/schemas/context.schema.json`:

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
        "kind": { "type": "string" }
      }
    }
  }
}
```

Lint the workspace:

```sh
rototo lint rollout-config
```

Resolve both paths:

```sh
rototo resolve rollout-config \
  --variable search-ranking-mode \
  --context account.kind=regular
```

```text
source: literal
```

```sh
rototo resolve rollout-config \
  --variable search-ranking-mode \
  --context account.kind=test
```

```text
source: literal
```

## Add A Stable Bucket

After the test-account path looks good, add a bucket for a small slice of real
accounts.

Create `rollout-config/qualifiers/hybrid-ranking-bucket.toml`:

```toml
schema_version = 1
description = "Stable five percent rollout bucket for hybrid ranking"

[[predicate]]
attribute = "account.id"
op = "bucket"
salt = "search-ranking-hybrid-2026-06"
range = [0, 500]
```

The bucket range is on a 0 to 10000 scale, so `[0, 500]` is five percent. The
salt names this rollout. Keep it stable while you widen the range; changing the
salt reshuffles account assignment. The exact operator rules are in
[Predicate Operators](reference-predicate-operators.html).

Now update the variable:

```toml
schema_version = 1

description = "Search ranking mode used for catalog queries"
type = "string"

[resolve]
default = "stable"

[[resolve.rule]]
qualifier = "test-accounts"
value = "hybrid"

[[resolve.rule]]
qualifier = "hybrid-ranking-bucket"
value = "hybrid"
```

[Rules are evaluated in order](reference-variable-resolution.html). Test
accounts stay first because they are an explicit operator-controlled path. The
bucket covers regular accounts after that.

## Regenerate The Context Contract

The new bucket qualifier introduced `account.id`. Regenerate the context schema
after that path exists. Since the file already exists from the test-account
phase, use `--force` and review the resulting diff:

```sh
rototo init rollout-config --context --force
```

On this workspace, the regenerated
`rollout-config/schemas/context.schema.json` includes both paths:

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
        "id": { "type": ["boolean", "number", "string"] },
        "kind": { "type": "string" }
      }
    }
  }
}
```

Lint the workspace:

```sh
rototo lint rollout-config
```

## Resolve Stable Assignments

The account ID is the bucket input. The same account ID and salt produce the
same bucket value every time.

`acct-0001` is outside this five percent range:

```sh
rototo resolve rollout-config \
  --variable search-ranking-mode \
  --context account.kind=regular \
  --context account.id=acct-0001
```

```text
test: bucket salt=search-ranking-hybrid-2026-06 range=[0,500] bucket=2978
source: literal
```

`acct-0005` is inside the range:

```sh
rototo resolve rollout-config \
  --variable search-ranking-mode \
  --context account.kind=regular \
  --context account.id=acct-0005
```

```text
test: bucket salt=search-ranking-hybrid-2026-06 range=[0,500] bucket=134
source: literal
```

The bucket value is deterministic, not sampled per request. That means logs,
support investigations, and app tests can
[explain why an account received](reference-resolution-output.html) `hybrid`.

## Widen Through Review

When the five percent rollout looks healthy, widen the same bucket by changing
the range:

```toml
[[predicate]]
attribute = "account.id"
op = "bucket"
salt = "search-ranking-hybrid-2026-06"
range = [0, 2000]
```

That moves the rollout to twenty percent without changing the salt. Existing
accounts that were already inside `[0, 500]` remain inside the wider range.

Run lint, review the diff, and merge through the same release path:

```sh
rototo lint rollout-config
```

Rollback is the reverse: move the range back to the previous value, or remove
the bucket rule while leaving the test-account path in place.

## Use The Mode In The App

The app should [resolve the mode](reference-sdk-resolution.html) near the code
path that needs it. Rototo selects the reviewed rollout policy; the app owns
both ranking implementations and the metrics that compare them.

:::sdk-snippet bucketed-rollout-app
```rust
use rototo::{ResolveContext, Workspace};

async fn search_ranking_mode(
    workspace: &Workspace,
    account_kind: &str,
    account_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let context = ResolveContext::from_json(serde_json::json!({
        "account": {
            "kind": account_kind,
            "id": account_id
        }
    }))?;

    let resolution = workspace
        .resolve_variable("search-ranking-mode", &context)
        .await?;
    let source = resolution.source.clone();
    let mode: String = serde_json::from_value(resolution.value)?;

    println!(
        "selected search-ranking-mode `{}` from {:?}",
        source,
        workspace.source_fingerprint()
    );

    Ok(mode)
}
```

```python
async def search_ranking_mode(
    workspace: rototo.Workspace,
    account_kind: str,
    account_id: str,
) -> str:
    context = {
        "account": {
            "kind": account_kind,
            "id": account_id,
        },
    }
    resolution = await workspace.resolve_variable("search-ranking-mode", context)
    mode = str(resolution.value)

    print(f"selected search-ranking-mode `{resolution.source}`")
    return mode
```

```typescript
async function searchRankingMode(
  workspace: Workspace,
  accountKind: string,
  accountId: string,
): Promise<string> {
  const resolution = await workspace.resolveVariable(
    "search-ranking-mode",
    { account: { kind: accountKind, id: accountId } },
  );
  const mode = String(resolution.value);

  console.log(`selected search-ranking-mode \`${resolution.source}\``);
  return mode;
}
```

```java
String searchRankingMode(
    Workspace workspace,
    String accountKind,
    String accountId
) throws Exception {
    VariableResolution resolution = workspace
        .resolveVariable(
            "search-ranking-mode",
            Map.of("account", Map.of(
                "kind", accountKind,
                "id", accountId
            ))
        )
        .get();

    System.out.printf("selected search-ranking-mode `%s`%n", resolution.source());
    return (String) resolution.value();
}
```

```go
func searchRankingMode(
    ctx context.Context,
    workspace *rototo.Workspace,
    accountKind string,
    accountID string,
) (string, error) {
    resolution, err := workspace.ResolveVariable(
        ctx,
        "search-ranking-mode",
        map[string]any{
            "account": map[string]any{
                "kind": accountKind,
                "id":   accountID,
            },
        },
        nil,
    )
    if err != nil {
        return "", err
    }

    mode, _ := resolution.Value.(string)
    fmt.Printf("selected search-ranking-mode `%s`\n", resolution.Source)
    return mode, nil
}
```
:::

Keep the implementation boundary clear. Rototo should not choose search
results, store account history, or own ranking metrics. It should answer which
reviewed mode applies for this request.

## Keep The Rollout Explainable

Use bucketed rollout when the assignment should be:

- deterministic for the same context value;
- controlled through review;
- observable in logs and traces;
- reversible without an app redeploy.

Avoid it when the decision belongs to another runtime system:

- per-request random sampling;
- model scoring;
- account records;
- metrics collection;
- high-volume mutable state.

The workspace owns the rollout policy. The app owns the behavior behind each
mode and the evidence that tells the team whether to widen, hold, or roll back.
