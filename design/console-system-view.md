# Console system view

Status: decided design note, cross-layer. The four layer specs
(`console-identity-authz.md`, `console-git-ops.md`, `console-semantic.md`,
`console-surfaces.md`) each contribute parts; this note records how they
compose into a system-level view, and the decisions taken. Where a
mechanism is specced elsewhere, this note points rather than repeats.

The model has two axes. **Rings** are zoom: how much of the system you are
looking at. **Facets** are the kind of question you are asking about it.
Every facet exists at every ring, and the editing view is the three facets
differenced.

## The rings (zoom)

- **Ring 0, one entity.** A variable, an entry, an enum.
- **Ring 1, one package.** Everything the package declares.
- **Ring 2, the composition.** A base and its overlays: tenants extending
  a platform package, dev/staging/prod extending an environments base.
- **Ring 3, the estate.** Every package an organization runs, across
  repositories. **Deferred.** If it ever arrives, it is console-consumed
  data (the Layer 4 pattern), not a new concept; nothing below blocks it.

The escalation ladder from Layer 4 (surface, then entity, then source) is
orthogonal to both axes: vocabulary depth, not zoom and not facet. A user
can be at ring 0 through a domain surface or through the workbench.

## The facets

- **Structure**: what exists and how it is wired. The static picture.
- **Execution**: what actually happens for a given context. The running
  picture.
- **Validity**: what lint says. The health picture.

### Structure

**Ring 0.** The entity detail Layer 3 specs: rendered definition, source
with diagnostics, references in both directions, history. Composed
packages add the provenance stack: which package declared it, which
overlay changed what, ending at the value in force (the core's provenance
sidecar already records this; the view renders it).

**Ring 1.** A package home assembled from specced parts: the inventory,
the reference graph, surfaces as domain lenses, upcoming changes, pending
change sets. The work is composition into one screen, not machinery.

**Ring 2.** No new declaration exists or is needed: the `extends` graph is
already in the manifests, and source-tree discovery already finds every
package, so the console infers the composition tree and draws it, each
edge annotated with what the overlay actually changes (which is precisely
what its files are: update markers, deleted markers, added entries).

### Execution

The execution facet is always parameterized by a **chosen context**: a
saved sample or ad-hoc JSON, picked once and carried across rings by the
console. This makes the context picker a first-class UI concept, not a
per-screen form field.

**Ring 0.** The trace preview Layer 3 specs: resolved value, provenance,
and the rule walk for the chosen context.

**Ring 1.** The package graph, lit up: every variable resolved against the
chosen context with traces, rendered on the reference graph. Paths that
fired carry their values; dormant paths dim. This is batch traced
resolution (existing core capability) composed onto the graph the console
already draws, and it is the single best answer to "what does this package
actually do".

**Ring 2.** The same context resolved across sibling overlays, as a
matrix: "log_level for this context: debug in dev, info in staging, warn
in prod", or one variable across twelve tenants. The comparison view is
not a separate feature; it is the execution facet at ring 2.

### Validity

**Ring 0.** Entity-scoped diagnostics, as specced in Layer 3.

**Ring 1.** The package lint report plus the coverage reports the core's
inspect report already carries (sample coverage per variable, dependency
reports, dangling references).

**Ring 2.** Fleet health, the genuinely new cell: every overlay of a base
composed and linted, aggregated ("3 of 12 tenant overlays fail lint
against this base"). This is what makes evolving a base under tenants
safe, and it is lint the console already runs, fanned out per overlay and
summarized.

## Editing: the three facets, differenced

When someone is editing, the system view is the projected consequence of
their change set, one delta per facet. All three are **mandatory fixtures
of change-set review**:

- **Structure delta**: the semantic diff, what changed in rototo terms
  (Layers 2 and 3, specced).
- **Execution delta**: resolution impact. The console re-resolves every
  variable against every sample context, before and after, and shows which
  outcomes changed: "flips checkout_redesign for 2 of 5 sample contexts,
  changes nothing else". Whole-package, because dependencies propagate;
  and per-overlay for every overlay of the edited package in the same
  source tree, because a base edit lands on every tenant. The core already
  computes this shape (`PackageDiff` with resolution impact, what
  `rototo diff --context` prints); the console makes it a review fixture.
- **Validity delta**: diagnostics introduced and diagnostics resolved by
  the change set.

An approver reading those three panels knows what changed, what it does,
and whether it is healthy, in that order. That is an informed decision in
domain terms, which is the point of this whole redesign.

One pre-edit affordance rides the structure facet at ring 0: **blast
radius**. Opening a control or entity editor shows what depends on the
thing: referencing entities (the reference closure, pointed downstream),
surfaces that bind it, overlays that override it.

## One shared home

Decided: one home for every persona, not homes per persona. The home is a
set of lenses, and grants plus surface audience decide what each lens
contains for the signed-in principal:

- **Domain lens**: the surfaces you can see, with effective values and
  pending-change badges.
- **Change lens**: change sets awaiting you as approver, and yours in
  flight.
- **Time lens**: upcoming changes across what you can see.
- **Model lens**: the package map and diagnostics (developers live here; a
  stakeholder simply sees less in it, not a different app).

A pricing manager and a platform developer open the same home; it is
populated differently because their grants and audiences differ, not
because the console branched.

## Mechanics this adds to the layer specs

Small and few:

- The two-pin semantic diff binding (Layer 3 inventory) gains a
  with-contexts variant returning resolution impact.
- Batch traced resolution (all variables, one context) for the lit-up
  graph; the core resolves and traces today, this is a batching surface.
- The composition tree needs `extends` edges surfaced through package
  discovery (the manifests declare them; the semantic surface exposes
  them).
- Fleet health and the validity delta are composition of lint the console
  already runs; provenance rendering reads the existing sidecar.
