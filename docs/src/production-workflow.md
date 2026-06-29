# Production Workflow

Development work ends when a package change is ready for review. Production
work starts when that reviewed package becomes the configuration a running
service uses.

The important boundary is the package source. Production should not depend on a
developer's checkout, generated runtime state, or a database row that bypasses
review. It should load a Rototo package from a source that CI checked and
reviewers approved. That keeps configuration deployable separately from the
application binary without removing the engineering controls that make the
change safe.

This page covers the production loop:

- choose the package source that production will load;
- run CI against the reviewed package;
- review file changes and resolution behavior together;
- release the reviewed package as an immutable artifact;
- load the package from the application;
- refresh long-running services while keeping last-known-good state.

## Choose the Package Source

Applications should receive a package source URI as deployment configuration.
That URI says where Rototo should load the reviewed package from; the
application still asks for named variables and qualifiers at runtime.

A local path is useful in development:

```sh
ROTOTO_PACKAGE_SOURCE=app-config
```

Git is still the source of truth for review. CI should check the package from
the Git commit under review, and smaller deployments can load that Git source
directly:

```sh
ROTOTO_PACKAGE_SOURCE='git+https://github.com/acme/runtime-config.git#main:packages/checkout'
```

The fragment has two parts: the Git ref and the package subdirectory. In the
example above, `main` is the ref and `packages/checkout` is the package root
inside that repository.

At fleet scale, direct GitHub loading should not be the default runtime
transport. If every long-running service instance refreshes from
`git+https://github.com/...`, the configuration read path inherits GitHub
availability, authenticated API limits, and git fetch behavior that is not
friendly to edge caching. That is a distribution problem, not a resolution
problem. Qualifiers, variables, context validation, and value selection still
happen in process after the package is loaded.

For production distribution, project the reviewed package into an immutable
archive at release time and serve it from an object store behind a CDN:

```sh
ROTOTO_PACKAGE_SOURCE='https://config.acme.com/rototo/checkout/prod/current.tar.gz'
```

From the SDK's point of view, this is an ordinary HTTPS archive source. Archive
sources can use bearer auth, conditional refresh through `ETag` or
`Last-Modified`, and the same last-known-good behavior as any other package
source.

Moving refs and immutable refs have different operational meanings:

- a branch ref, such as `main` or `production`, can refresh to newer reviewed
  package commits;
- a tag ref is useful for named releases;
- a full commit ref is reproducible, but refreshing it will keep loading the
  same package state;
- an immutable archive URL, such as one addressed by a `sha256` digest, is
  reproducible and cacheable with a long TTL;
- a channel archive URL, such as `prod/current.tar.gz`, can move to a newer
  digest and should use a short TTL.

Use immutable refs when the application release must be exactly reproducible.
Use moving refs when the service is expected to receive reviewed configuration
changes without an application redeploy. The application code does not change in
either case; only the package source contract changes.

For private HTTPS package sources, provide a bearer token through
`ROTOTO_PACKAGE_TOKEN` or the CLI's `--package-token` flag. The post-release
load check should use the same authenticated archive source production will use,
so CI proves the application can actually load the released package.

## Run CI as the Release Gate

CI has three jobs before release. Each one catches a different failure mode, so
they should not be collapsed into one smoke command.

First, lint the package:

```sh
rototo lint "$PACKAGE_UNDER_TEST"
```

Lint proves that the package files parse, references resolve, schemas compile,
catalog entries match their schemas, context samples match their context
schemas, custom lint registers correctly, and the whole package can become a
runtime model.

Second, run hard resolution expectations. These are the production cases where
an accidental change would matter: a qualifier that must match, a variable that
must select a specific catalog entry, or a literal value that must not drift.

Store those expectations as data. A compact JSONL file works well because each
line is one independently reviewable case:

