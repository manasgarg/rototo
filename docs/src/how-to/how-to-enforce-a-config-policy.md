# How to Enforce a Config Policy

Use this when the workspace is structurally valid, but your team has a policy
that schemas cannot express clearly.

Examples include token ceilings, naming conventions, and
environment-specific safety limits. rototo supports workspace-scoped custom Lua
lint, so policy can run before the workspace is published.

## Expected outcome

After this change:

- The workspace manifest declares custom lint rule metadata.
- A Lua script registers a handler and returns diagnostics when values violate policy.
- `rototo lint` fails on policy violations.
- Reviewers see a clear message and recovery guidance.

## Decide what belongs in custom lint

Use schemas for resource object shape:

```text
this object must have model, prompt, and max_output_tokens
```

Use custom lint for workspace policy:

```text
max-output-tokens must be <= 5000
variable descriptions must include an owner
production limits must stay below a service threshold
```

Keeping this distinction matters. Schemas define the application contract;
custom lint defines local rules that rototo cannot infer.

## Declare the rule

In `rototo-workspace.toml`, declare the diagnostic rule:

```toml
schema_version = 1

[environments]
values = ["prod"]

[[lint.rule]]
id = "platform/max-output-token-budget"
title = "Output token budget is too high"
help = "Use 5000 or fewer output tokens."
```

The rule id uses `<authority>/<rule-id>`; `rototo` is reserved for built-in
diagnostics.

## Write the policy

Create `lint/max-output-tokens.lua`:

```lua
function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value",
    rule = "platform/max-output-token-budget",
    handler = "check_token_budget",
  })
end

function check_token_budget(ctx)
  if ctx.target.value > 5000 then
    return {
      {
        message = "value " .. ctx.target.name .. " exceeds the token budget"
      }
    }
  end

  return {}
end
```

The registration runs the handler once for each primitive inline value. Handlers
return a list of diagnostics with `message`; the registration supplies the rule
id. A diagnostic can include `field` when it should point at a narrower field
inside the registered target. Return an empty list when the policy passes.

## Verify the policy

Run lint:

```sh
rototo lint config/
```

If a value violates the policy, lint fails with a custom diagnostic. Inspect
the workspace diagnostic catalog when you need the stable rule or JSON shape:

```sh
rototo show config/ --lint-rule platform/max-output-token-budget
```

## Common mistakes

Do not use custom lint for type checking that a schema or primitive type can
already enforce.

Do not write vague diagnostics. The message should identify the offending value;
the declared rule help should say how to fix it.

Custom lint files are discovered from `lint/*.lua`; keep policy routing in
`register(lint)`.

## Related docs

- `variable-reference` explains how variables are targeted by custom lint.
- `diagnostic-reference` explains custom lint diagnostics.
- `value-types-reference` explains value validation.
