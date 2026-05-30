# How to Enforce a Config Policy

Use this when the workspace is structurally valid, but your team has a policy
that schemas cannot express clearly.

Examples include token ceilings, naming conventions, required prompt metadata,
allowed model families, or environment-specific safety limits. rototo supports
custom Lua lint on variables, so policy can run before the workspace is
published.

## Expected outcome

After this change:

- The variable declares a custom lint script.
- The script returns diagnostics when values violate policy.
- `rototo workspace lint` fails on policy violations.
- Reviewers see a clear message and recovery guidance.

## Decide what belongs in custom lint

Use schemas for shape:

```text
this object must have model, prompt, and max_output_tokens
```

Use custom lint for policy:

```text
max_output_tokens must be <= 5000
production models must use the approved model prefix
enterprise prompts must include escalation guidance
```

Keeping this distinction matters. Schemas define the application contract;
custom lint defines local rules that rototo cannot infer.

## Attach lint to the variable

In the variable file, add `[variable.lint]`:

```toml
schema_version = 1

[variable]
description = "LLM settings for the incident summary agent"
schema = "../schemas/llm-config.schema.json"

[variable.lint]
path = "../lint/llm-agent-config.lua"
```

The path is resolved relative to the variable file.

## Write the policy

Create `lint/llm-agent-config.lua`:

```lua
function lint_value(value)
  local config = value.value

  if config.max_output_tokens > 5000 then
    return {
      {
        message = "value " .. value.name .. " exceeds the token budget",
        help = "Use 5000 or fewer output tokens."
      }
    }
  end

  return {}
end
```

`lint_value(value)` runs once for each expanded value, including values loaded
from external value files.

Use `lint(variable)` when the policy needs to inspect the variable as a whole:

```lua
function lint(variable)
  return {}
end
```

Each function returns a list of diagnostics. Return an empty list when the
policy passes.

## Verify the policy

Run lint:

```sh
rototo workspace lint config/
```

If a value violates the policy, lint fails with a custom diagnostic. Inspect
the diagnostic catalog when you need the stable code or JSON shape:

```sh
rototo diagnostics get rototo/variable-custom-lint-failed
```

## Common mistakes

Do not use custom lint for type checking that a schema or primitive type can
already enforce.

Do not write vague diagnostics. The message should identify the offending value
and the help text should say how to fix it.

Do not assume custom lint is a separate workspace-level extension point today.
Attach the policy to the variables whose values it governs.

## Related docs

- `variable-reference` specifies `[variable.lint]`.
- `diagnostics` explains custom lint diagnostics.
- `value-types-reference` explains value validation.
