# Operating Runtime Configuration

Once tests are in place, rototo changes can move independently from the
application binary. That is the point, but it changes the operating model.

A workspace commit can change production behavior as soon as a long-running
service refreshes its workspace source. The application did not redeploy, but
the behavior changed. I would treat that as a release and operate it with the
same care: clear review, narrow blast radius, observable selection, and a
recovery path that people understand before they need it.

The day-two habits are where the model either keeps paying off or quietly
turns back into ad hoc configuration.

## Treat Workspace Changes As Releases

The smallest useful review for a runtime configuration change should answer:

- which variables can select a different value;
- which value keys are new, changed, or removed;
- which runtime conditions can match the new path;
- which accounts, environments, buckets, or workspace layers are affected;
- which tests prove the intended behavior;
- how to recover if the policy is wrong.

That does not need a ceremony-heavy process. It does need the reviewer to see
the behavior delta, not just the TOML diff.

For example, this is reviewable:

```text
Change account-limit-profile:
- add value key preview-enterprise
- route test accounts in prod to preview-enterprise
- leave standard and enterprise defaults unchanged
- verified with rototo fixtures and account-app policy tests
- rollback: remove the preview rule or revert this commit
```

This is much harder to operate:

```text
Update config
```

The workspace is the control plane. Its commits deserve commit messages and
pull request descriptions that explain the runtime decision being changed.

## Choose Source Refs Deliberately

Application deployments should be explicit about the workspace source they
load:

```text
ROTOTO_WORKSPACE_SOURCE=git+https://github.com/acme/runtime-config.git#main:workspaces/prod
```

A branch or tag ref lets a long-running service discover later reviewed
workspace commits through refresh. That is the usual choice for services that
should receive runtime policy updates without a restart.

A full commit SHA gives reproducibility:

```text
ROTOTO_WORKSPACE_SOURCE=git+https://github.com/acme/runtime-config.git#2f3c4d5e6f708192aabbccddeeff001122334455:workspaces/prod
```

That is useful for jobs, migrations, audits, and deployments where the exact
workspace version must not move. It also means refresh will not discover newer
commits from that source. Pinning is a tradeoff: better reproducibility, no
ongoing updates.

Write that choice down in the app or deployment docs. Operators should not
have to infer from a URI whether a service is expected to refresh.

## Narrow The First Runtime Scope

Most risky policy changes should start with a narrow runtime scope. Rototo
gives several ways to do that without adding app-side policy:

- test accounts;
- account classes;
- deployment lanes;
- deterministic buckets;
- customer or team workspace layers.

I usually prefer adding a new value key before changing the default path:

```toml
[values]
standard = { max_projects = 10, audit_retention_days = 30 }
enterprise = { max_projects = 100, audit_retention_days = 365 }
preview_enterprise = { max_projects = 150, audit_retention_days = 365 }

[resolve]
default = "standard"

[[resolve.rule]]
qualifier = "test-enterprise-account"
value = "preview_enterprise"

[[resolve.rule]]
qualifier = "enterprise-account"
value = "enterprise"
```

That shape gives review and rollback a clean handle. If the preview value is
wrong, remove the preview rule or point it back at `enterprise`. The existing
default and enterprise paths stay visible and unchanged.

For bucketed changes, keep the bucket condition stable and change the
percentage deliberately. A jump from 5 percent to 50 percent is a larger
release than it looks in a one-line diff.

For layered workspaces, use the narrowest administrative boundary that matches
ownership. A customer-specific override belongs in the customer layer. A
support-team override belongs in the support-team layer. The application still
loads one assembled workspace source, but the operational blast radius follows
the layer that changed.

## Observe Selection, Refresh, And Freshness

The runtime logs should explain which reviewed configuration was used without
dumping the full payload.

For each important resolution, log:

- variable id;
- selected value key;
- workspace fingerprint;
- relevant request, account, or tenant identifier;
- service deployment identity when useful.

For refresh, expose:

- current workspace fingerprint;
- last successful refresh time;
- last attempted refresh time;
- consecutive refresh failures;
- last refresh error;
- whether the source is immutable.

Those fields make the common production questions answerable:

```text
Which workspace version is this service using?
Did it pick up the latest reviewed commit?
Which value key did this account receive?
Is the service serving last-known-good because refresh is failing?
```

Do not make operators reconstruct those answers from repository history alone.
Repository history tells you what was approved. Runtime observability tells you
what this process actually loaded and selected.

## Alert On Stale Refresh

Failed refreshes keep the last successfully loaded workspace active. That is
the right runtime behavior, but it still needs an alert.

An alert should fire when the service has not successfully refreshed within the
freshness window you expect for that source. The window depends on the system.
For some services, five minutes is too long. For others, an hour is fine.

The alert should point at the workspace source and the last refresh error. The
first operator question is usually whether the workspace is broken, the source
is unreachable, or the service no longer has access.

Treat stale refresh as a control-plane incident, not as an app crash. The
service may still be serving valid last-known-good configuration, but it is no
longer receiving reviewed changes.

## Roll Back Through Git First

When a workspace policy is wrong, the first recovery path should usually be a
workspace revert:

```sh
git revert <bad-workspace-commit>
git push
```

Services following a branch source can refresh to the reverted workspace. The
application binary did not change because the app-workspace contract is still
valid; the policy was wrong.

Redeploy the application when the contract is wrong:

- the app sent the wrong context shape;
- the app cannot deserialize a valid selected value;
- the app applies the selected policy incorrectly;
- the service is configured with the wrong workspace source.

That distinction matters during an incident. If policy is wrong, fix policy in
the workspace. If the app-workspace boundary is wrong, fix the app or its
deployment configuration.

## Keep Emergency Changes Reviewable

Incidents sometimes need fast configuration changes. Fast should not mean
invisible.

For urgent workspace changes, keep the path short but still reviewable:

- make one policy change per pull request when possible;
- include the exact runtime condition and value key being changed;
- run `rototo lint` and the relevant fixture or app tests;
- get approval from the owner of the affected administrative boundary;
- record the recovery command or revert commit in the incident notes.

If your organization has a break-glass path, make it explicit in the workspace
repository. The dangerous part is not speed. The dangerous part is a hidden
side path that bypasses the same files, tests, and history everyone else uses.

## Keep The Operating Boundary Clear

An operated rototo integration has a clean split:

- the workspace owns reviewed policy;
- the app owns runtime facts and applying selected policy;
- CI owns lint, fixtures, and app contract tests;
- observability owns selected value keys, fingerprints, and refresh state;
- git owns recovery history.

When that split holds, configuration can move quickly without becoming
mysterious. A workspace change can reach a running service through refresh, and
the team can still answer the questions that matter: what changed, who reviewed
it, where did it apply, what did the app select, and how do we recover?
