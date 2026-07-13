# Console lifecycle contracts

Status: decided design note. This note records the full lifecycle every
console-visible object must support: how it comes to exist, how it changes,
and how it goes away. It came out of a lifecycle audit that found the stack
bottom-heavy: the edit engine supports create and delete for every package
entity, the API passes those through, but the UI exposed only a sliver, and
several coordination objects could be created but never changed or removed.
The contracts below are written as given/when/then scenarios so each gap is
checkable. Where a mechanism is specced elsewhere, this note points rather
than repeats.

There are two planes, and they have deliberately different write paths:

- **Package entities** (variables, catalogs, entries, lists, evaluation
  contexts, samples, layers, files, and surfaces, which are entries of the
  `console/surfaces` catalog) live in git. Their lifecycle runs through
  change-set edit operations (`design/console-semantic.md`), compiled by the
  Rust engine into file changes on a branch, reviewed and merged as a pull
  request. Nothing on this plane mutates git directly.
- **Coordination objects** (source trees, change sets, collaborators,
  approvals, principals, identities, sessions, groups, grants, invitations)
  live in the SQLite store. Their lifecycle runs through `decide()`-gated
  routes (`design/console-identity-authz.md`). The store is rebuildable from
  GitHub, so these lifecycles favor simplicity over preservation: the only
  data that must survive is what GitHub cannot reconstruct.

## Package plane

Shared preconditions for every scenario in this section:

- Given a change set in `draft` that the caller may edit, when an operation
  lands, then the engine compiles it to file changes plus a change record,
  and the change set's preview lints the result. Operations never target
  merged, abandoned, or proposed change sets.
- Given an entity the package inherits from a base layer, when any operation
  targets it, then the console refuses. Ownership-aware compilation to
  `.update.toml` / `.deleted.toml` overlay markers is deferred with a
  recorded trigger in `design/console-implementation-plan.md`; hard refusal
  is the contract until that lands.
- Given an operation whose id is not lowercase snake_case (with optional `/`
  namespacing), when it lands, then the engine rejects it before writing.

### Variable

- Given a package, when `create_variable` lands with an id, a type, and a
  default, then a new `variables/<id>.toml` exists with `schema_version = 1`
  and a `[resolve]` block, and the file lints clean on its own.
- Given a variable, when `set_description`, `set_type`, `set_default`,
  `add_rule`, `update_rule`, `remove_rule`, or `move_rule` lands, then the
  file changes splice at model locations and preserve unrelated formatting.
- Given a variable resolved by rules, when `set_query` lands with `from` and
  `filter` (and optional `sort`, `order`, `limit`), then the resolve method
  becomes `query`, the query fields land under `[resolve]`, and any existing
  rules are removed, since a query resolve has no rules to run.
- Given a variable resolved by query, when `clear_query` lands, then the
  resolve method returns to rules with the default intact.
- Given a variable that other variables reference through
  `variables["<id>"]`, when `delete variable=<id>` lands, then the file is
  gone and lint reports `rototo/variable-rule-unknown-variable` at each
  stale reference. The console shows that blast radius, from the model's
  reference index, before it emits the delete.

### Catalog

- Given a package, when `create_catalog` lands with an id and a JSON Schema,
  then `model/catalogs/<id>.schema.json` exists. The engine takes the schema
  as given; the console supplies a small starter schema (an object with
  `additionalProperties` open) so a new catalog is immediately usable.
- Given a catalog, when its schema must change, then the change is a raw
  file edit of the `.schema.json`. Form-based schema editing is a non-goal
  (see decisions below).
- Given a catalog with entries, when `delete catalog=<id>` lands, then the
  schema file and every entry under `data/catalogs/<id>/` are gone, and any
  variable or `x-rototo-ref` pointing at the catalog surfaces as a lint
  failure in the preview. The console shows that blast radius first.

### Catalog entry

- Given a catalog, when `create_entry` lands with a key and a fields object,
  then `data/catalogs/<catalog>/<key>.toml` exists and validates against the
  catalog schema at lint.
- Given an entry, when `set_field` or `unset_field` lands with a JSON
  pointer, then the named field changes in place.
- Given an entry that other entries or variables reference, when
  `delete catalog=<id>:entry=<key>` lands, then the file is gone and stale
  references surface as lint failures in the preview.

### List

- Given a package, when `create_list` lands with an id, a member type, and a
  non-empty member array, then `lists/<id>.toml` exists.
