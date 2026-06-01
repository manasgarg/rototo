# How to Load Config from a Git Repo in an App

Use this when application code should load a reviewed workspace from a separate
configuration repository.

The goal is to deploy the application with a workspace source URI, not with
configuration files copied into the application repository.

## Expected outcome

After this change:

- The workspace lives in its own Git repository.
- The application is configured with a `git+...` workspace URI.
- The URI is verified from the CLI before app deployment.
- Application code can load the reviewed workspace through the SDK.

## Publish the workspace from Git

Keep the workspace in a configuration repository:

```text
runtime-config/
  config/
    rototo-workspace.toml
    qualifiers/
    variables/
    schemas/
```

Choose the ref the application should load. A common production convention is a
mutable promotion ref such as `prod`:

```text
git+https://github.com/acme/runtime-config.git#prod:config
```

This URI means: clone the Git repository, read ref `prod`, and load the
workspace from the `config` directory.

## Test the source URI from the CLI

Before changing application code, verify that the URI loads:

```sh
rototo inspect 'git+https://github.com/acme/runtime-config.git#prod:config'
```

Then resolve one application-facing variable:

```sh
rototo resolve 'git+https://github.com/acme/runtime-config.git#prod:config' --variable llm-agent-config \
  --env prod \
  --context '{"account":{"plan":"team","seats":25}}'
```

This catches URI mistakes before they become application startup failures.

## Configure the application

Store the source URI in application deployment configuration:

```text
ROTOTO_WORKSPACE=git+https://github.com/acme/runtime-config.git#prod:config
```

At startup, application code should read that URI and load the workspace through
the SDK. The application can then resolve stable variable ids such as
`llm-agent-config` or `max-output-tokens`.

## Use refs intentionally

Use a branch or promotion ref when running services should receive future
configuration releases after refresh.

Use a tag or commit ref when you need reproducibility and do not expect refresh
to move to a newer workspace version.

The application does not need to change when the `prod` ref moves. It keeps the
same workspace URI and refreshes the loaded workspace according to the
application's refresh policy.

## Common mistakes

Do not deploy the application with a local path that only exists on a
developer's machine.

Do not point production services at an unreviewed development branch unless
that is an intentional release policy.

Do not use an immutable tag when you expect running services to pick up future
config releases from refresh.

## Related docs

- `source-uri-reference` specifies Git URI syntax.
- `sdk` explains application integration.
- `model` explains workspace source and refresh lifecycle.
