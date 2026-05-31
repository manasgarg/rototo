# How to Investigate Why a Value Was Selected

Use this when an application received a value and you need to explain the
decision.

The goal is to reconstruct the same resolution inputs: workspace version,
variable id, environment, and runtime context. Once those are known, rototo can
show the selected value key and value.

This page is for a successful resolution that needs explanation. If the
workspace cannot load or lint fails, use `how-to-diagnose-a-failing-workspace`
instead.

## Expected outcome

After this investigation:

- You know which workspace version was used.
- You can reproduce the resolution from the CLI.
- You know which value key was selected.
- You can tell whether the cause was environment mapping, qualifier matching,
  context shape, or workspace version drift.

## Collect the resolution inputs

Start from application telemetry or an evaluation record. You need:

```text
workspace source or version
variable id
environment
runtime context used for resolution
selected value key, if recorded
```

The most important fields are the variable id, environment, and context. A
different context can select a different value even when the workspace and
environment are the same.

## Inspect the variable

Use the CLI to inspect the variable definition:

```sh
rototo show config/ --variable llm-agent-config
```

Look at the selected environment block. It defines the default value and any
rules that can override that default.

## Inspect referenced qualifiers

If the environment block has rules, inspect the qualifiers referenced by those
rules:

```sh
rototo show config/ --qualifier enterprise-accounts
```

Check the predicates against the runtime context. Missing fields resolve as
non-matches, and a context schema can catch those mismatches before qualifier
evaluation.

## Re-run the resolution

Resolve the variable with the same environment and context:

```sh
rototo resolve config/ --variable llm-agent-config \
  --env prod \
  --context @incident-context.json \
  --json
```

The JSON output includes the selected `value_key` and returned `value`. Compare
that with the application telemetry.

## Check the workspace version

If the CLI result does not match production, verify that you are using the same
workspace version production used. A moving Git ref may now point at a newer
workspace.

For production incidents, prefer investigating against the recorded source
version or commit when available.

## Common mistakes

Do not debug from the current workspace if production used an older version.

Do not inspect only the returned value. The selected value key is usually the
fastest clue for which branch matched.

Do not ignore context validation failures. If context does not match the
workspace schema, the issue may be at the application/config boundary rather
than in the rule itself.

## Related docs

- `json-output-reference` specifies resolution output.
- `variable-reference` explains environment rules.
- `qualifier-reference` explains qualifier evaluation.
- `context-reference` explains missing fields and context schemas.
