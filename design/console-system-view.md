# Console system view

Status: decided design note, cross-layer. The four layer specs
(`console-identity-authz.md`, `console-git-ops.md`, `console-semantic.md`,
`console-surfaces.md`) each contribute parts; this note records how they
compose into a system-level view for understanding and for editing, and the
decisions taken. Where a mechanism is specced elsewhere, this note points
rather than repeats.

## The four rings

The system view is a zoom stack. Each ring answers "what is this and what
is happening to it" at one altitude:

- **Ring 0, one entity.** A variable, an entry, an enum.
- **Ring 1, one package.** Everything the package declares.
- **Ring 2, the composition.** A base and its overlays: tenants extending a
  platform package, dev/staging/prod extending an environments base.
- **Ring 3, the estate.** Every package an organization runs, across
  repositories, plus the applications consuming them. **Deferred.** If it
  ever arrives, it is console-consumed data (the Layer 4 pattern), not a
  new concept; nothing below blocks it.

The escalation ladder from Layer 4 (surface, then entity, then source) is
the orthogonal axis: vocabulary depth, not zoom. A user can be at ring 0
through a domain surface or through the workbench; the ring is the same.

## Understanding mode

**Ring 0.** The entity detail Layer 3 specs: rendered definition, source
with diagnostics, references in both directions, trace previews, history.
Composed packages add the provenance stack: which package declared it,
which overlay changed what, ending at the value in force (the core's
provenance sidecar already records this for resolve; the view renders it).

**Ring 1.** A package home assembled from specced parts: the inventory,
the reference graph, surfaces as domain lenses, upcoming changes, pending
change sets, diagnostics. The work here is composition into one screen,
not machinery.

**Ring 2.** No new declaration exists or is needed: the `extends` graph is
already in the packages, and source-tree discovery already finds every
package, so the console infers the composition tree and draws it. Base at
the root, overlays as children, each edge annotated with what the overlay
actually changes, which is precisely what its files are (update markers,
deleted markers, added entries). Two views fall out:

- **Provenance view** (ring 0 seen from ring 2): one entity's stack across
  the composition.
- **Comparison view**: one entity or one surface across sibling overlays,
  as a matrix ("log_level: debug in dev, info in staging, warn in prod"),
  computed with the core's existing diff machinery pairwise. This is the
  stretch goal of ring 2; the tree and provenance views are the floor.

## Editing mode

When someone is editing, the system view is the projected consequence of
their change set, in three panels of increasing depth:

**Ring 0, blast radius, before you type.** Opening a control or an entity
editor shows what depends on this thing: referencing entities (reference
closure, the same machinery as Layer 1's lineage rule, pointed
downstream), surfaces that bind it, and overlays in the source tree that
override it.

**Ring 1, semantic diff, as you go.** What the change set changes in
rototo terms; Layer 2 and 3's two-pin semantic diff, already specced.

**Ring 2, resolution impact, before you submit.** Decided: this is a
**mandatory panel of change-set review**, not an optional tool. The
console re-resolves every variable against every sample context, before
and after the change set, and shows which outcomes changed: "flips
checkout_redesign for 2 of 5 sample contexts, changes nothing else", or
the alarming version an approver needs to see. Whole-package, because
dependencies propagate through variable references; and per-overlay for
every overlay of the edited package discovered in the same source tree,
because a base edit lands on every tenant. The core already computes this
shape (`PackageDiff` with resolution impact, what `rototo diff --context`
prints); the console makes it a review-time fixture. An approver reading
"outcome changes: none" versus "outcome changes: 14 across 3 tenants" is
making an informed decision in domain terms, which is the point of this
whole redesign.

## One shared home

Decided: one home for every persona, not homes per persona. The home is a
set of lenses over the rings, and grants plus surface audience decide what
each lens contains for the signed-in principal:

- **Domain lens**: the surfaces you can see, with effective values and
  pending-change badges.
- **Change lens**: change sets awaiting you (as approver) and yours in
  flight.
- **Time lens**: upcoming changes across what you can see.
- **Model lens**: the package map and diagnostics (developers will live
  here; a stakeholder simply sees less in it, not a different app).

A pricing manager and a platform developer open the same home; it is
populated differently because their grants and audiences differ, not
because the console branched.

## Mechanics this adds to the layer specs

Small and few:

- The two-pin semantic diff binding (Layer 3 inventory) gains a
  with-contexts variant returning resolution impact.
- The composition tree needs `extends` edges surfaced through package
  discovery (the manifests already declare them; the semantic surface just
  exposes them).
- Provenance rendering reads the existing sidecar; no new computation.

Everything else in this note is composition of already-specced parts.