```json
{"name":"premium qualifier matches","qualifier":"premium-users","context":"app-config/evaluation-contexts/request-samples/premium.json","expect":true}
{"name":"premium checkout selects catalog entry","variable":"checkout-redesign","context":"app-config/evaluation-contexts/request-samples/premium.json","expectSource":{"kind":"catalog","catalog":"checkout-redesign","value":"premium"}}
{"name":"default message remains stable","variable":"premium-message","context":"app-config/evaluation-contexts/request-samples/free.json","expectValue":"Welcome back."}
```

The cases should live beside the application or package CI configuration. Use
evaluation context samples from the package when possible, because those samples
are also validated by lint. Keep the list focused on invariants, not every
possible runtime permutation.

`rototo resolve --json` is the stable interface for these checks. The following
script is an application-owned pattern, not a separate Rototo command:

```sh
#!/usr/bin/env bash
set -euo pipefail

package_source="${1:?usage: check-rototo-resolution.sh <package-source> <cases.jsonl>}"
cases_file="${2:?usage: check-rototo-resolution.sh <package-source> <cases.jsonl>}"

python3 - "$package_source" "$cases_file" <<'PY'
import json
import subprocess
import sys

package_source = sys.argv[1]
cases_file = sys.argv[2]
failures = []


def run_case(case):
    args = ["rototo", "resolve", package_source, "--json"]
    if "variable" in case:
        args.extend(["--variable", case["variable"]])
        kind = "variable"
    elif "qualifier" in case:
        args.extend(["--qualifier", case["qualifier"]])
        kind = "qualifier"
    else:
        raise ValueError("case must declare variable or qualifier")

    args.extend(["--context", "@" + case["context"]])
    completed = subprocess.run(args, text=True, capture_output=True)
    if completed.returncode != 0:
        raise AssertionError(completed.stderr.strip() or completed.stdout.strip())

    payload = json.loads(completed.stdout)
    if kind == "qualifier":
        actual = payload["qualifiers"][0]["value"]
        expected = case["expect"]
        if actual != expected:
            raise AssertionError(f"expected qualifier {expected!r}, got {actual!r}")
        return

    resolution = payload["variables"][0]["resolution"]
    if "expectSource" in case:
        actual = resolution["source"]
        expected = case["expectSource"]
        if actual != expected:
            raise AssertionError(f"expected source {expected!r}, got {actual!r}")
    if "expectValue" in case:
        actual = resolution["value"]
        expected = case["expectValue"]
        if actual != expected:
            raise AssertionError(f"expected value {expected!r}, got {actual!r}")


with open(cases_file, encoding="utf-8") as cases:
    for number, line in enumerate(cases, start=1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        case = json.loads(line)
        try:
            run_case(case)
            print(f"ok {case['name']}")
        except Exception as exc:
            failures.append(f"{cases_file}:{number}: {case.get('name', '<unnamed>')}: {exc}")

if failures:
    print("\n".join(failures), file=sys.stderr)
    sys.exit(1)
PY
```

Then CI can run:

```sh
rototo lint "$PACKAGE_UNDER_TEST"
ci/check-rototo-resolution.sh "$PACKAGE_UNDER_TEST" ci/rototo-resolution-cases.jsonl
```

Third, run the application integration test. `rototo resolve` proves Rototo's
view of the package. The application test proves the application still builds
the expected runtime context, loads the package through the SDK, deserializes
values into application types, and exercises the code path that depends on
those values.

For example:

```sh
ROTOTO_PACKAGE_SOURCE="$PWD/app-config" \
  cargo test -p checkout-service rototo_config_contract
```

That test should use the application's normal configuration loading path. It
should not shell out to `rototo resolve` from inside application code. The
point is to test the SDK integration and the application's context contract,
not only the package model.

The full CI gate then has a clear shape:

```sh
set -euo pipefail

export PACKAGE_UNDER_TEST="${PACKAGE_UNDER_TEST:-app-config}"

rototo lint "$PACKAGE_UNDER_TEST"
ci/check-rototo-resolution.sh "$PACKAGE_UNDER_TEST" ci/rototo-resolution-cases.jsonl
ROTOTO_PACKAGE_SOURCE="$PWD/$PACKAGE_UNDER_TEST" \
  cargo test -p checkout-service rototo_config_contract
```

