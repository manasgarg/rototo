# Console re-implementation: surfaces, principals, and tenant self-service

Status: draft for review. This spec reopens the console design (and, where
noted, small parts of the package format and CLI) around three personas and a
domain-vocabulary layer. Decisions already made are stated as decisions;
genuinely open items are collected at the end.

## 1. Why re-implement

The current console is a developer workbench that mirrors rototo's raw data
model one to one: source trees, packages, then entity sections for variables,
catalogs, context, and linters. That shape has three problems.

First, the users we now care about do not think in rototo's data model. An app
developer tolerates "variables" and "catalogs", but a pricing manager thinks
about pricing options, a release owner thinks about feature flags, and a
tenant admin thinks about their own plan limits. Today the console offers them
nothing but the raw model.

Second, every console user is implicitly a GitHub user. Identity comes from
GitHub OAuth or an ambient token, write authority comes from GitHub
permissions, and review comes from GitHub PRs and code owners. That works for
app developers and fails for everyone else: internal stakeholders may not
have GitHub accounts at all, and tenant users must never have write access to
the config repository.

Third, the console has drifted from the core model. It still speaks
"qualifier" across roughly twenty files even though the core dissolved that
entity, and its per-principal source-tree model does not fit a team
deployment where an admin registers repositories once for everyone.

The re-implementation keeps the parts that are sound (the axum server, the
SQLite store, the GitHub client, package staging and lint, the resolution
preview, the in-process LSP bridge, the embedded SPA build) and re-centers
the product on three new pillars: surfaces, principals with grants, and a
console-owned write path.

## 2. Personas and their jobs

**App developers** own packages: schemas, variables, catalogs, evaluation
contexts, lint rules, governance, and (new) surfaces. They need the full
entity-level workbench, LSP-backed editing, and the normal GitHub PR flow.
They authenticate with GitHub. They are also the authors of the domain layer
everyone else uses.

**Internal stakeholders** (product, pricing, operations, support) operate the
configuration through domain vocabulary: flip a flag, widen a rollout, change
a price, tune an operational knob. They may have no GitHub account. Their
changes need approval and audit, not code review.

**Tenant users** are admins at external customer organizations. They
self-serve their own tenant's configuration within the contract the app
ships: a tenant overlay package bounded by `governance.toml`. They must see
only their tenant, authenticate with an external identity, and never touch
GitHub or the repository directly.

## 3. The three-layer model

The design splits "who may change what" into three layers, each living where
it can be reviewed by the people who own it:

1. **Surfaces** (in the package, git-backed) declare the domain vocabulary:
   which entities make up "Feature Flags" or "Pricing", how each is edited,
   and which audience sees it. Surfaces are presentation and affordance, not
   authority.
2. **Grants** (in the console store) bind principals to scopes: this person
   may view, propose on, approve for, or administer this package or surface.
   Grants answer *who*.
3. **Governance** (`governance.toml`, already shipped) bounds what any
   overlay may change in its base, deny by default. Governance answers *what
   is possible at all* and is enforced at load time regardless of who asked.

The invariant that makes this safe: a console grant can never exceed
governance. Grants select people; governance bounds packages. A console
administrator handing out roles cannot widen what a tenant overlay may do,
because that bound lives in git under code review.

## 4. Surfaces

A surface is a named, package-defined view over the package's entities, with
editing affordances. Surfaces are a new first-class rototo concept, so the
CLI and console render the same domain layer.

### 4.1 File format

Surfaces live at `surfaces/*.toml` in the package root (top level like
`lint/`, because a surface is neither a contract in `model/` nor a value in
`data/`). The file stem is the surface id: lowercase snake_case with `/`
namespacing, like every rototo-recognized id.

```toml
schema_version = 1
title = "Pricing"
description = "Plans and their limits, owned by the pricing team."
audience = ["internal"]           # "internal", "tenant"; default ["internal"]

[approval]
require = "role:pricing_admins"   # or "none" for auto-merge surfaces

[[section]]
title = "Plans"

[[section.item]]
catalog = "plans"
control = "table"
editable_fields = ["monthly_price", "limits"]

[[section.item]]
variable = "active_plan"
control = "select"
```

An item names exactly one entity (`variable = "<id>"` or `catalog = "<id>"`)
and a `control` describing how it is edited:

- `toggle`: bool variables, including condition variables.
- `rollout`: a variable whose rules use a bucket predicate; the control edits
  the percentage.
- `select`: enum-typed and catalog-typed variables; options come from the
  enum members or catalog entry ids.
- `number`, `text`: int/number/string variables.
- `table`: a catalog; entries render as rows. `editable_fields` limits which
  top-level fields the surface exposes. Optional `can_add` / `can_delete`
  booleans (default false) expose entry creation and deletion.

