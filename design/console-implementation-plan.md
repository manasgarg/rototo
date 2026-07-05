# Console implementation plan

Status: decided sequencing over the six design documents
(`console-identity-authz.md`, `console-git-ops.md`, `console-semantic.md`,
`console-surfaces.md`, `console-system-view.md`, and the layer build orders
inside them). This plan interleaves those build orders into tranches the
way the tenant-config branch ran T0 to T2: each tranche independently
shippable, `just check` clean, and gated before the next begins.

## Ground rules

1. **Greenfield beside, not retrofit inside.** The new TypeScript console
   grows next to the existing Rust console, which stays untouched and
   default until cutover (C7). Reason: the platform migration and the
   qualifier retirement together make in-place retrofitting mostly wasted
   motion, and the old console keeps users working the whole time.
2. **Riskiest novelty first.** The splice engine, the bindings boundary,
   and CAS writes are the pieces most likely to surprise us; they land in
   the earliest tranches where surprises are cheapest.
3. **Every tranche ends at a gate.** Two kinds: **walkthroughs** (a
   scripted persona journey, performed and timed, not just automated) and
   **budgets** (latency assertions in CI from C1 onward: interaction under
   100ms, save acknowledged under 300ms, preview under 500ms on cached
   pins). A tranche is done when its gate passes, not when its code merges.
4. **Two standing decision points**, placed where the plan needs them and
   not before: the Phase A/B product decision (non-GitHub stakeholders in
   the first release?) is needed **before C5**; the single-binary product
   shape (how `rototo console` ships a TypeScript server) is needed
   **before C7**.
5. Tenants stay out of every tranche, as decided throughout.

## The tranches

| Tranche | One line | Ships to |
| --- | --- | --- |
| C0 | Edit engine and pin staging in the core | CLI users |
| C1 | Bindings and the TypeScript server spine | dev flag |
| C2 | Change sets end to end, user tokens | new console (beta) |
| C3 | The read side: previews, graphs, provenance | new console |
| C4 | Surfaces floor and three-delta review | new console |
| C5 | The Phase B cluster: OIDC, grants, App writes | new console |
| C6 | Extension host, table and flags experiences | new console |
| C7 | Cutover and retirement of the old console | everyone |

### C0. Core groundwork (no console involved)

- Edit engine v1: the operation vocabulary over owned entities, splicing
  at model locations, change records, structural validation
  (`console-semantic.md`). `rototo init` moves onto `create_*` operations,
  so the engine has two consumers from day one.
- Pin-keyed staging in the core: shallow fetch by SHA, size-bounded cache
  (`console-git-ops.md` read path substrate).
- Promoted bidirectional reference queries; `extends` edges surfaced
  through discovery.

Gate: engine property tests (splices preserve comments and formatting;
operation to change record fidelity; ownership-aware compilation deferred
but the contract carries the ownership parameter from day one). All
CLI-visible, no console.

### C1. Bindings and the server spine

- The internal binding surface for the console: stage, discover, views,
  diff, lint, semantic model, inspect report, engine apply, traced
  resolution.
- TypeScript server skeleton: sessions (GitHub OAuth and local ambient
  auth ported), `principals` and `identities` tables with credentials on
  identity rows, the `decide()` seam with Backend A (advisory), reworked
  `/api/me`, the mutation-guard invariants, the store.
- New frontend shell: the shared home, empty.

Gate: decide() honesty tests (rendered capability always matches a
recomputed server decision); the latency harness exists and budgets run in
CI from here on.

### C2. Change sets end to end (user tokens only)

- Git-data-API writes: one-commit edit plans, CAS ref updates with the
  expected-pin staleness check, change sets with the four-state machine,
  collaborators, the events diary, the reconciler.
- Workbench editing v1 in the new console: entity views, the raw-text
  path, and the first form-submits-operations editor (variables), with
  lint on the post-edit stage.
- The store fire drill implemented as a test (rebuild from GitHub).

Gate: **developer walkthrough**: browse a package, edit a variable through
the form, watch the change set become a PR, merge it on GitHub, watch the
reconciler observe it. Performed and timed.

