# Custom Lua Lint Reference

Built-in lint protects rototo's workspace contract. Custom Lua lint protects
local policy: the constraints that matter in your domain and are worth
checking before a workspace is released.

Custom lint files live under `lint/*.lua`.

## Minimal File

```lua
function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value.projects",
    rule = {
      id = "operations/account-project-limit",
      title = "Account project limit is outside policy",
      help = "Keep configured project limits between 1 and 100.",
      severity = "warning",
    },
    handler = "check_projects",
  })
end

function check_projects(ctx)
  local value = ctx.target.selected
  if type(value) == "number" and value > 100 then
    return {
      { message = "project limit is above the operations policy" }
    }
  end
  return {}
end
```

The file must define `register(lint)`. Registration happens before custom
handlers run.

## Registration

`lint:on` accepts a registration table:

| Field | Required | Meaning |
| --- | --- | --- |
| `stage` | Yes | `project`, `reference`, `value`, `graph`, or `policy`. |
| `entity` | Yes | `workspace`, `qualifier`, `variable`, `value`, or `schema`. |
| `field` | No | Narrower field selector for the target. |
| `rule` | Yes | Rule metadata table. |
| `handler` | Yes | Name of a callable Lua function in the same file. |

Rule metadata:

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | Yes | Custom rule id in `<authority>/<rule-id>` form. |
| `title` | Yes | Short title used in catalogs. |
| `help` | Yes | Guidance shown with diagnostics. |
| `severity` | No | `error` or `warning`; defaults to `error`. |

If the same custom rule id is registered more than once, title, help, and
severity must match.

## Field Selectors

Supported selectors:

| Entity | Fields |
| --- | --- |
| `workspace` | `extends` |
| `qualifier` | `id`, `description`, `predicates` |
| `variable` | `id`, `description`, `type`, `schema`, `values`, `resolve` |
| `value` | `key`, `value`, `value.<json.path>` |
| `schema` | `json`, `json.<json.path>` |

For `value.<json.path>` and `json.<json.path>`, path components must use
lowercase ASCII letters, uppercase ASCII letters, digits, `_`, or `-`.

## Handler Context

Handlers receive one argument:

```lua
function handler(ctx)
  -- ctx.stage
  -- ctx.entity
  -- ctx.target
end
```

`ctx.target` depends on the registered entity.

For `value` targets:

```json
{
  "kind": "value",
  "name": "enterprise",
  "value": {},
  "origin": { "kind": "inline", "doc": 4 },
  "selected": {},
  "variable": {
    "id": "account-limits",
    "uri": "variable://account-limits",
    "path": "variables/account-limits.toml"
  }
}
```

`selected` is the narrowed field when a `value.<json.path>` selector is used.

For `schema` targets, `selected` is the narrowed JSON Schema field when a
`json.<json.path>` selector is used.

## Handler Return

Handlers return a list of diagnostics:

```lua
return {
  {
    message = "configured value violates local policy",
    field = "value.projects",
  }
}
```

`message` is required. `field` is optional and can narrow the diagnostic
location within the registered target.

Returning `nil` or an empty list means no diagnostics.

## Lua Runtime Limits

Custom lint runs in a restricted Lua VM:

- memory limit: 16 MiB;
- instruction limit: 1,000,000;
- task timeout: 2 seconds;
- disabled globals include `os`, `io`, `package`, `require`, `dofile`,
  `loadfile`, `load`, `collectgarbage`, and `debug`.

Available standard libraries are table, string, UTF-8, and math.

Custom lint should be deterministic and local to the workspace data it
receives.
