# rototo docs revamp

This page is the clean starting point for the new rototo documentation. The old
publishable docs have been removed from the active docs tree so the rewrite can
start from reader needs instead of from the previous page structure.

## What the docs must do

rototo is about one core idea:

```text
Runtime configuration looks like data, but it changes production behavior like
code.
```

The docs should make that idea land at different attention budgets.

```text
2 seconds  -> What is this?
2 minutes  -> Why should I care?
10 minutes -> Could I adopt this?
30 minutes -> Can I try it?
1 day      -> Can I put my first use case on it?
ongoing    -> Can I operate and debug it?
```

The documentation should be layered around those needs, not around an internal
feature list.

## Reader needs

Curious readers need the point immediately. They should be able to glance at
the homepage and understand that rototo gives behavior-changing runtime
configuration a reviewed, validated lifecycle without requiring an application
redeploy for every approved change.

Evaluators need the problem to feel concrete. They need to see why normal
constants, environment variables, dashboards, JSON files, and database rows can
break down once a configuration value becomes a production decision.

Conviction seekers need the argument to become memorable. The docs should make
the code/data/config lifecycle distinction clear enough that they can repeat it
to another engineer.

Adoption checkers need to know the effort is small. They should see that an app
can start with one workspace source, one variable, one environment, and one
resolution call before growing into refresh, schemas, policy, and operational
observability.

Hands-on users need a low-friction first success. The first tutorial should use
one primitive variable, a local workspace, lint, and resolve. No resources,
custom lint, remote Git, or broad vocabulary should appear before the user has
seen rototo answer one runtime configuration question.

First-use-case users need copyable patterns. They are trying to put a real
decision on rototo: an environment-specific limit, an LLM agent configuration,
an incident banner, a tenant exception, a bucketed rollout, or operational
routing.

Growing users need production guidance. They need help with CI validation,
workspace release, app loading, refresh, last-known-good behavior, rollback,
observability, and debugging surprising selections.

Agents need stable workflows and exact references. They need pages that explain
what can be changed safely, which commands validate a workspace, how resolution
works, and where exact syntax lives.

## Proposed structure

```text
Start
  home
  why-rototo
  how-it-works

Try
  quickstart
  add-runtime-context
  add-structured-config

Adopt
  add-rototo-to-an-app
  move-one-config-value
  adoption-checklist

Use Cases
  environment-specific-limits
  llm-agent-configuration
  incident-banner
  tenant-account-exception
  bucketed-rollout
  operational-routing

Operate
  release-config-separately
  validate-in-ci
  refresh-in-a-running-service
  keep-last-known-good-config
  roll-back-a-config-release
  observe-selected-values

Troubleshoot
  why-did-this-value-get-selected
  why-did-lint-fail
  why-did-context-validation-fail
  why-did-refresh-not-change-anything

Reference
  workspace-format
  variable-format
  resource-format
  qualifier-format
  context-schema
  source-uris
  cli
  sdk
  diagnostics
  json-output
```

## Bundled documentation

```text
Start
  index
```
