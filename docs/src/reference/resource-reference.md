# Resource Reference

Resources define structured value catalogs that multiple variables can select
from. They keep the selection logic in `variables/` small while letting resource
objects carry the full JSON Schema contract.

## Location and ID

Resource definitions live under `resources/`:

```text
resources/<resource-id>.toml
```

The file stem is the resource id. Resource objects live beside the definition:

```text
resources/<resource-id>-objects/<object-id>.toml
```

The object file stem is the object id. A variable with
`type = "resource:<resource-id>"` uses these object ids as its environment and
rule values.

## Resource Definition

Each resource declares its schema version and JSON Schema:

```toml
schema_version = 1
description = "LLM agent configuration"
schema = "../schemas/llm-config.schema.json"
```

The schema path is resolved relative to the resource file and must stay inside
the workspace. The schema file must parse and compile as JSON Schema.

## Resource Objects

Each object file is a TOML document validated against the resource schema:

```toml
model = "gpt-5"
gateway = "openai"
max_output_tokens = 5000
temperature = 0.2
```

The whole TOML document is the object. A top-level `value` key is just a field
named `value`; it is not unwrapped.

## Variables Selecting Resource Objects

Resource-backed variables point at a resource through `type`:

```toml
schema_version = 1
description = "LLM settings for the incident summary agent"
type = "resource:llm-agent-config"

[env._]
value = "standard"

[[env.prod.rule]]
qualifier = "enterprise-accounts"
value = "enterprise"
```

`standard` and `enterprise` must exist as files under
`resources/llm-agent-config-objects/`.

## Resource References in JSON Schema

A resource object can refer to another resource object by storing the referenced
object id in a string field. Express that contract in JSON Schema with
`x-rototo-resource`:

```json
{
  "type": "object",
  "required": ["limit_profile"],
  "properties": {
    "limit_profile": {
      "type": "string",
      "x-rototo-resource": "tenant-limit-profile"
    }
  }
}
```

During lint, rototo checks that the target resource exists and that each string
value points at a known object for that resource.

## Validation

Resource lint checks:

- `schema_version = 1` exists.
- `schema` points at a readable JSON Schema inside the workspace.
- each object file parses as TOML.
- each object matches the resource schema.
- each `x-rototo-resource` value points at a known resource object.
