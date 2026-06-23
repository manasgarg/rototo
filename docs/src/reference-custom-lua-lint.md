# Custom Lua Lint Reference

[Built-in lint](reference-lint-overview.html) protects rototo's workspace
contract: files parse, references resolve, values match their declared types or
catalog schemas, and request context schemas line up with the qualifiers that
read them. Custom Lua lint protects local policy: the constraints that belong
to a team or application and still need to fail review before a workspace is
released.

Custom lint files live under `lint/*.lua`. Each file defines `register(lint)`.
Registration declares one or more rules. Each rule selects a target address in
the semantic workspace model. Rototo expands that address, invokes the handler
once for each selected target, and attaches returned diagnostics to that target.

## Minimal File

```lua
function register(lint)
  lint:rule({
    id = "operations/max-token-budget",
    title = "Token budget is outside policy",
    help = "Keep token budgets within the approved range.",
    severity = "warning",
    target = "/variables/agent-config",
    handler = "check_agent_config",
  })
end

function check_agent_config(workspace, variable)
  local max_tokens = variable.resolve.default.max_tokens

  if type(max_tokens) == "number" and max_tokens > 4096 then
    return {
      {
        message = "max_tokens exceeds the approved limit",
        path = "/resolve/default/max_tokens",
      }
    }
  end

  return {}
end
```

The rule runs once because `/variables/agent-config` addresses one variable.
The handler receives the whole semantic `workspace` plus the selected
`variable`. When a diagnostic omits `path`, rototo attaches it to the whole
target.

## Registration

Use `lint:rule` inside `register(lint)`:

```lua
function register(lint)
  lint:rule({
    id = "authority/rule-id",
    title = "Short title",
    help = "Actionable guidance for fixing the problem.",
    severity = "error",
    target = "/",
    handler = "handler_name",
  })
end
```

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | Yes | Custom rule id in `<authority>/<rule-id>` form. The authority must not be `rototo`. |
| `title` | Yes | Short title shown in diagnostic catalogs and editor tooling. |
| `help` | Yes | Guidance shown with the diagnostic. |
| `severity` | No | `error` or `warning`. Defaults to `error`. |
| `target` | No | Semantic address that selects handler targets. Defaults to `/`. |
| `handler` | Yes | Name of a callable Lua function in the same file. |

A file may register more than one rule. If the same custom rule id appears in
more than one file, the metadata must match exactly: `title`, `help`, and
`severity` are part of the rule contract.

Custom lint runs after rototo has built the semantic workspace and reference
model. Registration does not choose a lint stage; custom rules run in the
policy stage.

## Target Addresses

Targets use a REST-style address grammar over the semantic workspace model:

| Address | Handler invocations |
| --- | --- |
| `/` | Once for the workspace. |
| `/qualifiers` | Once for each qualifier. |
| `/qualifiers/<id>` | Once for that qualifier. |
| `/variables` | Once for each variable. |
| `/variables/<id>` | Once for that variable. |
| `/variables/<id>/values` | Once for each inline value in that variable. |
| `/variables/<id>/values/<key>` | Once for that inline value. |
| `/variables/<id>/rules` | Once for each resolve rule in that variable. |
| `/variables/<id>/rules/<index>` | Once for that resolve rule. |
| `/catalogs` | Once for each catalog. |
| `/catalogs/<id>` | Once for that catalog. |
| `/catalogs/<id>/entries` | Once for each catalog entry. |
| `/catalogs/<id>/entries/<key>` | Once for that catalog entry. |
| `/request-contexts` | Once for each request context. |
| `/request-contexts/<id>` | Once for that request context. |
| `/request-contexts/<id>/entries` | Once for each stored sample. |
| `/request-contexts/<id>/entries/<key>` | Once for that stored sample. |

Collection addresses select members. For example, `/qualifiers` does not pass a
qualifier collection to the handler; it invokes the handler once for each
qualifier. Use `/` and inspect `workspace.qualifiers` when a rule needs an
aggregate check over the collection.

Rule indexes are zero-based because they match diagnostic entity indexes. Lua
arrays are still traversed with `ipairs`; the `index` field on each rule
carries the zero-based rototo index.

## Handler Input

Each handler receives two arguments:

```lua
function handler(workspace, target)
  return {}
end
```

`workspace` is a read-only semantic projection. It is not a TOML parse tree and
it is not rototo's internal Rust index. It is the stable data model available to
custom lint authors.

`target` is the item selected by the rule address. For `/`, `target` is the
workspace object. For collection and entity addresses, `target` is the current
qualifier, variable, value, rule, catalog, catalog entry, request context, or
request context entry.

