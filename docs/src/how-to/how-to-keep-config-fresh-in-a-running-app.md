# How to Keep Config Fresh in a Running App

Use this when a deployed application should pick up reviewed configuration
changes without being redeployed.

The model is: the application is deployed with a workspace source URI, loads the
workspace at startup, and refreshes that source periodically. Future
resolutions use the latest successfully loaded workspace.

## Expected outcome

After this change:

- The application uses a movable workspace source such as a production branch.
- Startup loads a valid workspace before serving traffic.
- Background refresh updates future resolutions after successful config
  releases.
- Refresh failures keep last-known-good config and emit telemetry.

## Use a movable workspace source

Refresh only helps when the source can move:

```text
git+https://github.com/acme/runtime-config.git#prod:config
```

Here, `prod` is a branch or promotion ref. When the config repository updates
that ref, the next refresh can load a newer workspace.

Pinned commit refs and tags are useful for reproducibility, but they should not
be used when the goal is continuous refresh.

## Load once at startup

At startup, load the workspace from the configured source URI. Startup should
fail if the application cannot load a valid workspace and has no previous
snapshot to use.

This gives the application a clear boot contract: it either has reviewed
configuration or it does not start serving traffic.

## Refresh in the background

Configure the application to refresh the workspace on an interval appropriate
for the blast radius of config changes. Shorter intervals reduce release
latency; longer intervals reduce load on the source repository and make changes
less immediate.

On refresh success, future resolutions should use the new workspace version.

On refresh failure, the application should keep using the last successfully
loaded workspace and emit telemetry. A transient Git or network failure should
not erase the configuration the service is already using.

## Record refresh state

Expose refresh state in application telemetry:

```text
workspace_source
workspace_version
last_refresh_started_at
last_refresh_succeeded_at
last_refresh_error
```

Resolution telemetry should include the workspace version, variable id,
environment, context summary, selected value key, and whether resolution
succeeded. That is what lets operators connect a production decision to the
workspace release that produced it.

## Common mistakes

Do not assume refresh means every request sees the same workspace version during
a rollout. Running instances refresh independently.

Do not use an immutable source ref when expecting refresh to pick up changes.

Do not hide refresh failures. A service can continue on last-known-good config,
but operators still need to know refresh is failing.

Do not put final selection logic in application code after resolution. If the
workspace refreshed correctly but the app branches afterward, the runtime
decision is no longer fully observable through rototo.

## Related docs

- `model` explains the runtime refresh lifecycle.
- `source-uri-reference` explains source ref behavior.
- `sdk` explains SDK loading and refresh APIs.