- Given a list, when `add_member`, `remove_member`, or `set_description`
  lands, then members change in place. A list's `type` is fixed at creation:
  changing it would silently invalidate every member and every value checked
  against the list, so the honest path is delete and recreate.
- Given a list referenced by variables (`list=<id>` types, `lists.<id>` in
  expressions) or schemas (`x-rototo-ref`), when `delete list=<id>` lands,
  then stale references surface as lint failures in the preview, and the
  console shows the blast radius first.

### Evaluation context

- Given a package, when `create_context` lands with an id, then
  `model/context/<id>.schema.json` exists along with one starter sample, so
  resolution and fixtures have something to run against from the start.
- Given a context, when its schema must change, then the change is a raw
  file edit, same as catalog schemas.
- Given a context with samples, when `delete evaluation-context=<id>` lands,
  then the schema and its samples directory are gone.

### Sample

- Given a context, when `create_sample` lands with a key and content, then
  `model/context/<id>-samples/<key>.json` exists and validates against the
  context schema at lint. Promoting a traced or ad-hoc context from the
  resolve preview emits this same operation; there is no separate path.
- Given a sample, when `replace_sample` lands, then the content is replaced
  whole. Samples are small JSON documents; field-level splicing buys
  nothing.
- Given a sample, when `delete evaluation-context=<id>:sample=<key>` lands,
  then the file is gone.

### Layer

- Given a package, when `create_layer` lands, then `layers/<id>.toml`
  exists.
- Given a layer, when `add_allocation`, `remove_allocation`,
  `set_allocation_status`, `set_allocation_eligibility`, or
  `set_arm_buckets` lands, then the allocation changes in place. These are
  the operational verbs a rollout needs day to day, so the UI exposes them
  where the rollout is monitored, not only in the workbench.
- Given a layer, when `delete layer=<id>` lands, then the file is gone.

### File

- Given a package, when a raw file is created, edited, or deleted through
  the files view, then the change lands in the same change set mechanism as
  structured operations, with no change record beyond the file diff. Raw
  file editing is the escape hatch that keeps the console honest: anything
  the structured operations cannot express is still a reviewable edit.
- Given the package manifest (`rototo-package.toml`), when a delete is
  requested, then the console refuses: a package without a manifest is not
  a package.

### Surface

- Given that surfaces are entries of the `console/surfaces` catalog, when a
  surface is created, changed, or deleted, then the entry lifecycle above
  applies verbatim, addressed as `catalog=console/surfaces:entry=<id>`. The
  console configures itself with rototo; surfaces get no special write
  path, only a dedicated form.

## Coordination plane

Shared preconditions for every scenario in this section:

- Given any mutating route, when a request arrives, then it carries the
  `x-rototo-console` header and passes the Origin check, and the caller's
  subject passes `decide()` for the named capability. The webhook endpoint
  remains the sole exception.

### Source tree

- Given an `administer` grant on the deployment, when a registration lands
  with `{ kind: "github", owner, name }`, then the tree exists and its
  default branch is filled from GitHub when omitted.
- Given a registered tree, when an update lands, then only the default
  branch can change. Owner and name are the tree's identity; a renamed
  repository is a new registration.
- Given a tree with change sets in `draft` or `proposed`, when a deregister
  lands, then the console refuses and names the open change sets. Given no
  open change sets, when a deregister lands, then the tree's status becomes
  `deregistered`: it disappears from listings and rejects new change sets,
  but its row and its merged history stay for audit. The store is
  rebuildable, so a hard delete would not even stay deleted after a rebuild
  from GitHub; a soft status is the only marker that survives honestly.
- Registration, update, and deregistration are available in the admin UI,
  not only over the API.

### Change set

- Given a tree a subject can view, when a change-set create lands, then a
  `draft` exists with the creator as author.
- Given a `draft`, when edits land, then they append operations; when
  `submit` lands, then the state becomes `proposed` and a branch plus pull
  request exist through the GitHub API.
- Given a `draft` or `proposed` change set, when the author or a
  collaborator edits the title, then it changes in place, and the PR title
  follows on the next reconcile.
- Given a `draft` or `proposed` change set, when the author or a
  collaborator adds or removes a collaborator, then the collaborator set
  changes. Removing a collaborator does not touch edits they already made;
  those are history.
- Given a `proposed` change set, when an approval lands, then it is
  recorded (advisory when a user token acts, authoritative when the console
  grant acts). Given an approval, when its author withdraws it while the
  change set is still `proposed`, then it is gone; approvals on merged or
  abandoned change sets are frozen.
