# Resolution Output Reference

Resolution output is the operational explanation for a selected value. It
answers three questions: what value was selected, which rule selected it, and
which qualifier conditions caused that rule to match or skip.

Use this output when logs, CI, tests, or support tools need to explain runtime
selection without reimplementing rototo's resolution logic. The shapes below
are the stable [JSON contract](reference-json-output.html) returned by
`rototo resolve --json`.

## Top Level

The top level reports the workspace source and the selected target traces:

```json
{
  "workspace": "examples/basic",
  "variables": [],
  "qualifiers": []
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `workspace` | string | Workspace source path used by the command after staging. |
| `variables` | array | Variable resolution traces. |
| `qualifiers` | array | Qualifier resolution traces. |

Only selected targets are included. `rototo resolve --variable account-limits`
returns variable traces and an empty qualifier array. `--qualifier paid-account`
does the inverse. Mixed selectors may return both.

## Variable Trace

A variable trace starts with the selected result, then shows the default and
the rule outcomes that led to that result:

```json
{
  "resolution": {
    "id": "account-limits",
    "value": {
      "projects": 100,
      "members": 250
    },
    "source": {
      "kind": "catalog",
      "catalog": "account-limit-profile",
      "value": "enterprise"
    }
  },
  "default_value": "growth",
  "default_source": {
    "kind": "catalog",
    "catalog": "account-limit-profile",
    "value": "growth"
  },
  "rules": [
    {
      "index": 0,
      "qualifier": "enterprise-account",
      "value": "enterprise",
      "source": {
        "kind": "catalog",
        "catalog": "account-limit-profile",
        "value": "enterprise"
      },
      "matched": true
    }
  ],
  "qualifier_traces": []
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `resolution.id` | string | Variable id. |
| `resolution.value` | JSON value | Selected value after TOML to JSON conversion. |
| `resolution.source` | object | Selected source. Literal values use `{ "kind": "literal" }`; catalog values include `catalog` and `value`. |
| `default_value` | JSON value | Default value from `[resolve]`. |
| `default_source` | object | Default source. |
| `rules` | array | Rule outcomes in resolve order. |
| `qualifier_traces` | array | Qualifiers evaluated while resolving the variable. |

Rule indexes are zero-based and match the order of `[[resolve.rule]]` tables in
the [variable file](reference-variables.html).

## Qualifier Trace

A qualifier trace shows the condition rototo evaluated and the final boolean
value:

```json
{
  "id": "enterprise-account",
  "when": "context.account.plan == \"enterprise\"",
  "value": true
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Qualifier id. |
| `when` | string | Expression from the qualifier file. |
| `value` | boolean | Final qualifier result. |

## Human Output

Without `--json`, `rototo resolve` prints the same information in a compact
human format. Use JSON for tests, automation, and logs that need a stable
shape.
