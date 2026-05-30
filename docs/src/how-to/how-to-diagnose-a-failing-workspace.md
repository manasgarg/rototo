# How to Diagnose a Failing Workspace

Use this when `rototo workspace lint` fails and you need to turn the diagnostic
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
- You know the stable diagnostic code.
- You can inspect the diagnostic help text.
- You can rerun lint after the fix and get a clean result.

## Run lint with the intended workspace source

Use the same workspace source the failing workflow used:

```sh
rototo workspace lint config/
```

or:

```sh
rototo workspace lint \
  --workspace 'git+https://github.com/acme/runtime-config.git#prod:config'
```

If CI failed on a Git source, reproduce with that Git URI instead of your local
working tree. Otherwise you may debug a different workspace version.

## Read the diagnostic code first

Lint diagnostics include a stable code. Use that code to inspect the catalog:

```sh
rototo diagnostics get rototo/workspace-toml-file-parse-failed
```

For automation, use JSON:

```sh
rototo diagnostics get rototo/workspace-toml-file-parse-failed --json
```

The catalog explains the rule, source, title, and recovery guidance.

## Narrow the failure area

Use the diagnostic kind and message to decide where to look:

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
rototo variable get llm-agent-config \
  --workspace config/
```

If lint points at a qualifier, inspect it:

```sh
rototo qualifier get enterprise-accounts \
  --workspace config/
```

Inspection helps confirm what rototo loaded, including expanded variable values
from external files.

## Re-run the smallest useful check

After editing, rerun workspace lint:

```sh
rototo workspace lint config/
```

Then resolve the affected variable or qualifier with representative context:

```sh
rototo variable resolve llm-agent-config \
  --workspace config/ \
  --env prod \
  --context '{"account":{"plan":"enterprise","seats":250}}'
```

Lint proves the workspace is valid. Resolution proves the repaired decision
behaves as intended.

## Common mistakes

Do not fix diagnostics from memory. Look up the diagnostic code and confirm the
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