- Given a `proposed` change set, when `merge` lands and the checks decide()
  requires pass, then the state becomes `merged`. Given a `draft` or
  `proposed` change set, when `abandon` lands, then the state becomes
  `abandoned` and the branch and PR close.
- Given a `merged` or `abandoned` change set, when any mutation arrives,
  then the console refuses: both states are terminal, and reconcile is the
  only thing that still touches the row.

### Principal

- Given the enrollment policy admits a sign-in, when a person first signs
  in, then a principal exists with their identity linked.
- Given a principal, when an admin disables it, then its sessions are gone
  and sign-in is refused; when an admin re-enables it, then sign-in works
  again.
- Principals are never deleted. Approvals, edits, and grants point at
  principals, and that audit trail must keep naming the person after they
  leave. Disable is the terminal state.

### Identity

- Given a principal, when a sign-in through a new provider matches
  enrollment, then the identity links to the principal.
- Given a principal with more than one identity, when an admin unlinks one,
  then it is gone and that provider no longer signs in as this principal.
  Given a principal's last identity, when an unlink is requested, then the
  console refuses: a principal with no identity can never sign in again,
  which is a disable wearing a disguise. Disable the principal instead.

### Session

- Given a sign-in, when it completes, then a session exists; when the
  person logs out or the principal is disabled, then it is gone. There is
  no admin session browser (see non-goals).

### Group

- Given an `administer` grant, when a group create lands, then the group
  exists; when a rename or description edit lands, then it changes in
  place. Group names are labels, not addresses, so rename is safe here in a
  way it is not for package ids.
- Given a group, when member adds and removes land, then membership changes
  in place.
- Given a group no grant references, when a delete lands, then the group is
  gone. Given a group a grant still references, when a delete lands, then
  the console refuses and names the grants: deleting it would silently
  widen or narrow access, and access changes should only ever happen
  through explicit grant operations.

### Grant

- Given an `administer` grant, when a grant create lands, then it exists;
  when a revoke lands, then it is marked revoked and stops influencing
  `decide()` immediately.
- Grants have no edit. A grant is a small auditable fact, and editing one
  in place would rewrite that fact. Changing access is revoke plus create,
  which leaves both the old and the new decision on the record.

### Invitation

- Given invite-only enrollment, when an invitation create lands, then a
  pending invitation exists for the email.
- Given a pending invitation, when the invitee signs in with a matching
  email, then it is redeemed and a principal exists.
- Given a pending invitation, when a revoke lands, then it is hard-deleted:
  nothing else references a pending invitation, so there is nothing to
  audit. Given a redeemed invitation, when a revoke lands, then the console
  refuses: the principal already exists, and the lever for that is
  disabling the principal.

## Decisions

- **No rename operations on the package plane.** Package-entity ids are
  addresses: they appear in expressions, `x-rototo-ref` targets, other
  packages' overlays, and git history. A rename operation would imply the
  console can chase all of those, and it cannot. Rename is delete plus
  create in the same change set, with the blast radius visible. Group
  names are the deliberate exception on the coordination plane, because
  nothing addresses a group by name.
- **Query is a resolve method, not a rule field.** A variable resolves
  either by rules or by one query (`[resolve] method = "query"` with
  `from`, `filter`, and optional `sort`, `order`, `limit`). The operation
  vocabulary in `design/console-semantic.md` predates this and describes a
  query parameter on `add_rule`/`update_rule`; that is stale. The
  structured operations are `set_query` and `clear_query` on the whole
  resolve.
- **Deletes are engine-level; lint owns referential integrity.** `delete`
  removes the entity's files. Dangling references are lint failures in the
  change-set preview, not engine refusals, so a delete and its reference
  fixes can land in one change set in any order. The console's job is to
  show the blast radius before emitting the delete, not to block it.
- **Store schema changes ask for a fresh data directory.** Coordination
  lifecycles may add columns (source tree status). The store refuses a
  non-current schema version and asks for a fresh directory; it rebuilds
  from GitHub, so this stays cheap and migration machinery stays out.

## Non-goals

- **Form-based schema editing.** Catalog and context schemas are JSON
  Schema documents; a form that edits arbitrary JSON Schema is a schema
  editor product. Raw file editing through the files view is the contract.
- **Inherited-entity overlay editing.** Deferred with its trigger recorded
  in `design/console-implementation-plan.md`; hard refusal until then.
- **Admin session management.** Sessions are short-lived and die with
  logout or principal disable. A session browser adds surface without a
  failure mode it prevents.
- **Change-set reassignment.** The author is a fact about who opened it.
  Collaborators cover shared work.