When the package lives in a separate repository, run the same commands in that
repository and run the application integration test in a job that checks out
both the application and the package revision under review.

## Review Resolution Behavior

Reviewers need the file diff, but they also need to know what the diff changes
at runtime. `rototo diff` keeps the review anchored in production cases instead
of asking reviewers to infer behavior from TOML and JSON alone.

Compare the pull request against the production base:

```sh
rototo diff app-config \
  --from origin/main \
  --to HEAD \
  --context @app-config/evaluation-contexts/request-samples/premium.json
```

The useful review signal is specific: this variable now selects a different
catalog entry, this qualifier started matching, this catalog value changed
shape, or no runtime selection changed for the checked case.

If the CI system can attach artifacts, attach the relevant diff output. If it
cannot, print the important diffs in the job log and summarize them in the pull
request. A package change is ready to merge when lint passes, hard resolution
expectations pass, the application integration test passes, and reviewers can
see the intended runtime behavior.

## Release the Package Artifact

After CI and review pass, release should turn the reviewed package into the
source that production instances load. For a small deployment that may be a Git
ref. For a fleet, prefer an HTTPS archive distributed through object storage and
a CDN.

The release action should do five things:

1. Check out the exact Git commit being released.
2. Run the same package gate CI ran: lint, hard resolution expectations, and the
   application integration test.
3. Pack the package root into a deterministic archive.
4. Upload the archive under a content address, such as `sha256:<digest>`, with a
   long cache lifetime.
5. Atomically move the environment channel pointer, such as `prod/current`, to
   that digest with a short cache lifetime.

The archive should be deterministic: sorted entries, fixed permissions, zeroed
mtimes, and reproducible compression. The same package commit should produce
the same digest. That makes the digest URL immutable and gives rollbacks a
concrete target.

The channel pointer is the moving part. A production service can deploy with the
channel URL:

```sh
ROTOTO_PACKAGE_SOURCE='https://config.acme.com/rototo/checkout/prod/current.tar.gz'
```

or with a pinned digest URL:

```sh
ROTOTO_PACKAGE_SOURCE='https://config.acme.com/rototo/checkout/sha256:0f4c...b91.tar.gz'
```

The pinned form makes configuration part of the application release and avoids
runtime drift. The channel form lets a long-running service refresh into newly
reviewed configuration without an application redeploy. Promotion from staging
to production and rollback are pointer moves, not package rewrites.

After the upload and pointer move, run a load check against the public release
source:

```sh
rototo lint "$ROTOTO_PACKAGE_SOURCE"
ci/check-rototo-resolution.sh "$ROTOTO_PACKAGE_SOURCE" ci/rototo-resolution-cases.jsonl
```

That final check proves the released artifact is reachable and resolves the
same hard cases as the reviewed package.

## Load the Package in the Application

Production applications should use the SDK, not the CLI, for runtime loading and
resolution. The CLI is for authoring, CI, review, and operations. The SDK keeps
package loading, lint gating, context validation, qualifier evaluation, variable
resolution, and typed error handling in process.

A Rust service can load the configured package source at startup:

```rust
use rototo::{EvaluationContext, Package};

let source = std::env::var("ROTOTO_PACKAGE_SOURCE")?;
let package = Package::load(source).await?;

let context = EvaluationContext::from_json(serde_json::json!({
    "user": { "tier": "premium" },
    "request": { "country": "DE" }
}))?;

let resolution = package.resolve_variable("checkout-redesign", &context)?;
let checkout = resolution.value;
```

The application owns the runtime context because it knows the request,
account, user, tenant, or environment facts. The package owns the schema and
the rules that interpret those facts. That split is what lets package authors
change reviewed behavior without changing how the service is deployed.

## Refresh Long-Running Services

Most production services run longer than a single package release. If the
service should receive reviewed package changes without an application redeploy,
load it with refresh support:

