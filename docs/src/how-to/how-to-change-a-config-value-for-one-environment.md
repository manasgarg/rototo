# How to Change a Config Value for One Environment

Use this when one environment needs a different value while the application
contract stays the same.

The usual example is changing production behavior without changing development
or staging. The application still resolves the same variable id; only the
environment mapping changes.

## Expected outcome

After this change:

- The variable id is unchanged.
- One environment points at a different value key.
- Other environments keep their previous behavior.
- The changed and unchanged environments both resolve as expected.

## Locate the variable

Find the variable file under `variables/`:

```text
variables/max-output-tokens.toml
```

The relevant section is the environment block:

```toml
[variable.env.prod]
value = "standard"
```

## Add or choose the target value

If the desired value key already exists, reuse it:

```toml
[variable.values]
small = 500
standard = 1000
large = 2000
```

If it does not exist, add a new value key first:

```toml
[variable.values]
small = 500
standard = 1000
large = 2000
extra_large = 3000
```

Keep the value key descriptive. The key appears in resolution output and should
make sense in logs and review.

## Change the environment mapping

Update only the environment that should change:

```toml
[variable.env.prod]
value = "large"
```

Leave other environment blocks unchanged:

```toml
[variable.env.dev]
value = "small"

[variable.env.stage]
value = "standard"
```

This preserves the application contract. Application code continues to resolve
`max-output-tokens`, but production now receives the `large` value key.

## Verify the affected environment

Lint the workspace:

```sh
rototo workspace lint config/
```

Resolve the changed environment:

```sh
rototo variable resolve max-output-tokens \
  --workspace config/ \
  --env prod \
  --context '{}'
```

Resolve at least one unchanged environment as a regression check:

```sh
rototo variable resolve max-output-tokens \
  --workspace config/ \
  --env stage \
  --context '{}'
```

## Common mistakes

Do not change `[variable.env._]` when only one named environment should change.
The fallback can affect any environment without its own block.

Do not point an environment at a value key that does not exist. Lint catches
this, but review should catch the intent.

Do not change the variable id for an environment-specific change. That would
force application code to know about deployment lanes.

## Related docs

- `variable-reference` specifies environment mappings.
- `environment-reference` explains fallback behavior.