`editable_fields` and `can_add`/`can_delete` are presentation-level scoping.
They can be narrower than governance but never effectively wider, because
governance is enforced at load time on the resulting files no matter what the
surface claimed.

`[approval]` names the approval requirement for changes submitted through
this surface. `require = "role:<id>"` references a console role by id; the
role's membership lives in the console store (section 5). `require = "none"`
marks the surface auto-merge: changes that pass lint and governance merge
without a human approver. Auto-merge exists for operational kill-switch
surfaces where speed is the point; the default when `[approval]` is absent is
`role:package_approvers` semantics resolved by the console (any principal
with the approve grant on the package).

### 4.2 Composition and tenants

Surfaces compose like `model/` files: whole-file replacement by path. The
important consequence is that a base package declares a tenant-facing surface
once (`audience = ["tenant"]`), and every tenant overlay that extends the
base inherits it. The tenant edits through the inherited surface, and the
writes land as overlay files in the tenant's package, bounded by the base's
governance.

### 4.3 Lint

New built-in rules, all in the existing flat `rototo/<rule-id>` namespace:

- `rototo/surface-parse-failed`, `rototo/surface-schema-version`,
  `rototo/surface-shape`: file parses, declares `schema_version = 1`, and
  matches the surface shape.
- `rototo/surface-unknown-entity`: an item references a variable or catalog
  that does not exist in the composed package.
- `rototo/surface-control-mismatch`: the control does not fit the entity
  (toggle on a non-bool, rollout on a variable with no bucket rule, select on
  a plain string, table fields not in the catalog schema).
- `rototo/surface-empty`: a surface with no items.

The failure fixture at `tests/fixtures/packages/lint-failures` grows cases
for each. `examples/basic` gains at least one surface so the broad example
stays representative.

### 4.4 CLI

Selectors only, no noun subcommands: `--surface <id>` / `--surfaces` join the
existing selector set on `lint`, `inspect`, and `show`. `rototo show
--surface pricing` renders the surface's sections and items with each item's
effective value (the composed default, plus rule count). `resolve` is
unchanged; it still selects only variables.

### 4.5 Semantic model and SDK

The surface model lives in the Rust core next to variables and catalogs, so
lint, CLI, console, and any future SDK need read it from one place. No SDK
resolution behavior changes. `inspect_package` output includes surfaces; no
new public read-by-kind API.

## 5. Principals, roles, and grants

### 5.1 Identity

Three identity sources, resolved per deployment mode:

- **GitHub OAuth** (exists today): app developers in team mode.
- **OIDC** (new): internal stakeholders sign in through the organization's
  identity provider. Configured with the usual issuer/client env vars
  (`ROTOTO_CONSOLE_OIDC_ISSUER`, `ROTOTO_CONSOLE_OIDC_CLIENT_ID`,
  `ROTOTO_CONSOLE_OIDC_CLIENT_SECRET`).
- **Tenant identity** (new, phase 3): per-tenant OIDC or console-issued
  invitations with email verification. Tenant identity configuration hangs
  off the tenant registry entry, so different tenants can bring different
  IdPs. The first slice is invitation-based accounts, designed so per-tenant
  OIDC slots in without reworking sessions.

Local mode keeps its current behavior: no login, ambient GitHub token, single
implicit principal with every capability. Read-only mode also stays as is.

### 5.2 Roles and grants

The console store grows three concepts:

- **Principal**: one row per identity (existing sessions table already
  carries a principal id; it becomes a real entity with identity source,
  display name, email, and optional tenant id).
- **Role**: a named set of principals, administered in the console
  (`pricing_admins`, `support`). Roles are console-scoped, referenced from
  surface `[approval]` blocks by id. The console warns on approval
  requirements naming roles that do not exist.
- **Grant**: `(principal | role) x scope x action`. Scope is a package or a
  specific surface within a package. Actions are ordered: `view` <
  `propose` < `approve` < `administer`. Tenant principals can hold grants
  only within their own tenant's package.

Developers with repository access do not need grants for the workbench in
team mode; their GitHub permissions remain authoritative for the raw-model
editing path. Grants govern the surface path and non-GitHub principals.

### 5.3 Audit

Every mutation writes an audit row: acting principal, tenant (if any),
surface, change-set id, before/after summary, resulting commit SHA and PR
URL, approver principal, timestamps. Audit is append-only and queryable per
package and per tenant. Commits authored by the console app carry an
`Acting-For:` trailer naming the console principal, so the git history alone
identifies the human even without console access.

## 6. Change sets and the write path

### 6.1 GitHub App identity