Top-level shape:

```lua
workspace = {
  version = 1,
  root = "/abs/workspace/path",

  manifest = manifest,

  qualifiers = {
    ["premium-users"] = qualifier,
  },

  variables = {
    ["checkout-redesign"] = variable,
  },

  catalogs = {
    ["checkout-redesign"] = catalog,
  },

  request_contexts = {
    request = request_context,
  },
}
```

Top-level collections are keyed by stable ids. That means direct lookup is:

```lua
local qualifier = workspace.qualifiers["premium-users"]
local variable = workspace.variables["checkout-redesign"]
local catalog = workspace.catalogs["checkout-redesign"]
```

Do not use `#workspace.qualifiers` or `ipairs(workspace.qualifiers)` for
top-level collections. They are maps, not arrays. Ordered child collections
such as `variable.resolve.rules` are arrays.

## Semantic Entities

A semantic entity is an object in the projected workspace that rototo can select
with a target address and attach diagnostics to. The custom lint target model
exposes these entities:

| Entity `kind` | Selected by | Stable identity |
| --- | --- | --- |
| `workspace` | `/` | The workspace root target. |
| `qualifier` | `/qualifiers`, `/qualifiers/<id>` | `id` |
| `variable` | `/variables`, `/variables/<id>` | `id` |
| `value` | `/variables/<id>/values`, `/variables/<id>/values/<key>` | `variable`, `key` |
| `rule` | `/variables/<id>/rules`, `/variables/<id>/rules/<index>` | `variable`, zero-based `index` |
| `catalog` | `/catalogs`, `/catalogs/<id>` | `id` |
| `catalog_entry` | `/catalogs/<id>/entries`, `/catalogs/<id>/entries/<key>` | `catalog`, `key` |
| `request_context` | `/request-contexts`, `/request-contexts/<id>` | `id` |
| `request_context_entry` | `/request-contexts/<id>/entries`, `/request-contexts/<id>/entries/<key>` | `request_context`, `key` |

Those identities are the durable way to reason about an entity. Custom linters
do not receive source locations or line ranges. Rototo keeps those internally so
it can map semantic diagnostics back to files.

Built-in rototo diagnostics also use entities for the manifest and custom lint
files. They are not separately addressable custom lint targets. A policy about
workspace manifest data should target `/` and return a JSON Pointer such as
`/manifest/extends`.

## Source Paths

File-backed entities expose a workspace-relative `path` string. Use that only
when source layout is part of the policy. For ordinary value, schema, reference,
or naming checks, prefer ids, fields, and diagnostic JSON Pointers.

In practice, most custom lint rules should not branch on source paths. Rototo's
workspace layout is already part of the built-in contract. The path is mainly
provenance for source-aware tools and agents, and a narrow escape hatch for a
local convention that is genuinely about repository layout. The diagnostic
still returns no source coordinates; it attaches to the current semantic target,
and rototo maps that target to the source span internally.

Path fields are available on `workspace.manifest`, `qualifier`, `variable`,
`catalog`, `catalog_entry`, `request_context`, and `request_context_entry`.
Nested entities such as inline values and resolve rules do not have their own
path field; use their parent id to look up the containing variable when a
source-layout rule needs the file path.

## Workspace

```lua
workspace = {
  kind = "workspace",
  root = "/abs/workspace/path",
  manifest = manifest,
  extends = {},
}
```

The root target `/` passes this workspace object as `target`.

## Qualifiers

```lua
qualifier = {
  kind = "qualifier",
  id = "premium-users",
  path = "qualifiers/premium-users.toml",
  description = "Premium users",
  when = "context.user.tier == \"premium\"",
}
```

The qualifier target exposes the expression as `when`. Custom rules that
need expression-specific checks should target the qualifier and return
diagnostics with `path = "/when"`.

## Variables, Values, And Rules

```lua
variable = {
  kind = "variable",
  id = "checkout-redesign",
  path = "variables/checkout-redesign.toml",
  description = "Checkout redesign config",

  declaration = {
    kind = "catalog",
    value = "checkout-redesign",
  },

  values = {
    premium = value,
  },

  resolve = {
    kind = "resolve",
    default = "control",
    rules = {
      {
        kind = "rule",
        variable = "checkout-redesign",
        index = 0,
        when = 'qualifier["premium-users"]',
        value = "premium",
      },
    },
  },
}
```

`declaration.kind` is one of:

```text
primitive
catalog
schema
missing
conflict
invalid
```

`schema` is retained only to describe legacy invalid variable declarations.
Standalone workspace `schemas/` files are not part of the active model.

Inline values are exposed under `variable.values`:

