# JSON Output Reference

Most CLI commands support `--json`. JSON output is intended for tests, scripts,
CI, and agents.

## Workspace Inspect

```sh
rototo workspace inspect --workspace ./config --json
```

Shape:

```json
{
  "workspace": "/abs/path/config",
  "environments": ["dev", "stage", "prod"],
  "qualifiers": [
    {
      "id": "enterprise-accounts",
      "uri": "qualifier://enterprise-accounts",
      "path": "qualifiers/enterprise-accounts.toml"
    }
  ],
  "variables": [
    {
      "id": "llm-agent-config",
      "uri": "variable://llm-agent-config",
      "path": "variables/llm-agent-config.toml"
    }
  ]
}
```

## Lint

Workspace lint:

```json
{
  "workspace": "/abs/path/config",
  "diagnostics": []
}
```

Qualifier lint:

```json
{
  "workspace": "/abs/path/config",
  "id": "enterprise-accounts",
  "diagnostics": []
}
```

Variable lint:

```json
{
  "workspace": "/abs/path/config",
  "id": "llm-agent-config",
  "diagnostics": []
}
```

Diagnostics contain stable fields such as `code`, `source`, `message`, `help`,
and `rule` when a lint rule is associated with the diagnostic. See
`diagnostics`.

## List Commands

Qualifier list:

```json
{
  "workspace": "/abs/path/config",
  "qualifiers": [
    {
      "id": "enterprise-accounts",
      "uri": "qualifier://enterprise-accounts",
      "path": "qualifiers/enterprise-accounts.toml"
    }
  ]
}
```

Variable list:

```json
{
  "workspace": "/abs/path/config",
  "variables": [
    {
      "id": "llm-agent-config",
      "uri": "variable://llm-agent-config",
      "path": "variables/llm-agent-config.toml"
    }
  ]
}
```

## Get Commands

Qualifier get:

```json
{
  "workspace": "/abs/path/config",
  "id": "enterprise-accounts",
  "uri": "qualifier://enterprise-accounts",
  "path": "qualifiers/enterprise-accounts.toml",
  "value": {
    "schema_version": 1,
    "qualifier": {}
  }
}
```

Variable get:

```json
{
  "workspace": "/abs/path/config",
  "id": "llm-agent-config",
  "uri": "variable://llm-agent-config",
  "path": "variables/llm-agent-config.toml",
  "value": {
    "schema_version": 1,
    "variable": {}
  }
}
```

Variable get returns the expanded variable TOML after external value files have
been loaded.

## Qualifier Resolution

```sh
rototo qualifier resolve enterprise-accounts \
  --workspace ./config \
  --context @context.json \
  --json
```

Shape:

```json
{
  "workspace": "/abs/path/config",
  "id": "enterprise-accounts",
  "value": true
}
```

Resolve all qualifiers:

```json
{
  "workspace": "/abs/path/config",
  "values": [
    {
      "id": "enterprise-accounts",
      "value": true
    }
  ]
}
```

## Variable Resolution

```sh
rototo variable resolve llm-agent-config \
  --workspace ./config \
  --env prod \
  --context @context.json \
  --json
```

Shape:

```json
{
  "workspace": "/abs/path/config",
  "id": "llm-agent-config",
  "environment": "prod",
  "value_key": "enterprise",
  "value": {
    "model": "gpt-5",
    "gateway": "openai",
    "max_output_tokens": 5000,
    "temperature": 0.2
  }
}
```

Resolve all variables:

```json
{
  "workspace": "/abs/path/config",
  "values": [
    {
      "id": "llm-agent-config",
      "environment": "prod",
      "value_key": "enterprise",
      "value": {
        "model": "gpt-5"
      }
    }
  ]
}
```

## Diagnostic Catalog

Diagnostic list:

```json
{
  "scope": "global",
  "subject": "rototo",
  "diagnostics": []
}
```

Diagnostic get prints one catalog entry as JSON.

## Stability Notes

The documented fields are intended for automation. Consumers should ignore
unknown fields so future versions can add detail without breaking existing
tools.