The console is installed as a GitHub App on the configuration repositories,
with contents and pull-request permissions
(`ROTOTO_GITHUB_APP_ID`, `ROTOTO_GITHUB_APP_PRIVATE_KEY`; installation
discovered per repository). All surface-path writes are authored by the app.
Developer workbench writes keep using the developer's own token as today.

Console writes still go through the GitHub API only. The app identity does
not introduce a generic git write backend; it is the same API with a
different credential.

### 6.2 The change-set lifecycle

Surface edits accumulate into a **change set**, the console's unit of
proposal. One change set maps to one branch and (on submit) one PR.

1. **Draft.** The principal edits controls on one or more surfaces of one
   package. The console's edit planner (section 6.3) turns each control
   change into concrete file edits on a draft branch created by the app.
   Every save runs staged lint; violations surface immediately.
2. **Review.** The console shows the change set as a domain-level diff
   (item, before, after, affected surfaces) plus resolution previews against
   the package's sample contexts, so the proposer sees what callers would
   receive.
3. **Submit.** The console opens a PR with a structured description
   (per-item before/after, acting principal, surface, tenant). The change
   set enters `awaiting_approval`, or merges immediately if every touched
   surface is auto-merge.
4. **Approve.** Principals satisfying every touched surface's approval
   requirement see the change set in their queue. Approval is a console
   action; the console app then merges the PR. Rejection returns the change
   set to draft with a comment. Proposers cannot approve their own change
   sets.
5. **Merged / abandoned.** Merged change sets are immutable audit objects.
   Abandoning deletes the branch and closes the PR.

The PR underneath is the audit substrate and the escape hatch: developers can
review, comment on, or take over any console-originated PR with normal
GitHub tooling. GitHub branch protection on the config repo should permit
the app to merge; organizations that also want code-owner review on
surface-path changes can keep it, at the cost of console approvals waiting on
GitHub review too.

### 6.3 The edit planner

The same logical change ("set `plans/pro.monthly_price` to 49") must produce
different files depending on where it lands:

