# How to Diagnose a Failing Workspace

Use this when `rototo lint` fails and you need to turn the diagnostic
into a concrete fix.

The goal is to identify whether the failure is in the workspace manifest,
context schema, qualifier, variable, value file, reference, or custom lint
policy.

This page is for load, lint, and validation failures. If resolution succeeded
but returned an unexpected value, use
`how-to-investigate-why-a-value-was-selected` instead.

## Expected outcome

After this investigation:

- You know which file or rule failed.
- You know the stable diagnostic rule.
- You can inspect the diagnostic help text.
- You can rerun lint after the fix and get a clean result.

## Run lint with the intended workspace source

Use the same workspace source the failing workflow used:

```sh
rototo lint config/
```

or:

```sh
rototo lint 'git+https://github.com/acme/runtime-config.git#prod:config'
```

If CI failed on a Git source, reproduce with that Git URI instead of your local
working tree. Otherwise you may debug a different workspace version.

## Read the diagnostic rule first

Lint diagnostics include a stable rule id. Use that rule to inspect the catalog:

```sh
rototo show --lint-rule rototo/qualifier-parse-failed
```

For automation, use JSON:

```sh
rototo show --lint-rule rototo/qualifier-parse-failed --json
```

The catalog explains the entity, severity, title, and recovery guidance. For
custom lint rules, pass the workspace so the catalog can include declared
non-rototo authorities.

## Narrow the failure area

Use the diagnostic rule, path, and message to decide where to look:

```text
workspace manifest failure -> rototo-workspace.toml
context schema failure     -> schemas/context.schema.json
qualifier failure          -> qualifiers/<id>.toml
variable failure           -> variables/<id>.toml
value validation failure   -> inline values or *-values/*.toml
custom lint failure        -> lint script and expanded variable values
```

Parse failures usually mean syntax. Reference failures usually mean a missing
environment, qualifier id, value key, schema path, or lint path. Validation
failures usually mean a value or context field does not match its declared
contract.

## Inspect the affected object

If lint points at a variable, inspect it:

```sh
rototo show config/ --variable llm-agent-config
```

If lint points at a qualifier, inspect it:

```sh
rototo show config/ --qualifier enterprise-accounts
```

Inspection helps confirm what rototo loaded, including expanded variable values
from external files.

## Re-run the smallest useful check

After editing, rerun workspace lint:

```sh
rototo lint config/
```

Then resolve the affected variable or qualifier with representative context:

```sh
rototo resolve config/ --variable llm-agent-config \
  --env prod \
  --context '{"account":{"plan":"enterprise","seats":250}}'
```

Lint proves the workspace is valid. Resolution proves the repaired decision
behaves as intended.

## Common mistakes

Do not fix diagnostics from memory. Look up the diagnostic rule and confirm the
actual rule.

Do not debug against local files when CI failed against a Git ref.

Do not stop at a passing lint result after a behavior change. Re-run at least
one representative resolution.

Do not ignore custom lint diagnostics because they are local policy. They are
part of the workspace release gate.

## Related docs

- `diagnostics` explains diagnostic fields and catalog commands.
- `cli` lists lint, get, and resolve commands.
- `workspace-manifest-reference` specifies the manifest.
- `variable-reference` and `qualifier-reference` specify object validation.
