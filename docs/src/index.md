# Introducing rototo

rototo is a control plane for runtime configuration.

It is built around a simple premise: configuration that changes production behavior should move through the same discipline as code, even when the application does not need to be redeployed.

rototo gives teams two things:

* Runtime configuration that stays inside the software lifecycle: review,
  tests, CI, observability, and rollback.
* Long-running applications that can [refresh](reference-sdk-refresh.html)
  reviewed configuration without restarting or redeploying the application
  binary.

## Why rototo exists

Most production systems eventually need behavior to vary by environment, account, request context, rollout state, or operational condition.

At first, the values look harmless: a limit, a switch, a model name, a prompt, a rollout bucket, an exception for one customer. Then one of those values starts controlling real production behavior, and the place where it lives begins to matter.

Environment variables are familiar, but they often couple configuration changes to deploys or restarts. Feature flag systems solve part of the runtime problem, but they can create a release path that drifts away from the code, tests, and review process that depend on them. Bespoke admin systems are even more expensive: authentication, authorization, audit logs, validation, approvals, APIs, migrations, rollback, and the operating habits around all of it.

Coding agents make this drift more costly. Code changes faster. More features are in flight at the same time. Runtime configuration expands with them. Engineers and agents both need the same answers: Can this configuration break behavior? How is it tested? Who reviewed it? What changed? How do we recover?

rototo is the system I kept wanting: runtime configuration that can change without an application redeploy, but still moves through review, tests, observability, and rollback.

## The rototo model

rototo treats runtime configuration as reviewable workspace files.

A [workspace](reference-workspace-layout.html) is a directory tree rooted at
`rototo-workspace.toml`. It is versioned in git and contains the
[variables](reference-variables.html), [qualifiers](reference-qualifiers.html),
[catalogs](reference-catalogs.html), request contexts, and
[custom lint rules](reference-custom-lua-lint.html) that define runtime policy.

At runtime, an application is deployed with a
[workspace source](reference-workspace-sources.html) URI. The
[rototo SDK](reference-sdk-loading.html) loads that source, lints the
workspace, and [resolves named variables](reference-sdk-resolution.html) using
the [runtime context](reference-context.html) provided by the application.

For long-running services, successful
[refreshes](reference-sdk-refresh.html) affect future resolutions. Failed
refreshes keep the last successfully loaded workspace active.

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

Schemas and [custom Lua lint rules](reference-custom-lua-lint.html) define
what valid configuration means. [Built-in lint](reference-lint-overview.html)
catches malformed workspace structure, unknown references, invalid primitive
values, context mismatches, and schema failures. Custom lint captures the local
policy only your team knows.

Application tests can load the same workspace source the service will use and
assert the values selected for important runtime contexts. That catches
failures workspace lint cannot see: the app expected an integer, the workspace
now selects a structured entry, or the app no longer provides the facts the workspace
expects. [Testing Runtime Configuration](testing-runtime-configuration.html)
covers that app-workspace contract.

Because resolution happens through the SDK in the application process, existing observability can explain what value was selected, from which workspace version, and why.

## Where rototo fits

rototo fits when a configuration value changes application behavior and deserves release discipline.

Common examples include:

* Account and environment-specific limits;
* [Operational switches](operational-switches.html);
* [Onboarding checklists](onboarding-checklist.html) that vary by account state;
* Account-specific exceptions;
* [Bucketed rollouts](bucketed-rollout.html);
* [Incident banners](incident-banner.html);
* [Service degradation policies](service-degradation-policy.html);
* Model, prompt, and provider settings;
* [Runtime policy for another system](notification-delivery-policy.html).

rototo is not ordinary application storage. User records, transactions, analytics events, and high-volume mutable data should stay in the systems that already own them.

## Start here

Start with [Getting Started](getting-started.html). It builds one account
limit end to end: workspace files, CLI resolution, SDK loading, and refresh.

Then read [Configuration Primitives](configuration-primitives.html) for the
model the whole system shares: the few primitives, how they compose into one
resolution, and where the model deliberately stops.

Then read the examples when you want to model a similar production case.
[Modeling Runtime Configuration](modeling-runtime-configuration.html),
[Application Integration](application-integration.html),
[Testing Runtime Configuration](testing-runtime-configuration.html), and
[Operating Runtime Configuration](operating-runtime-configuration.html) turn
those examples into habits for running rototo in a service. The reference pages
are there when you need exact file formats, commands, SDK APIs, and
[JSON output](reference-json-output.html).