- In the base package (internal stakeholder editing the app's own config): a
  direct edit of `data/catalogs/plans/pro.toml` or the variable file.
- In a tenant overlay: the structural overlay shapes, `pro.update.toml`,
  `<entry>.deleted.toml`, `variables/<id>.update.toml`, and so on.

The edit planner is a server-side component that owns this mapping. It takes
(package, entity, control change) and emits file operations, then validates
the result by staging and linting, which is also where governance denials
surface as actionable errors ("your plan does not allow deleting the free
plan") rather than raw load failures.

## 7. Tenants

### 7.1 Registry and provisioning

A **tenant registry** lives in the console store: tenant id, display name,
package source for the tenant's overlay, identity configuration, and status.
Provisioning a tenant is a console-admin operation that (a) creates the
overlay package skeleton via the app identity (a `rototo-package.toml` with
`extends` pointing at the base, in whatever repository layout the operator
chose), and (b) registers the tenant.

Per-tenant repository versus per-tenant directory inside one tenants
repository is an operator choice; both are just package sources. One
repository with per-tenant directories is the recommended default (single
app installation, simpler ops); a tenant demanding stronger isolation gets
its own repository without any model change.

### 7.2 Isolation requirements

These are hard requirements on every API route, not UI conventions:

- A tenant principal's requests are scoped to their tenant id by middleware;
  there is no route that enumerates other tenants or their packages.
- Tenants see only surfaces with `tenant` in `audience`, rendered against
  their own composed package. Effective values shown are the composed
  results; base entities not reachable from a tenant-audience surface are
  never serialized to a tenant session.
- Resolution previews for tenant sessions run only against the tenant's own
  composed package and its sample contexts.
- Staged package caches, LSP sessions, and change sets are keyed by tenant.
- Tenant API traffic is rate-limited per tenant.
- The existing mutation invariant (the `x-rototo-console` header plus Origin
  check) stays on every mutating route.

## 8. Console UX

The console becomes domain-first. What a principal sees at sign-in is
determined by their grants:

- **Home** lists the surfaces the principal can access, grouped by package,
  with pending change sets and an approval queue for approvers.
- **Surface view** renders sections and items with current effective values,
  edit controls per the declared affordances, pending-change badges, and
  inline resolution previews ("with this sample context, callers get X").
- **Change set view** shows the domain-level diff, lint status, approval
  state, and the underlying PR link.
- **Workbench** is the current entity-level editor, preserved as an advanced
  mode inside the same app, visible to developer principals (GitHub-backed
  in team mode; everyone in local mode). It gets the vocabulary cleanup
  (qualifiers removed, entity sections matched to the current core model)
  but keeps its architecture: branch editing, Monaco plus LSP bridge, raw
  file access, publish to PR.

Local mode renders everything for the single implicit principal: surfaces
first, workbench one click away. This keeps the single-binary demo story
(`rototo console` in a repo, open browser, see your package as its surfaces).

Wire shapes stay serde camelCase mirrored in `apps/console/src/lib/types.ts`,
Rust as source of truth. The SPA remains a static Vite + React bundle
embedded via `build.rs` and `rust-embed`, no server runtime.

## 9. Server architecture

Kept: axum server under `src/console/`, SQLite store, GitHub REST client,
package staging and lint, resolution preview, in-process LSP bridge,
embedded SPA, deployment/write-policy capability computation.

New or reworked:

- **Surface model** in the Rust core (shared with CLI and lint), plus
  surface-aware endpoints: list surfaces per package per audience, surface
  detail with effective values, control-change submission.
- **Auth**: OIDC alongside GitHub OAuth; GitHub App credentials and
  installation-token management (tokens are short-lived; cached in memory,
  never stored at rest; the app private key comes from env, consistent with
  the existing secret posture).
- **Store schema**: principals, roles, role members, grants, tenants, change
  sets, change-set items, audit log. Existing per-principal `source_trees`
  become deployment-level in team mode: admins register repositories once,
  grants control who sees what. Local mode keeps ad-hoc registration.
- **Change-set state machine** and the edit planner.
- **Tenant scoping middleware** and per-tenant rate limiting.

Deployment modes stay startup-resolved: `local` (default, single user),
`team` (OAuth/OIDC, roles and grants, GitHub App writes), `read-only`
(unchanged). Tenant self-service is a capability of team mode that activates
when the tenant registry is non-empty, not a fourth mode.

The console feature flag in Cargo.toml stays; SDK binding crates keep
building with `default-features = false`.

## 10. What this retires

- The "qualifier" vocabulary everywhere in `src/console/` and
  `apps/console/src/`.
- Per-principal source trees as the team-mode organizing concept (kept for
  local mode).
- The assumption that every console principal has a GitHub identity.
- The console screens' raw-model-first navigation as the default landing
  experience (it survives inside the workbench).

## 11. Phasing

Each phase is independently shippable and `just check`-clean.

- **Phase 0, surfaces in the core.** File format, semantic model, lint
  rules, fixtures, `--surface`/`--surfaces` selectors, `examples/basic`
  surface, docs page. No console changes. This phase is useful on its own:
  `rototo show --surface pricing` already gives CLI users the domain view.
- **Phase 1, domain-first console for GitHub principals.** Surface
  rendering, change sets, edit planner, approvals, GitHub App write path,
  audit log. Identity is still GitHub OAuth plus local mode. Workbench
  becomes advanced mode; qualifier cleanup lands here.
- **Phase 2, non-GitHub principals.** OIDC, principals/roles/grants,
  deployment-level source registration, approval queues driven by roles.
- **Phase 3, tenant self-service.** Tenant registry, provisioning, tenant
  identity, isolation middleware, rate limiting, tenant-audience rendering.

## 12. Non-goals

Consistent with the use-case catalog's existing non-goals:

- No exposure logging, metric-driven rollback, or experimentation analytics.
- No id-list targeting or identity resolution inside rototo.
- No secrets management; surfaces must not become a secrets UI.
- No generic git write backend; writes remain GitHub API only.
- No workflow engine beyond the change-set lifecycle (no multi-step
  approval chains, delegation, or escalation in v1).
- No per-field permission language in grants; field scoping belongs to
  surfaces (presentation) and governance (enforcement).

## 13. Open questions

1. **Naming.** "Surface" is the working name for the domain layer. It reads
   well ("the pricing surface") but is new vocabulary; alternatives
   considered: view, panel, facet. Needs a final call before phase 0 mints
   ids and rule names.
2. **Approval requirement location.** This spec puts `[approval]` in the
   surface file (reviewable policy) with role membership in the console
   store (operational). The alternative is approval policy entirely
   console-side. Leaning git-backed as specced.
3. **Multi-package surfaces.** A surface currently views one package. An
   internal stakeholder overseeing several packages gets several surfaces.
   Is a cross-package dashboard needed, and if so is it a console feature
   rather than a package concept?
4. **Base visibility for tenants.** Effective values necessarily reveal
   composed base values for entities on tenant surfaces. Is that ever a
   problem (e.g. internal-only pricing floors), and do we need a
   surface-level "hide base value, show only own override" affordance?
5. **Change sets touching multiple surfaces with different approvers.**
   Specced as "every touched surface's requirement must be satisfied".
   Simpler alternative: one change set per surface.
6. **Tenant invitation flow details.** Token lifetime, email delivery
   mechanism, and whether local mode can simulate a tenant session for
   development.
