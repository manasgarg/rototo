# Diagnostics Reference

Diagnostics are the contract between rototo, humans, CI, editor integrations,
and agents. A diagnostic tells you which rule failed, where it failed, how
serious it is, and what to change.

## Diagnostic Shape

[JSON diagnostics](reference-json-output.html) use this shape:

```json
{
  "rule": "rototo/variable-unknown-value",
  "severity": "error",
  "stage": "reference",
  "target": {
    "entity": {
      "kind": "rule",
      "variable": "account-limits",
      "index": 0
    },
    "field": {
      "kind": "variable_rule_value"
    }
  },
  "message": "rule references unknown value: enterprise",
  "help": "Create the referenced value or update the resolve reference.",
  "location": {
    "path": "variables/account-limits.toml",
    "range": {
      "start": { "line": 12, "character": 8 },
      "end": { "line": 12, "character": 20 }
    }
  },
  "related": []
}
```

## Rule Ids

Built-in rules use the `rototo` authority:

```text
rototo/<rule-id>
```

Examples:

```text
rototo/workspace-manifest-missing
rototo/qualifier-when-shape
rototo/variable-unknown-value
rototo/catalog-entry-schema-mismatch
```

Custom rules use:

```text
<authority>/<rule-id>
```

The `rototo` authority is reserved. Each rule-id part must contain lowercase
ASCII letters, digits, or hyphens.

## Severity

Severity values are:

```text
error
warning
```

`rototo lint` exits with failure when selected diagnostics include any error.
Warnings are still reported because they often point at confusing or risky
workspace behavior.

## Targets

Diagnostics can target:

```text
workspace
manifest
qualifier
variable
catalog
catalog_entry
value
rule
custom_lint
schema
```

The target gives tools a stable object and optional field to group by or attach
to a form control. The location gives humans and text editors the file and range
to edit. Both are part of the diagnostic contract.

## Catalog

The diagnostic catalog describes available rules:

```sh
rototo show --lint-rules
rototo show examples/basic --lint-rules
rototo show examples/basic --lint-authority rototo
```

The global catalog contains built-in rototo rules. A workspace-scoped catalog
also includes [custom rules](reference-custom-lua-lint.html) registered by
`lint/*.lua`.

Catalog values contain:

```json
{
  "rule": "rototo/variable-unknown-value",
  "severity": "error",
  "entity": "variable",
  "title": "Variable references an unknown value",
  "help": "Create the referenced value or update the resolve reference."
}
```

## Stability

Treat rule ids and targets as stable automation inputs. CI filters, editor
integrations, and agents should use `rule`, `severity`, `stage`, and `target`
rather than matching diagnostic message text.

Messages are written for humans and may become more specific over time.
