# Introducing rototo

rototo is a control plane for runtime configuration. It is built around a small
idea that I want in every production system: configuration that changes
behavior should move through the same discipline as code, without requiring the
application to restart every time a value changes.

That leaves rototo with two jobs:

- Keep runtime configuration in the software lifecycle: review, tests, CI,
  observability, and rollback.
- Let long-running applications refresh reviewed configuration without
  redeploying the application binary.

## Motivation

Most applications I have worked on eventually need runtime behavior to vary by
deployment environment, account settings, request context, or system state. The
values look small at first: a limit, a switch, a model name, a prompt, a rollout
bucket. Then one of those values changes production behavior, and the storage
choice starts to matter.

Environment variables are familiar, but they tend to tie configuration changes
to redeploys or restarts. Feature flagging systems can solve part of the
runtime problem, but they often create a release path that drifts away from the
code and tests that depend on it. Bespoke admin systems bring a larger bill:
authentication, authorization, audit logs, validation, approvals, APIs,
migrations, rollback, and the operational habits around all of those pieces.

Coding agents raise the cost of that drift. Code changes faster, more features
are in flight at once, and runtime configuration expands with them. Engineers
and agents both need a place to answer the same questions: can this
configuration break behavior, how do we test that, who reviewed it, what
changed, and how do we recover?

rototo is the thing I kept wanting: runtime configuration that can change
without an app redeploy, but still goes through code review, tests, and
rollback.

## The Model

rototo treats runtime configuration as reviewable workspace files. A workspace
is a directory tree rooted at `rototo-workspace.toml`, versioned in git, with
variables, qualifiers, schemas, resources, and custom lint rules living beside
it.

At runtime, the app is deployed with a workspace source URI. The rototo SDK
loads that source, lints the workspace, and resolves named variables from the
runtime context the app provides. In long-running services, successful
refreshes affect future resolutions. Failed refreshes keep the last
successfully loaded workspace active.

That is the core loop I care about:

1. Edit workspace files.
2. Review the diff.
3. Run lint and tests.
4. Merge the change.
5. Let applications refresh the workspace source and use the new values.

The configuration moved independently from the application binary, but it did
not move outside the engineering process.

## What rototo Gives You

The practical benefit is not that configuration lives in TOML. The benefit is
that everyone can see which files own the runtime policy and which checks must
pass before the app uses it.

Code and configuration can both live in git. You can review them together, test
them together, and use the repository history as the operational record of what
changed.

Schemas and custom Lua lint rules let the workspace say what valid
configuration means. Built-in lint catches malformed workspace structure,
unknown references, invalid primitive values, context mismatches, and schema
failures. Custom lint handles the local policy only your team knows.

Application tests can load the same workspace source the service will use and
assert the values selected for important runtime contexts. That catches the
failures workspace lint cannot see: the app expects an integer, the workspace
now selects an object, or the app no longer sends the facts the workspace
expects.

And because the SDK runs in the application process, your existing
observability path can explain what value was selected, from which workspace
version, and why.

## Where rototo Fits

rototo fits anywhere a configuration value changes application behavior and
needs release discipline. The examples I reach for are:

- Account and environment-specific limits;
- Operational switches;
- Account-specific exceptions;
- Bucketed rollouts;
- Incident banners;
- Model, prompt, and provider settings.

rototo is not ordinary application storage. User records, transactions,
analytics events, and high-volume mutable data should stay in the systems that
already own them.

## What Adoption Looks Like

I would start with one account limit. Pick something that matters enough to
deserve review, but is small enough that the first loop stays clear.

First, create a workspace and put that value behind a named variable. Use the
CLI to lint and resolve it, so the workspace proves it can stand on its own.

Then load the workspace from the app with the SDK. The app should ask for a
named variable and provide runtime context; it should not parse workspace files
or duplicate resolution rules.

Once the loop works locally, add the production pieces around it: context
schemas, qualifiers, custom lint, generated fixtures, app tests, pre-commit,
CI, and a hosted git workspace source.

The learning examples follow that path. Getting started builds the first
account limit. Operational switches show how reviewed policy changes affect a
running service during operations. Incident banner shows how to return a
validated structured payload. Onboarding checklist shows list values,
qualifier composition, and test-account enablement before a wider change.
Bucketed rollout shows deterministic percentage rollout for a stable account
slice. Notification delivery policy shows how rototo can select reviewed policy
for another runtime system without owning that system's mutable state. Service
degradation policy shows how reviewed policy and stable buckets help teams try
recovery variations without redeploying the service. Workspace layering shows
how product, customer, and team owners can share one configuration model
without sharing one administrative boundary.

The adoption section comes next because examples are not enough by themselves.
It turns the model into the habits I would want a team to use in production:
model the runtime decision first, treat workspaces as administrative
boundaries, integrate through the SDK, test behavior, release carefully, and
observe what was selected. Modeling runtime configuration starts there.
Application integration shows how the app should call rototo. Testing runtime
configuration shows what to prove before release. Operating runtime
configuration covers release, observability, and recovery after refresh is
live. Production workflow ties those pieces together.

The reference section is last because it specifies the contracts readers need
once they understand the operating model: workspace layout, source loading,
layering, context, qualifiers, variables, resources, resolution output, CLI
commands, SDK loading and refresh, lint, diagnostics, custom Lua lint, and JSON
output.

From there, the regular develop, test, review, and release process applies to
runtime configuration. You can keep expanding the workspace as the domain grows
without changing the loop that made the first value safe to operate.