```rust
use rototo::{EvaluationContext, RefreshOptions, RefreshingPackage};

let source = std::env::var("ROTOTO_PACKAGE_SOURCE")?;
let package = RefreshingPackage::load(source, RefreshOptions::new()).await?;
let context = EvaluationContext::from_json(serde_json::json!({}))?;

let resolution = package.resolve_variable("checkout-redesign", &context)?;
```

A successful refresh affects future resolutions. A failed refresh keeps the
last successfully loaded package active, so a bad package update does not
replace known-good configuration in a running service.

Refresh behavior depends on the package source. A branch ref can discover new
commits. A channel archive URL can discover a new artifact through the
archive's conditional refresh metadata. A pinned commit ref or digest archive is
immutable, so it is excellent for reproduction but does not produce new package
states on refresh.

## Observe the Rollout

Refresh moves one process forward. That is enough for a single service, but it
leaves an operational blind spot the moment you run more than one instance: you
published a reviewed change, and now you need to know whether the fleet actually
accepted it. Did every running instance load `sha256:4d1c...`? Which instances
are stale, still on the previous package, or failing to refresh? Without an
answer, a configuration rollout is a hope, not a fact.

The SDK cannot answer fleet questions on its own, because it does not know
service identity, deployment identity, or which instances count. Your
observability system already knows those things. So rototo reports the package
facts, and the application publishes them with the labels its operations team
already uses. The boundary matters: rototo stays out of your telemetry vendor,
and you attach `service`, `region`, and `instanceId` exactly once, at the edge.

The unit everyone compares against is the package's release id. Every loaded
package has a stable identity derived from its source fingerprint: a Git commit
becomes `git:<commit>`, a content-addressed archive becomes its `sha256:...`
digest. That release id is what a dashboard joins on across the fleet.

Start with the snapshot, because it answers "what is true now" and survives a
restarted instance that has no event history yet. Publish it as a heartbeat:

```rust
let snapshot = package.snapshot();
let identity = &snapshot.identity;

tracing::info!(
    service = "checkout-api",
    instance_id = %instance_id,
    release_id = identity.release_id.as_deref().unwrap_or("none"),
    last_success = ?snapshot.last_success,
    consecutive_failures = snapshot.consecutive_failures,
    "rototo heartbeat",
);
```

A rollout is complete when, for every instance your platform considers active,
the most recent heartbeat reports the target release id, is recent enough to
trust, and is not stuck failing refresh:

```text
for every active instance:
  snapshot.releaseId == target_release_id
  snapshot.reportedAt >= now - freshness_window
  snapshot.consecutiveFailures == 0 or failures are accepted
```

Rototo does not implement this check. Deciding which instances are active —
excluding the ones that are draining or terminating — belongs to the platform
that owns the fleet, not to the SDK.

Snapshots tell you what is true now; events tell you what changed and exactly
when. Subscribe to the refresh event stream when you want transition times in
your logs, traces, or an audit table:

```rust
let mut events = package.subscribe_refresh_events();
tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        tracing::info!(
            event_type = event.event_type.as_str(),
            release_id = event
                .current
                .as_ref()
                .and_then(|identity| identity.release_id.as_deref())
                .unwrap_or("none"),
            duration_ms = event.duration.as_millis(),
            "rototo refresh event",
        );
    }
});
```

The stream is best-effort: it never blocks refresh, and a consumer that falls
behind drops the oldest events rather than stalling the service. That is a
deliberate trade. If the stream is the only thing you watch, a lagging consumer
can miss a transition — so treat events as the audit trail and the snapshot as
the source of truth you reconcile against.

The same three surfaces — `identity()`, `snapshot()`, and the event stream —
exist in every language SDK, adapted to local idioms: an async iterator in
Python and TypeScript, a channel in Go, a listener in Java. The release id,
snapshot, and event shapes are shared across bindings so one dashboard reads
every service.