### C3. The read side (understanding, all three facets)

- Trace previews at ring 0; the context picker as a first-class fixture;
  the lit-up graph (batch traced resolution binding); the composition tree
  and provenance view; `upcoming_changes`; diagnostics views; the LSP
  bridge ported with definition and references added.
- Layer 3 cold-start empty states (the dependency-chain proposals,
  synthesized-context previews with promotion to samples).
- Qualifier vocabulary is absent by construction; nothing to retire in the
  new console.

Gate: **understanding walkthrough**: answer "what does this package do for
this context" and "what was this value on March 3rd" without touching the
CLI, timed against budgets.

### C4. Surfaces floor and the review fixture

- The `console/surfaces` catalog schema, load-time validation, the null
  renderer with inferred controls, audience filtering, cold-start
  suggestions.
- Surface read side: effective values, previews, upcoming changes, surface
  history, pending change sets.
- The three-delta review panel: semantic diff, resolution impact with
  impact confidence (with-contexts diff binding, synthesis via the
  fixtures machinery, promotion of synthetic contexts to samples), lint
  delta.
- Approval semantics in this tranche are still Backend A (GitHub is the
  authority); surface `approval` fields render and inform but role-based
  enforcement waits for C5.

Gate: **stakeholder-with-GitHub walkthrough**: a PM edits a price through
the floor surface, an approver reads all three deltas and merges. This is
Priya's journey minus "no GitHub account".

### C5. The Phase B cluster

Requires the product decision. Everything here lands together because the
pieces only make sense together:

- OIDC, enrollment policies, invitations, identity linking, groups,
  grants, the admin surface, grant diagnostics, cross-package lineage
  closure with redacted impact.
- The GitHub App credential, acting-credential selection, authoritative
  `decide()`, role-based surface approvals with PR comments,
  console-initiated merge, `Acting-For` attribution, webhooks as nudges.

Gate: **the full Priya walkthrough**, the acceptance test this whole
redesign aims at: a pricing manager with no GitHub account signs in with
SSO, edits through a surface, sees impact with its confidence stated,
submits; an approver approves; the App merges; the audit trail names
everyone. Performed, timed, and repeated as a regression gate thereafter.

### C6. Extensions and experiences

- The extension host and contract (read, propose, UI kit), degradation
  rules, build-time composition.
- The **table** extension, then the **flags** extension (status
  derivation, ring advancement, the rollout dial on Layer 3's allocation
  operations), both through the public contract only.
- The vendorable `console/` lint script; fleet health; the cross-overlay
  comparison matrix (the ring-2 stretch goal).

Gate: contract proof (both experiences built with zero private APIs), plus
a **flag-rollout walkthrough** (dark ship to 10% to 50% to launched, with
the lit-up graph and impact panel consulted along the way).

### C7. Cutover and retirement

Requires the product-shape decision (how a TypeScript server ships inside
or beside the `rototo console` command).

- Parity checklist against the old console for the personas served; close
  the gaps that matter, consciously drop the ones that do not.
- `rototo console` starts the new server; the old Rust console server and
  SPA are deleted (`src/console/`, `apps/console/`), and the console
  feature flag in Cargo.toml is rewired to whatever the product shape
  decision requires.
- Docs updated; the design documents get a final consistency pass against
  what actually shipped.

Gate: the old console is removable with `just check` green, all
walkthroughs green, all budgets green.

## Deliberately in no tranche

Tenants (identity, isolation, self-service), ring 3 estate views, WASM
engine in the browser, dynamic extension loading and marketplaces,
sub-field permissions, production context capture, multi-VCS. Each has its
trigger recorded in its spec; none has a schedule.

## What could reorder this

- If the Phase A/B decision lands early as "A alone ships", C5 slides
  after C6 and the first release is C4 plus C6 for GitHub-holding users.
- If the splice engine surprises us in C0, C1 proceeds anyway (bindings
  and spine do not depend on it) and the engine gets the time it needs;
  C2's form editing is the first thing that would wait.
- If bindings friction is worse than expected in C1, the fallback is
  narrowing the surface (views and lint first, engine apply second), not
  abandoning the boundary.
