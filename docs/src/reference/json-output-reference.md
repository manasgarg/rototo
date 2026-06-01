# JSON Output Reference

Most CLI commands support `--json`. JSON output is intended for tests, scripts,
CI, and agents.

## Workspace Inspect

```sh
rototo inspect ./config --json
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
  "resources": [
    {
      "id": "llm-agent-config",
      "uri": "resource://llm-agent-config",
      "path": "resources/llm-agent-config.toml"
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
  "documents": [
    {
      "id": 0,
      "path": "rototo-workspace.toml",
      "uri": "file:///abs/path/config/rototo-workspace.toml",
      "version": null,
      "kind": "manifest"
    }
  ],
  "diagnostics": []
}
```

Targeted lint uses the same envelope after selectors filter diagnostics:

```json
{
  "workspace": "/abs/path/config",
  "documents": [],
  "diagnostics": []
}
```

Lint output includes every document considered by lint, including documents
with no diagnostics. Diagnostics contain `rule`, `severity`, `stage`, `entity`,
`message`, `help`, `location`, and `related`. The `rule` field is the stable
identity for automation. `location` contains the workspace-relative `path` and
a zero-based line/character `range` when rototo can attach the diagnostic to a
span. See `diagnostic-reference`.

## Show Commands

`show` uses selectors to return variables, resources, qualifiers, lint rules,
authorities, and linters in one envelope:

```json
{
  "command": "show",
  "workspace": "/abs/path/config",
  "variables": [],
  "resources": [],
  "qualifiers": [],
  "lint_rules": [],
  "lint_authorities": [],
  "linters": []
}
```

Selected variable, resource, and qualifier entries include authored TOML when a
specific id is selected:

```json
{
  "command": "show",
  "workspace": "/abs/path/config",
  "variables": [
    {
      "id": "llm-agent-config",
      "uri": "variable://llm-agent-config",
      "path": "variables/llm-agent-config.toml",
      "value": {
        "schema_version": 1
      }
    }
  ],
  "resources": [],
  "qualifiers": [],
  "lint_rules": [],
  "lint_authorities": [],
  "linters": []
}
```

Resource show returns the resource definition with an `objects` table containing
the parsed object files.

## Qualifier Resolution

```sh
rototo resolve ./config --qualifier enterprise-accounts \
  --context @context.json \
  --json
```

Shape:

```json
{
  "workspace": "/abs/path/config",
  "variables": [],
  "qualifiers": [
    {
      "id": "enterprise-accounts",
      "value": true
    }
  ]
}
```

## Variable Resolution

```sh
rototo resolve ./config --variable llm-agent-config \
  --env prod \
  --context @context.json \
  --json
```

Shape:

```json
{
  "workspace": "/abs/path/config",
  "variables": [
    {
      "id": "llm-agent-config",
      "environment": "prod",
      "value_key": "enterprise",
      "value": {
        "model": "gpt-5"
      }
    }
  ],
  "qualifiers": []
}
```

## Diagnostic Catalog

Diagnostic list:

```json
{
  "scope": "global",
  "subject": "global",
  "diagnostics": [
    {
      "rule": "rototo/variable-unknown-type",
      "severity": "error",
      "entity": "variable",
      "title": "Variable type is unknown",
      "help": "Use one of bool, int, number, string, or list."
    }
  ]
}
```

Diagnostic get prints one catalog entry as JSON. Workspace-scoped catalogs also
include declared custom lint rules such as `payments/max-token-budget`.

## Stability Notes

The documented fields are intended for automation. Consumers should ignore
unknown fields so future versions can add detail without breaking existing
tools.
