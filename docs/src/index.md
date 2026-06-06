# Introducing rototo

rototo is a control plane for runtime configuration.

It is built around a simple premise: configuration that changes production behavior should move through the same discipline as code, even when the application does not need to be redeployed.

rototo gives teams two things:

* Runtime configuration that stays inside the software lifecycle: review, tests, CI, observability, and rollback.
* Long-running applications that can refresh reviewed configuration without restarting or redeploying the application binary.

## Why rototo exists

Most production systems eventually need behavior to vary by environment, account, request context, rollout state, or operational condition.

At first, the values look harmless: a limit, a switch, a model name, a prompt, a rollout bucket, an exception for one customer. Then one of those values starts controlling real production behavior, and the place where it lives begins to matter.

Environment variables are familiar, but they often couple configuration changes to deploys or restarts. Feature flag systems solve part of the runtime problem, but they can create a release path that drifts away from the code, tests, and review process that depend on them. Bespoke admin systems are even more expensive: authentication, authorization, audit logs, validation, approvals, APIs, migrations, rollback, and the operating habits around all of it.

Coding agents make this drift more costly. Code changes faster. More features are in flight at the same time. Runtime configuration expands with them. Engineers and agents both need the same answers: Can this configuration break behavior? How is it tested? Who reviewed it? What changed? How do we recover?

rototo is the system I kept wanting: runtime configuration that can change without an application redeploy, but still moves through review, tests, observability, and rollback.

## The rototo model

rototo treats runtime configuration as reviewable workspace files.

A workspace is a directory tree rooted at `rototo-workspace.toml`. It is versioned in git and contains the variables, qualifiers, schemas, resources, and custom lint rules that define runtime policy.

At runtime, an application is deployed with a workspace source URI. The rototo SDK loads that source, lints the workspace, and resolves named variables using the runtime context provided by the application.

For long-running services, successful refreshes affect future resolutions. Failed refreshes keep the last successfully loaded workspace active.

The core loop is:

1. Edit workspace files.
2. Review the diff.
3. Run lint and tests.
4. Merge the change.
5. Let applications refresh the workspace source and use the new values.

The configuration moves independently from the application binary, but it does not move outside the engineering process.

## What rototo gives you

The point is not that configuration lives in TOML. The point is that runtime policy becomes visible, reviewable, testable, and recoverable.

Code and configuration can live in git. Teams can review them together, test them together, and use repository history as the operational record of what changed.

Schemas and custom Lua lint rules define what valid configuration means. Built-in lint catches malformed workspace structure, unknown references, invalid primitive values, context mismatches, and schema failures. Custom lint captures the local policy only your team knows.

Application tests can load the same workspace source the service will use and assert the values selected for important runtime contexts. That catches failures workspace lint cannot see: the app expected an integer, the workspace now selects an object, or the app no longer provides the facts the workspace expects.

Because resolution happens through the SDK in the application process, existing observability can explain what value was selected, from which workspace version, and why.

## Where rototo fits

rototo fits when a configuration value changes application behavior and deserves release discipline.

Common examples include:

* Account and environment-specific limits;
* Operational switches;
* Account-specific exceptions;
* Bucketed rollouts;
* Incident banners;
* Model, prompt, and provider settings;
* Runtime policy for another system.

rototo is not ordinary application storage. User records, transactions, analytics events, and high-volume mutable data should stay in the systems that already own them.

## What adoption looks like

Start with one account limit.

Pick something important enough to deserve review, but small enough that the first loop stays clear. Create a workspace, put the value behind a named variable, and use the CLI to lint and resolve it. The workspace should prove that it can stand on its own.

Then load the workspace from the application through the SDK. The application should ask for a named variable and provide runtime context. It should not parse workspace files or duplicate resolution rules.

Once that loop works locally, add the production pieces around it: context schemas, qualifiers, custom lint, generated fixtures, app tests, pre-commit checks, CI, and a hosted git workspace source.

The examples follow that path.

Getting started builds the first account limit. Operational switches show how reviewed policy changes affect a running service. Incident banner returns a validated structured payload. Onboarding checklist demonstrates list values, qualifier composition, and test-account enablement before wider release. Bucketed rollout shows deterministic percentage rollout for a stable account slice. Notification delivery policy shows how rototo can select reviewed policy for another runtime system without owning that system’s mutable state. Service degradation policy shows how reviewed policy and stable buckets help teams try recovery variations without redeploying the service. Workspace layering shows how product, customer, and team owners can share one configuration model without sharing one administrative boundary.

The adoption section turns those examples into production habits: model the runtime decision first, treat workspaces as administrative boundaries, integrate through the SDK, test behavior, release carefully, and observe what was selected.

The reference section comes last. It specifies the contracts readers need once they understand the operating model: workspace layout, source loading, layering, context, qualifiers, variables, resources, resolution output, CLI commands, SDK loading and refresh, lint, diagnostics, custom Lua lint, and JSON output.

From there, the regular develop, test, review, and release process applies to runtime configuration. The workspace can grow with the domain without changing the loop that made the first value safe to operate.
