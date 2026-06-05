# Production Workflow with rototo

In the `getting-started`, you got a feel of an end to end process with rototo but it was still a toy example and not something that you could take to production. We would know layer in the steps that you would like to take before you move it to production.

We would do the following:
- Reconfigure `max-output-tokens` variable so that premium accounts get higher limit
- Move rototo workspace to hosted git so that production systems can access it
- Setup `rototo lint` for our variable `max-output-tokens` so that it's value is appropriate
- Add tests to the app that would ensure contracts between app and config hold up at runtime

## Make variable more interesting

Earlier, we defined `max-output-tokens` to have only one value. Now, we would change it so that premium accounts get higher output tokens.

Create `token-config/qualifiers/premium-account.toml`:

```toml
schema_version = 1
description = "Users with premium account"

[[predicate]]
attribute = "user.account.type"
op = "eq"
value = "premium"
```

Update `token-config/variables/max-output-tokens.toml`:

```toml
schema_version = 1
description = "Maximum output tokens for LLM responses"
type = "int"

[values]
standard = 2000
large = 4000

[resolve]
default = "standard"

[[resolve.rule]]
qualifier = "premium-account"
value = "large"
```

We created a `qualifier` that identifies premium accounts and updated `max-output-tokens` rules so that premium accounts get `large` value of `4000` and everyone else gets `standard` value of `2000`.

The dir tree now looks as follows:

```sh
token-config/
  |- rototo-workspace.toml
  |- lint/
  |- schemas/
  |- resources/
  |- qualifiers/
    |- premium-account.toml
  |- variables/
    |- max-output-tokens.toml
```

## Move rototo workspace to git

The `getting-started` example used a local dir for rototo workspace. We would now host `token-config` on git and have rototo sdk read it from git server.

First, we need to change dir to `token-config`.

```sh
cd /path/to/token-config
```

Now, we would initialize it as a git repository and host it on GitHub as a private repo. The following script assumes that you have `gh` installed and authentication has already been done.

```sh
git init .
git add .
git commit -m "Init rototo workspace"

# TODO: write the commands to create github private repo and put its location in WORKSPACE_URI variable.

```

We now have a git-hosted `token-config` workspace. Next, we would pass `$WORKSPACE_URI` to our `token-app`.

```sh
cd /path/to/token-app

cargo run -- $WORKSPACE_URI
```

That's it. rototo sdk will now load the configuration from git instead of loading it from a local folder. When you want to change the configuration, follow the usual edit, commit, PR, CI workflow.

## Ensure contracts in dev workflow

Now, we want to make sure that we have a robust workflow that would avoid mistakes. First, we would make sure that `max-output-tokens` has syntactically and semantically appropriate values. We would do it by using a custom lint rule written in `lua` that would ensure that values are integers _and_ they `standard` value is never more than the `large` value. After all, we don't want premium users to be at a disadvantage.

```lua
// TODO: add the lua linter
```

Next, we would add `rototo lint` to the pre-commit hook. This way, before the commit, we can catch any errors in the configuration.

TODO: add pre-commit hook details.

We should also add tests that prove premium and non-premium accounts based on `user.account.type` get different values. We can run these tests as part of CI job.

TODO: add tests

## Add tests to the app
