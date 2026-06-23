# JSON Output Reference

`--json` is the format automation should use for the rototo CLI. Human output
may change to read better. CI, agents, and tests should consume JSON output.

The rule I use for automation is straightforward: do not parse human output.
Ask for JSON, assert the fields you depend on, and let human output stay
readable for operators.

## Common Rules

JSON output is pretty-printed.

Diagnostics use the shape described in
[Diagnostics](reference-diagnostics.html).

When a command reports package documents, each document summary uses this
shape:

```json
{
  "id": 0,
  "path": "variables/account-limits.toml",
  "uri": "file:///package/variables/account-limits.toml",
  "version": null,
  "kind": "variable"
}
```

`version` is reserved for editor and overlay integrations. Disk-backed CLI
documents usually report `null`.

## `lint --json`

```json
{
  "package": "/package",
  "documents": [],
  "diagnostics": []
}
```

Use this in CI when you need structured diagnostics tied to files, ranges, and
stable rule ids.

## `diff --json`

`diff --json` reports semantic changes between two package sources:

```json
{
  "before": "/package-before",
  "after": "/package-after",
  "changes": [
    {
      "kind": "variable_value_changed",
      "target": {
        "entity": {
          "kind": "value",
          "variable": "premium-message",
          "key": "premium"
        }
      },
      "before": "Welcome back, premium member.",
      "after": "Welcome back, valued premium member.",
      "before_location": {
        "path": "variables/premium-message.toml",
        "range": null
      },
      "after_location": {
        "path": "variables/premium-message.toml",
        "range": null
      }
    }
  ],
  "resolution_impacts": []
}
```

When `--context` is supplied, `resolution_impacts` lists variables whose
resolved value changes for that context.

## `show --json`

`show` returns selected config and catalog views. Use it when automation needs
to read configured package data or the diagnostic catalog:

```json
{
  "command": "show",
  "package": "/package",
  "catalogs": [],
  "variables": [],
  "qualifiers": [],
  "lint_rules": [],
  "lint_authorities": [],
  "linters": []
}
```

Selected variables, qualifiers, and catalogs include `id`, `uri`, `path`, and
their TOML content converted to JSON.

Catalog output includes an `entries` table when catalog value files exist.

## `inspect --json`

`inspect` returns the most complete package explanation. Use it when tooling
needs dependencies, consumers, runtime status, and optional resolution traces:

```json
{
  "package": "/package",
  "documents": [],
  "runtime": { "status": "available" },
  "diagnostics": [],
  "catalogs": [],
  "variables": [],
  "qualifiers": [],
  "lint_rules": [],
  "lint_authorities": [],
  "linters": []
}
```

Selected variables, catalogs, and qualifiers include dependencies,
consumers, and diagnostics. When `--context` is supplied, selected variables
and qualifiers can include `trace`.

`runtime.status` is `available` when the package compiles into a runtime
model. Otherwise it is `unavailable` with a reason.

## `resolve --json`

```json
{
  "package": "/package",
  "variables": [],
  "qualifiers": []
}
```

Use `resolve --json` when automation needs to know what value or qualifier
result rototo selected. See
[Resolution Output](reference-resolution-output.html) for the trace fields.

## `docs --json`

`rototo docs --json` lists navigation sections. This is mainly for docs
publishers and tools that need to mirror the bundled docs order:

```json
{
  "sections": [
    {
      "title": "Reference",
      "pages": [
        { "id": "reference-package-layout", "title": "Package Layout" }
      ]
    }
  ]
}
```

`rototo docs -p <page> --json` returns page metadata and Markdown. `docs -s`
returns search hits with page ids, line numbers, and match spans.

## `fixtures --json`

`fixtures --json` reports the fixture generation result. Use it when CI or
scaffolding tools need to know which fixture files were written. The generated
files are TOML fixtures under the directory passed with `--out`.

## Error Output

Command-line parse errors and runtime errors are printed to stderr and return a
non-zero exit code. They are not wrapped in the command's JSON success shape.

When automation needs structured lint failures, prefer `rototo lint --json`
over parsing stderr from a failed command.
