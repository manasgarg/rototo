# Introducing rototo

rototo is the control plane for runtime configuration of your apps. It is built
with two key objectives:

- Apply the software lifecycle to runtime configuration: tests, reviews, CI,
  observability, and rollback.
- Dynamically refresh runtime configuration without requiring application
  redeployment or restart.

## Motivation

Over the last decade, every application I built needed runtime behavior to vary
by deployment environment, account settings, user context, or system state.
In this context, the same problem kept recurring: these values changed
production behavior, but the usual places for storing them did not handle that
lifecycle well.

Environment variables force redeploys or restarts. Feature flagging systems
create a separate release process that drifts from code release. Specialized
admin systems bring their own surface area: authentication, authorization, audit
logs, validation, approvals, APIs, migrations, and rollback.

Coding agents raise the cost of that drift. Code changes faster, more features
are in flight at once, and runtime configuration expands with them. Engineers
and agents both need to know when a configuration change can break behavior, how
to test it, and how to recover.

rototo is the thing I wished existed: runtime configuration that can change
without redeploying the app, while still going through the discipline of code.

## How rototo approaches the runtime configuration problem

rototo addresses this by treating runtime configuration as both data and code.
Configuration lives as a set of `TOML` files in a directory tree, versioned in
git. At runtime, the rototo SDK loads that configuration directly from git.

When you change configuration, you follow the regular git process: edit,
commit, raise a PR, run CI, and merge. Once the git repo is updated, the rototo
SDK in your app refreshes the configuration without requiring app redeployment or restart.

So, how does rototo help?

- Code and config are both in git. You can bring them together under the same
  directory structure, reason about them together, and keep them cohesive as
  they change.
- You can enforce a contract on the config data schema and values through
  JSON Schema documents and custom Lua linters. Syntactic mistakes in config
  are caught as part of the pre-commit hook.
- CI jobs for application and config repos can run automated tests across the
  shapes of runtime context and configuration values. You can test application
  behavior across the full surface area touched by configuration.
- git gives you audit logs, validation, approval flows, migrations, and
  rollback for configuration changes.
- You can use the same post-deployment observability tools that you have
  already invested in.

That model is useful anywhere a configuration value changes application
behavior and needs release discipline.

## Where rototo fits

rototo can serve a wide variety of runtime configuration needs. Common examples
include:

- Environment and user-specific limits;
- Operational switches;
- Account-specific exceptions;
- Bucketed rollouts;
- Incident banners;
- LLM model, prompt, and token settings.

rototo is not meant for ordinary application data. Keep user records,
transactions, analytics events, and other high-volume mutable data in the
systems that already own them.


## What adoption looks like

rototo is designed to be adopted in a small slice. Specifically:

- Install a CLI that helps you write correct config, and add an SDK to your app
  so it can use that config.
- Represent config as a rototo workspace: a directory tree of `TOML` files. Run
  `rototo resolve` to ensure the config resolves to the expected values based
  on environment and other runtime context.
- Set up pre-commit hooks and CI jobs for app and config repos to ensure they
  are compatible and demonstrate expected behavior.
- Wire up the rototo SDK with your existing observability system.

After that, the regular develop, test, and release process is applied uniformly
to code and configuration.

You can start with one variable and expand to a full representation of your
domain.