A word on what reaches users. If you build a customer-facing view that shows
which configuration version is active, expose the release id deliberately and
nothing else by default. Package source URLs can carry credentials, evaluation
context carries request facts, and rule text is internal. Rototo redacts source
strings for you — userinfo and bearer tokens never appear in an identity or
event — but the decision about which catalog ids, qualifier outcomes, or
selected values are safe to reveal is application policy, so the application
makes it.

## Trace a Single Resolution

Observability tells you which version is live and whether refresh is healthy.
The next question usually arrives as a support ticket: *this one user is seeing
the wrong checkout variant — why?* Reproducing it locally rarely works, because
the answer depends on that user's runtime context against the live package.

Resolution tracing answers that question without a redeploy. A trace is the full
execution record of one resolve: the rules attempted, which one matched, every
qualifier outcome, the selected value and its source, and the request context it
ran against. It is verbose and meant for debugging, so it is emitted
selectively, never on every resolve.

There are two ways to ask for one. An application can request a trace on a
specific call:

```rust
use rototo::ResolveOptions;

let options = ResolveOptions {
    trace: true,
    ..ResolveOptions::default()
};
let resolution = package.resolve_variable_with_options(
    "checkout-redesign",
    &context,
    options,
)?;
```

The more useful form for a production ticket lives in the package, so you can
turn tracing on for exactly the case you are chasing through a reviewed change —
no application deploy. Add a `[[trace]]` policy to `rototo-package.toml`:

```toml
[[trace]]
when = 'env.resolving.variable == "checkout-redesign" && context.user.id == "tester-123"'
```

The `when` is the same expression language used everywhere else. It reads
`context.*` and composes named conditions with `env.qualifier["<id>"]`, and
inside a trace policy it may additionally read `env.resolving.variable` and
`env.resolving.qualifier` — the entity currently being resolved. That binding
exists only here; a qualifier or rule cannot read it, because a condition must
stay a function of context, not of who is asking. When the policy matches a
resolution, rototo emits the trace.

Both forms deliver to one place: the trace stream. An application subscribes and
forwards traces to its logs or debugger, off the resolve path:

```rust
let mut traces = package.subscribe_trace_events();
tokio::spawn(async move {
    while let Some(item) = traces.recv().await {
        match item {
            rototo::TraceStreamItem::Trace(event) => tracing::info!(
                target_id = event.target_id(),
                "rototo resolution trace\n{:#}",
                event.to_json(),
            ),
            rototo::TraceStreamItem::Dropped { count } => {
                tracing::warn!(count, "rototo trace events dropped")
            }
        }
    }
});
```

Two properties make this safe to leave configured. Tracing is computed only when
something is listening: with no subscriber, a `[[trace]]` policy costs nothing,
because rototo skips the work entirely. And the stream is bounded — a consumer
that falls behind drops the oldest traces and receives a `Dropped { count }`
marker rather than stalling resolution. That marker matters when you are
debugging: silence then means *not traced*, never *traced but lost*.

Buffer depth is a deployment choice, not a package one, because the package
author cannot know a consumer's traffic or memory budget. Size it with
`LoadOptions::with_trace_capacity` where you load the package. The `when` decides
*which* resolutions are interesting; the buffer decides how much the consumer can
absorb.

One caution on the context. A trace carries the full request context so you can
see exactly what the resolve saw, and that context often holds user identifiers.
Redaction before logging is the application's responsibility, the same boundary
as the release id: rototo gives you the facts, the application decides what is
safe to persist.

## Roll Back by Moving the Package Source

Because production consumes a package source, rollback should happen at that
boundary. Revert the package change, move the production branch back to a known
good commit, move the CDN channel pointer back to a known-good digest, or deploy
an application configuration that points at an earlier immutable ref.

The application does not need to know why the package changed. It keeps loading
and resolving the same variable and qualifier names. Rototo validates the next
package before it becomes active, and refresh keeps the previous package active
if the new one cannot be loaded.

That is the production shape: CI proves the package is valid and preserves hard
runtime expectations, review shows the behavior change, the application loads a
reviewed source through the SDK, and refresh moves running services forward
only when the next package can be trusted.
