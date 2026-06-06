# Diagnostics Reference

Diagnostics are the contract between rototo, humans, CI, editor integrations,
and agents. A diagnostic tells you which rule failed, where it failed, how
serious it is, and what to change.

## Diagnostic Shape

JSON diagnostics use this shape:

```json
{
  "rule": "rototo/variable-unknown-value",
  "severity": "error",
  "stage": "reference",
  "entity": {
    "kind": "rule",
    "variable": "account-limits",
    "index": 0
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
rototo/qualifier-predicate-unknown-op
rototo/variable-unknown-value
rototo/resource-object-schema-mismatch
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

## Entity Kinds

Diagnostics can target:

```text
workspace
manifest
qualifier
predicate
variable
resource
resource_object
value
rule
custom_lint
schema
```

The entity gives tools a stable object to group by. The location gives humans
the file and range to edit.

## Catalog

The diagnostic catalog describes available rules:

```sh
rototo show --lint-rules
rototo show examples/basic --lint-rules
rototo show examples/basic --lint-authority rototo
```

The global catalog contains built-in rototo rules. A workspace-scoped catalog
also includes custom rules registered by `lint/*.lua`.

Catalog entries contain:

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

Treat rule ids as stable automation inputs. CI filters, editor integrations,
and agents should use `rule`, `severity`, `stage`, and `entity` rather than
matching diagnostic message text.

Messages are written for humans and may become more specific over time.
