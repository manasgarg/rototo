# JSON Output Reference

`--json` is the automation surface for the rototo CLI. Human output may change
to read better. JSON output is the shape CI, agents, and tests should consume.

## Common Rules

JSON output is pretty-printed.

Diagnostics use the shape described in `reference-diagnostics`.

Document summaries use:

```json
{
  "id": 0,
  "path": "variables/account-limits.toml",
  "uri": "file:///workspace/variables/account-limits.toml",
  "version": null,
  "kind": "variable"
}
```

`version` is reserved for editor and overlay integrations. Disk-backed CLI
documents usually report `null`.

## `lint --json`

```json
{
  "workspace": "/workspace",
  "documents": [],
  "diagnostics": []
}
```

Use this in CI when you need structured diagnostics.

## `show --json`

`show` returns selected config and catalog views:

```json
{
  "command": "show",
  "workspace": "/workspace",
  "schemas": [],
  "resources": [],
  "variables": [],
  "qualifiers": [],
  "lint_rules": [],
  "lint_authorities": [],
  "linters": []
}
```

Selected variables, qualifiers, and resources include `id`, `uri`, `path`, and
their TOML content converted to JSON.

Resource output includes an `objects` table when resource object files exist.

## `inspect --json`

`inspect` returns the most complete workspace explanation:

```json
{
  "workspace": "/workspace",
  "documents": [],
  "runtime": { "status": "available" },
  "diagnostics": [],
  "schemas": [],
  "resources": [],
  "variables": [],
  "qualifiers": [],
  "lint_rules": [],
  "lint_authorities": [],
  "linters": []
}
```

Selected variables, resources, qualifiers, and schemas include dependencies,
consumers, and diagnostics. When `--context` is supplied, selected variables
and qualifiers can include `trace`.

`runtime.status` is `available` when the workspace compiles into a runtime
model. Otherwise it is `unavailable` with a reason.

## `resolve --json`

```json
{
  "workspace": "/workspace",
  "variables": [],
  "qualifiers": []
}
```

See `reference-resolution-output` for trace fields.

## `docs --json`

`rototo docs --json` lists navigation sections:

```json
{
  "sections": [
    {
      "title": "Reference",
      "pages": [
        { "id": "reference-workspace-layout", "title": "Workspace Layout" }
      ]
    }
  ]
}
```

`rototo docs -p <page> --json` returns page metadata and Markdown. `docs -s`
returns search hits with page ids, line numbers, and match spans.

## `fixtures --json`

`fixtures --json` reports the fixture generation result. The generated files
are TOML fixtures under the directory passed with `--out`.

## Error Output

Command-line parse errors and runtime errors are printed to stderr and return a
non-zero exit code. They are not wrapped in the command's JSON success shape.

When automation needs structured lint failures, prefer `rototo lint --json`
over parsing stderr from a failed command.