```lua
value = {
  kind = "value",
  variable = "checkout-redesign",
  key = "premium",
  value = {
    variant = "premium",
    heading = "Priority checkout",
  },
}
```

For catalog-backed variables, resolve values name catalog entries. The catalog
entry itself is exposed under the catalog.

## Catalogs

```lua
catalog = {
  kind = "catalog",
  id = "checkout-redesign",
  path = "catalogs/checkout-redesign.schema.json",
  json = {
    type = "object",
    properties = {},
  },
  entries = {
    premium = catalog_entry,
  },
}
```

Catalog entries expose the whole TOML entry file converted to JSON:

```lua
catalog_entry = {
  kind = "catalog_entry",
  catalog = "checkout-redesign",
  key = "premium",
  path = "catalogs/checkout-redesign-entries/premium.toml",
  value = {
    variant = "premium",
    heading = "Priority checkout",
  },
}
```

## Request Contexts

```lua
request_context = {
  kind = "request_context",
  id = "request",
  path = "request-contexts/request.schema.json",
  json = {
    type = "object",
    properties = {},
  },
  entries = {
    premium = request_context_entry,
  },
}
```

Stored request context samples are exposed as entries:

```lua
request_context_entry = {
  kind = "request_context_entry",
  request_context = "request",
  key = "premium",
  path = "request-contexts/request-entries/premium.json",
  value = {
    user = { tier = "premium" },
  },
}
```

## Diagnostic Return

Handlers return a list of diagnostics:

```lua
return {
  {
    message = "max_tokens exceeds the approved limit",
    path = "/resolve/default/max_tokens",
  }
}
```

`message` is required. `path` is optional and uses JSON Pointer syntax:

- if `path` is omitted, the diagnostic attaches to the whole current target;
- if `path = ""`, the diagnostic also attaches to the whole current target;
- otherwise `path` must begin with `/`, and each token walks through the
  exposed target object;
- `/` inside a key is escaped as `~1`, and `~` is escaped as `~0`.

Returning `nil` or an empty list means the rule passed.

The pointer is relative to the handler target, not to the whole workspace unless
the rule itself targets `/`. A rule registered for `/variables` receives one
variable at a time, so `path = "/resolve/default"` means the current variable's
resolve default. A rule registered for `/` can point into the workspace map with
`path = "/variables/checkout-redesign/resolve/default"`.

Common pointers include:

| Handler target | Pointer |
| --- | --- |
| workspace | `/manifest/extends` |
| workspace | `/variables/<id>/resolve/default` |
| workspace | `/qualifiers/<id>/when` |
| qualifier | `/description`, `/when` |
| variable | `/description`, `/declaration/value`, `/resolve`, `/resolve/default` |
| variable | `/values/<key>/value/max_tokens`, `/resolve/rules/0/value` |
| value | `/value`, `/value/max_tokens` |
| rule | `/qualifier`, `/value` |
| catalog | `/json`, `/json/properties/variant`, `/entries/<key>/value/owner` |
| catalog_entry | `/value`, `/value/owner` |
| request_context | `/json`, `/json/properties/user`, `/entries/<key>/value/user/tier` |
| request_context_entry | `/value`, `/value/user/tier` |

Rototo maps known semantic nodes to source spans. If a pointer is valid JSON
Pointer syntax but reaches deeper than rototo can pinpoint, rototo reports
against the nearest mappable ancestor. If a pointer is malformed or does not
match the target model, rototo falls back to the current handler target.

## Complete Example

This rule rejects catalog entries that opt into a feature flag without declaring
an owner.

```lua
function register(lint)
  lint:rule({
    id = "operations/owned-enabled-feature",
    title = "Enabled features must declare an owner",
    help = "Add owner to each enabled catalog value so incident review has a contact.",
    target = "/catalogs/features/entries",
    handler = "check_enabled_feature_owner",
  })
end

function check_enabled_feature_owner(workspace, entry)
  if entry.value.enabled == true and entry.value.owner == nil then
    return {
      {
        message = "enabled catalog value is missing owner",
        path = "/value/owner",
      }
    }
  end

  return {}
end
```

## Runtime Limits

Custom lint runs in a restricted Lua VM:

- memory limit: 16 MiB;
- instruction limit: 1,000,000;
- task timeout: 2 seconds;
- disabled globals include `os`, `io`, `package`, `require`, `dofile`,
  `loadfile`, `load`, `collectgarbage`, and `debug`.

Available standard libraries are table, string, UTF-8, and math.

Custom lint should be deterministic and local to the workspace data it
receives. It should not depend on wall-clock time, network calls, subprocesses,
or machine-local files outside the workspace.
