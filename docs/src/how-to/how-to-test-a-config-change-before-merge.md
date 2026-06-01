# How to Test a Config Change Before Merge

Use this when a workspace change should be proven before review or before a
production promotion ref moves.

The goal is to test the same behavior reviewers care about: the workspace loads,
lint passes, and representative runtime requests resolve to the expected value
keys and values.

## Expected outcome

After this change:

- The config repository has small context fixtures for the behavior under
  review.
- CI or a local test script runs lint and representative resolutions.
- Reviewers can see which value keys should be selected before the change
  merges.

## Start with lint

Run workspace lint for every change:

```sh
rototo lint config/
```

Lint catches structural problems: invalid manifests, missing schemas, unknown
environments, unknown value keys, invalid values, invalid qualifier references,
and custom lint diagnostics.

## Add representative context fixtures

Create fixtures for the cases the change is meant to affect:

```text
config/tests/
  prod-enterprise.context.json
  prod-default.context.json
```

Example matching context:

```json
{
  "account": {
    "plan": "enterprise",
    "seats": 250
  }
}
```

Example default context:

```json
{
  "account": {
    "plan": "team",
    "seats": 25
  }
}
```

These fixtures should be small and intentional. They document the behavior the
change is supposed to preserve.

## Resolve the changed variable

Run the changed variable against each fixture:

```sh
rototo resolve config/ --variable llm-agent-config \
  --env prod \
  --context @config/tests/prod-enterprise.context.json \
  --json
```

```sh
rototo resolve config/ --variable llm-agent-config \
  --env prod \
  --context @config/tests/prod-default.context.json \
  --json
```

Check both `value_key` and `value`. The key explains the branch that matched;
the value is what the application will receive.

## Automate the checks

Put the commands in the config repository's test script or CI job:

```sh
rototo lint config/
rototo resolve config/ --variable llm-agent-config --env prod \
  --context @config/tests/prod-enterprise.context.json --json
rototo resolve config/ --variable llm-agent-config --env prod \
  --context @config/tests/prod-default.context.json --json
```

If your repository uses expected JSON files, compare the selected `value_key`
and important fields in `value`. Avoid snapshots that make harmless metadata
changes hard to review.

## Common mistakes

Do not test only the happy path. Include the default or non-matching context.

Do not rely on lint alone for rollout changes. Lint can prove the workspace is
valid, but representative resolution proves the behavior.

Do not use production secrets or full request payloads as fixtures. Keep
fixtures minimal and safe to review.

## Related docs

- `cli-reference` lists lint and resolve commands.
- `json-output-reference` specifies fields for assertions.
- `diagnostic-reference` explains lint failures.
