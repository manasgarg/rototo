# Resolution Output Reference

Resolution output is the operational explanation for a selected value. It
answers three questions: what value was selected, which rule selected it, and
which qualifier predicates caused that rule to match or skip.

Use this output when logs, CI, tests, or support tools need to explain runtime
selection without reimplementing rototo's resolution logic. The shapes below
are the stable JSON contract returned by `rototo resolve --json`.

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
    "value_key": "enterprise",
    "value": {
      "projects": 100,
      "members": 250
    }
  },
  "default_value": "growth",
  "rules": [
    {
      "index": 0,
      "qualifier": "enterprise-account",
      "value": "enterprise",
      "matched": true
    }
  ],
  "qualifier_traces": []
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `resolution.id` | string | Variable id. |
| `resolution.value_key` | string | Selected value key. |
| `resolution.value` | JSON value | Selected value after TOML to JSON conversion. |
| `default_value` | string | Default value key from `[resolve]`. |
| `rules` | array | Rule outcomes in resolve order. |
| `qualifier_traces` | array | Qualifiers evaluated while resolving the variable. |

Rule indexes are zero-based and match the order of `[[resolve.rule]]` tables in
the variable file.

## Qualifier Trace

A qualifier trace shows the final boolean value and every predicate that
contributed to it:

```json
{
  "id": "enterprise-account",
  "value": true,
  "predicates": [
    {
      "index": 0,
      "kind": "compare",
      "attribute": "account.plan",
      "op": "eq",
      "expected": "enterprise",
      "actual": "enterprise",
      "result": true
    }
  ]
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Qualifier id. |
| `value` | boolean | Final qualifier result. |
| `predicates` | array | Predicate traces in qualifier file order. |

Predicate indexes are zero-based and match the order of `[[predicate]]`
tables.

## Compare Predicate Trace

Compare predicates read either runtime context or another qualifier and compare
the actual value against the configured expectation:

```json
{
  "index": 0,
  "kind": "compare",
  "attribute": "account.plan",
  "op": "in",
  "expected": ["growth", "enterprise"],
  "actual": "enterprise",
  "result": true
}
```

`expected` is the predicate value from the workspace. `actual` is the value read
from context or from a referenced qualifier.

When the predicate reads another qualifier, the trace also includes
`qualifier`:

```json
{
  "attribute": "qualifier.paid-account",
  "qualifier": "paid-account",
  "actual": true
}
```

## Bucket Predicate Trace

Bucket predicates are useful only if the assignment is explainable. The trace
includes the computed bucket value so operators can see why a stable input fell
inside or outside the configured range:

```json
{
  "index": 0,
  "kind": "bucket",
  "attribute": "account.id",
  "bucket": {
    "salt": "billing-policy-2026-06",
    "start": 0,
    "end": 1000,
    "value": 427
  },
  "result": true
}
```

`bucket.value` is omitted only when the runtime context value was unavailable,
which normally appears as a resolution error before a successful trace is
returned.

## Human Output

Without `--json`, `rototo resolve` prints the same information in a compact
human format. Use JSON for tests, automation, and logs that need a stable
shape.
