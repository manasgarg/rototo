# Console identity and authorization (Layer 1)

Status: draft for review. This is the Layer 1 spec for the console
re-implementation: identity, principals, groups, and authorization. It records
the decisions from the design discussion and marks what is deferred. Tenant
users are out of scope here; where a decision would foreclose the tenant story,
this spec notes the hook and moves on. The earlier `design/console.md` is a
whole-console scratch draft and is superseded by per-layer specs as they land.

## 1. Where we start from

The current console has identity but no authorization model of its own:

- Identity is a GitHub credential (OAuth session in hosted mode, ambient token
  in local mode, git-config fallback with no authentication at all).
- The principal is a string derived from the identity (`github:<id>`,
  `git:<hash>`). There is no principals table; sessions are the only durable
  record a user exists.
- Authorization is deployment capability, not person permission: a
  process-wide write policy, per-source capability computation, row ownership
  by principal id, and GitHub itself as the real enforcer, since every write
  runs with the user's own token.

Two properties of the current design are worth keeping on purpose:

- The server recomputes authorization on every mutation; capability data sent
  to the browser is explanation only.
- Credentials are encrypted at rest and never leave the server.

And one property we give up knowingly when non-GitHub users arrive: user-token
writes gave perfect git attribution for free. Any app-credential write path
must reconstruct attribution deliberately.

## 2. Concepts

- **Identity**: a proof mechanism. A record of "this person can complete
  authentication with provider P as subject S". Identities are keyed by the
  provider's stable subject identifier, never by email or login.
- **Principal**: the durable entity that actions are attributed to and
  authorization is decided against. A principal has one or more identities.
  Principals have a `kind`; v1 ships `human` only, and the field exists so
  service principals (API tokens, automation) can arrive later without a
  schema rethink.
- **Acting credential**: what performs an operation against an external
  system. Today: the user's own GitHub token. Later (Layer 2): the console's
  GitHub App credential acting on behalf of a principal. Identity proves who
  asked; the acting credential is who does.
- **Group**: a console-managed set of principals. Groups exist to make grants
  administrable, nothing more.
- **Action** and **resource**: the authorization vocabulary (section 5).
- **Decision point**: the single internal function every route calls to answer
  "may this principal do this action on this resource?".

## 3. Identity

### 3.1 Providers

Exactly three ways to authenticate in the org phase:

1. **GitHub OAuth** (exists today). GitHub is special twice over: it has no
   OIDC login, so it stays a bespoke OAuth2 flow plus a viewer API call, and
   its token is also an acting credential for repository writes. Both roles
   stay, but they become separately modeled: signing in with GitHub proves
   identity and, as a side effect, stores an acting credential on that
   identity link.
2. **Generic OIDC** (new). One implementation covers Okta, Entra ID, Google,
   Auth0, Keycloak, and the rest. Configured at deployment time:
   `ROTOTO_CONSOLE_OIDC_ISSUER`, `ROTOTO_CONSOLE_OIDC_CLIENT_ID`,
   `ROTOTO_CONSOLE_OIDC_CLIENT_SECRET`, plus a display name. v1 supports
   exactly one OIDC provider alongside GitHub; multiple simultaneous OIDC
   providers are deferred until someone needs them.
3. **Local mode** (unchanged). Trust-the-workstation stays its own
   pseudo-provider: ambient token chain (flag/env, stored device-flow
   credentials, `gh auth token`), git-config identity fallback, no login UI.
   Nothing in this spec applies to local mode except the shared principal
   vocabulary; local mode has one implicit principal with every capability.

Org providers are deployment configuration (env vars at startup), consistent
with how hosted OAuth works today. Runtime-configurable providers are a tenant
concern and deferred with tenants.

### 3.2 Identity keys

An identity row is keyed `(provider, subject)`:

- GitHub: the numeric user id (`viewer.id`), as today.
- OIDC: `iss` + `sub`, exactly as asserted in the verified ID token.

Login, email, name, and avatar are stored as refreshable display snapshots and
are never used as keys. Emails are recorded with their verification status;
an unverified email is display data only.

### 3.3 Principals and linking

`principals` becomes a real table: opaque generated id, kind, display name,
status (`active` | `disabled`), timestamps. Identity rows carry a
`principal_id` foreign key. Disabling a principal invalidates all its sessions
and fails every authorization decision, regardless of grants.

Linking rules, in order of importance:

- **Never auto-link by email.** Two providers asserting the same email do not
  become one principal automatically; that is a classic account-takeover
  vector.
- **Explicit link while signed in**: a signed-in principal starts a "link
  identity" flow, completes the other provider's dance, and the new identity
  attaches to the current principal. This is how a developer who signs in
  with SSO attaches their GitHub credential for the workbench write path.
- **Invitation pre-binding**: an invitation (section 3.4) can name the
  expected identity (an email, or a specific provider subject). Redeeming the
  invitation and completing sign-in attaches that identity to the principal
  the invitation created.

### 3.4 Enrollment

Completing authentication must not grant access. Enrollment policy is
deployment configuration with three settings:

- `invite-only` (default for hosted deployments): sign-in succeeds only for
  identities already attached to a principal or matching an open invitation.
  Anyone else gets a "not enrolled" screen, and no principal row is created.
- `domain-allowlist`: an identity whose verified email matches a configured
  domain list auto-enrolls as a new principal with zero grants at first
  sign-in. Useful for orgs that want everyone visible but nothing accessible
  by default. Enrollment happens at sign-in rather than lazily at first
  grant: granting requires a principal to exist, and materializing
  principals from emails later would reintroduce email as a key through the
  back door. The cost is a few zero-grant rows from curious visitors, who
  see an empty console; the benefit is that administrators pick grantees
  from people who have actually signed in.
- `open`: auto-enroll with zero grants. Demo and evaluation deployments only.

Invitations are created by administrators: target email, optional provider
restriction, optional initial group memberships and grants, expiry, and a
single-use token delivered out of band (v1 shows the invite link to the
administrator; email delivery is not the console's job yet).

Bootstrapping: a fresh hosted deployment reads
`ROTOTO_CONSOLE_ADMINS`, a comma-separated list of identity references
(`github:<login>` or `oidc:<email>`). The first sign-in matching an entry
creates that principal with the `administer` grant at deployment scope.
GitHub logins and emails are mutable, so the match is used once at first
sign-in to mint the durable identity row keyed by stable subject; after that
the env var is ignored for that entry.

### 3.5 Sessions and credentials

Session mechanics stay as they are: server-side session rows, opaque cookie,
14-day TTL, `x-rototo-console` header plus Origin allowlist on every mutating
route.

Two changes:

- Sessions reference a principal id that is now a foreign key to a real row.
- Acting credentials move from the session to the identity link. Today the
  hosted session row carries the GitHub token; that dies with the session and
  cannot serve a principal who signs in via OIDC on Monday and needs their
  linked GitHub credential on Tuesday. Instead, the encrypted GitHub token
  (same `token_crypto` format, same `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY`)
  lives on the GitHub identity row and is refreshed whenever that identity
  completes a sign-in or link flow. Sessions prove presence; identities hold
  credentials.

## 4. Authorization: the decision point

One internal function answers every permission question:

```text
decide(subject, action, resource, context) -> Decision
```

- `subject`: a principal id.
- `action`: one of the verbs in section 5.1.
- `resource`: one node of the resource tree in section 5.2.
- `context`: request facts a rule may need. v1 defines one: `author`, the
  proposing principal of the change under review, so two-person policies
  (section 5.1) can be evaluated without a special case in every route.
- `Decision`: allow or deny, plus a machine-readable explanation: which grant,
  derivation, or backend produced the answer. Every allow can say why in one
  sentence.

This is deliberately the AuthZEN request shape (subject, action, resource,
context). We are not adopting the wire protocol, embedding a policy engine,
or taking a dependency; we are shaping our one internal seam so that a
standard evaluator could sit behind it someday without touching call sites.

Rules that hold regardless of backend:

- Default deny. No rule matched means no.
- The server calls `decide` during every mutation; anything sent to the
  browser earlier is explanation, never authority.
- A disabled principal always gets deny.
- Local mode short-circuits to allow (single implicit principal, workstation
  trust), keeping call sites uniform.

### 4.1 Two backends

`decide` is answered by the union of two backends. Either one allowing is an
allow; explanations name the backend.

**Backend A: GitHub-derived (org phase 1).** For subjects with a linked
GitHub identity, permissions are read from GitHub and mapped:

| GitHub fact | Console decision |
| --- | --- |
| repo `pull` permission | `view` on the source tree and everything under it |
| repo `push` permission | `propose` likewise |
| requested reviewer or team on an open pull request (GitHub's own CODEOWNERS evaluation) | `approve` for that proposed change |
| repo `maintain` or `admin` | `approve`, as the prediction before a change is proposed |
| repo `admin` | `administer` on the source tree |

Backend A also reads the target branch's protection rules, because they
carry the repository's change policy, not just its permissions: whether
reviews are required decides whether a proposer may land their own change,
and an unprotected branch plus push permission means propose and approve
collapse into direct application, exactly as `git push` would. One principle
binds this backend: **advisory means never stricter than the authority.** If
GitHub would accept a direct push or a self-merge, the console allows it and
renders it honestly (marking the change as landed without independent
review) rather than refusing; a rule the user can walk around with plain
`git push` is not a rule, only friction.

CODEOWNERS is deliberately not parsed. Once a change is proposed and a pull
request exists, GitHub evaluates ownership itself: requested reviewers and
teams are populated from CODEOWNERS, and the review decision says whether
requirements are met. Approval queues and "who can land this" read that
computed truth off the pull request. Before a proposal exists, `approve` is
predicted coarsely from `maintain` or `admin`; a conservative prediction
there is harmless because GitHub's own answer replaces it the moment the
change is proposed. A hand-rolled CODEOWNERS parser would be a shadow
reimplementation of GitHub semantics that drifts; it gets built only if the
coarse prediction proves misleading in practice.

Facts are fetched with the subject's own token (collaborator permission,
team membership, branch protection, review state on open pull requests) and
cached with a short TTL, around one to five minutes. Staleness is acceptable in this phase because Backend A is
**advisory**: the write itself still runs with the user's token, so GitHub
remains the authority at the moment of the operation. The console's decision
exists to render honest UI (grey the button before the doomed attempt, build
approval queues) and to keep every route on the `decide` seam from day one.

**Backend B: console grants (org phase 2).** For principals GitHub knows
nothing about, and for scopes finer than GitHub can express:

- `grants` table: `(grantee, action, resource)` where grantee is a principal
  or group. Allow-only; there are no explicit deny rules in v1. The only
  deny mechanisms are grant absence and principal disablement, which keeps
  every grant set auditable by reading it top to bottom.
- Grant administration is itself gated: creating or revoking a grant on a
  resource requires `administer` on that resource or above.
- When the acting credential is the console's own (the Layer 2 GitHub App
  path), Backend B is **authoritative**, not advisory: the app credential
  can do anything the App was granted, so the console must enforce the
  decision strictly before acting.

That advisory-versus-authoritative distinction is the load-bearing line in
this design: `decide` is a prediction when your own token acts, and the law
when the console's token acts.

## 5. Vocabulary

### 5.1 Actions

Four verbs, strictly ordered; a grant at a level implies the levels below it
on the same resource:

```text
view < propose < approve < administer
```

- `view`: see the resource exists, read its content, run resolution previews
  against it.
- `propose`: create and edit change branches, commit through the console,
  open a change for review. In a pull-request-policy deployment, editing IS
  proposing; there is no separate edit verb.
- `approve`: approve and land a proposed change. Whether a proposer may land
  their own change is policy, not a hard invariant. Under Backend A the
  policy is the repository's: branch protection requiring review means no
  self-landing, and an unprotected branch means direct application is
  allowed. Under Backend B it is a deployment or scope-level setting,
  defaulting to requiring a second person, because there the console is the
  only enforcement point. The `author` context exists so this policy has
  something to evaluate.
- `administer`: manage grants, groups, invitations, and source registration
  at this scope.

The ordering (approve implies propose, administer implies approve) matches
GitHub's own semantics and keeps v1 simple. Splitting `administer` from
`approve` would prevent nothing: an administrator can always grant
themselves approve, so the split's only real value is making that
escalation an explicit, logged act in `authz_audit` instead of an implied
power. That is a compliance nicety, not a v1 need. If a real need appears
for "may approve but not propose", actions become independent sets; the
grants schema does not care.

### 5.2 Resources

The administrative hierarchy, four levels, grants attach at any of them and
inherit downward:

```text
deployment
  source-tree:<id>
    package:<source-tree-id>/<path>
      entity:<package>/<kind>/<id>
```

Entity-level grants exist in the schema from day one but the v1 admin UI only
needs deployment, source-tree, and package scopes; entity-scoped grants become
load-bearing when Layer 4 surfaces want per-surface approvers. Surfaces will
address entities, so nothing new is needed here beyond an `entity` kind for
surfaces themselves.

Resource ids must survive renames where the underlying thing survives.
Source trees have stable generated ids already. Packages are identified by
path within a source tree; a package move is a new resource and grants do not
follow it silently. That is a known sharp edge, accepted for v1 and flagged by
grant diagnostics (section 6.3) when a grant points at nothing. If durable
package identity is ever needed, it belongs in the package itself: a declared
name in `rototo-package.toml` that travels through git, with grants
referencing the declared id. It does not belong in the console store, and
git-rename heuristics are ruled out either way. That is a package-format
change that must be motivated by more than rename-resilient grants.

### 5.3 Groups

Console-managed only in v1: `groups` and `group_members` tables, administered
by principals holding `administer` at deployment scope. No IdP group import,
no SCIM; when that arrives it should arrive as a mapping ("IdP group X feeds
console group Y"), so grants never reference IdP-native identifiers directly.

## 6. Lineage-aware visibility

Rototo's authorization has two graphs. Grants live on the administrative
hierarchy above. Visibility also flows along the semantic reference graph
(variable to catalog entry, schema to enum, variable to variable), because a
permission model that lets someone change a thing while hiding what the
change affects is unsafe even though nothing leaks.

### 6.1 The derivation rule

Explicit grants define the base sets. One derivation applies on top:

> If a principal may `propose` on an entity, they may `view` every entity
> connected to it in the composed package's reference graph, traversed in
> both directions: upstream (what it depends on, needed to edit with
> understanding) and downstream (what depends on it, needed to judge
> impact).

Derived visibility is `view` only. Nothing about write, approve, or
administer is ever derived. Every derived allow carries its path ("you can
see `active_plan` because it references `plans`, which you may edit") so an
auditor can always answer "why can this person see this?".

Under Backend A this derivation is inert: GitHub view is repo-wide, so the
closure adds nothing. It becomes meaningful exactly when Backend B grants
below-repo view scopes, and it ships with Backend B.

### 6.2 Boundaries

When the closure reaches an entity the principal has no path to view and the
derivation is the only claim, v1 renders **redacted impact**: the count and
kinds of affected entities ("referenced by 12 variables in 2 packages you
cannot view"), not their content. Existence is disclosed; content is not.

Two stricter alternatives are deliberately not defaults but stay available to
later layers: blocking the edit outright (right for a small set of critical
entities, if ever), and requiring approval from someone whose view scope
covers the full closure (the natural fit once tenant overlays make redaction
common; deferred with tenants).

### 6.3 Grant diagnostics

Treat grant configuration the way rototo treats packages: validate it and
report incoherence. The console admin surface lists diagnostics such as:

- a grant whose resource no longer exists (package moved, entity deleted);
- a `propose` grant whose reference closure is majority-redacted, meaning the
  grantee mostly cannot see what they affect;
- an approval requirement no active principal can satisfy;
- a group with grants but no members.

These are console diagnostics with their own listing, not package lint; they
do not mint `rototo/*` rule ids and they never fail a package load.

## 7. Store changes

Schema bumps to v9. The console store is rebuildable state and the existing
migration policy (any non-current version asks for a fresh data directory) is
acceptable pre-stability; no data migration is built.

New and changed tables, sketched:

```text
principals        id, kind, display_name, status, created_at, updated_at
identities        id, principal_id FK, provider, subject, login, email,
                  email_verified, name, avatar_url,
                  credential_ciphertext NULL, created_at, last_seen_at
                  UNIQUE(provider, subject)
sessions          id, principal_id FK, created_at, expires_at
groups            id, name UNIQUE, description, created_at
group_members     group_id FK, principal_id FK
grants            id, grantee_kind (principal|group), grantee_id, action,
                  resource, created_by FK, created_at
invitations       id, email, provider_restriction NULL, initial_groups,
                  initial_grants, token_hash, expires_at, redeemed_by NULL
authz_audit       id, at, actor FK, event (grant.create, grant.revoke,
                  group.*, invitation.*, principal.disable, ...), detail
```

`source_trees` drops per-principal ownership in hosted mode: registration
becomes a deployment-level act gated by `administer`, and visibility is
decided by `decide`, not row ownership. Local mode keeps the current
single-user behavior. `authz_audit` is append-only and covers Layer 1
administrative events; change-level audit (who edited which entity) belongs
to Layer 2.

## 8. API surface

- `GET /api/me` grows: principal, linked identities, enrollment state, and
  per-scope capability summaries (still explanation only).
- `GET/POST /api/auth/oidc/{start,callback}`: OIDC sign-in. Existing GitHub
  and device-flow routes stay.
- `POST /api/auth/link/{provider}/start` plus callback: identity linking for
  the signed-in principal.
- Admin, all gated on `administer` at the relevant scope, all under the
  existing mutation invariant: `/api/admin/principals`, `/api/admin/groups`,
  `/api/admin/grants`, `/api/admin/invitations`.
- Permission explanations ride along on resources they describe (a package
  response says why you can see it); no separate explain endpoint in v1.

## 9. Phasing

**Phase A (spine, GitHub-only).** Principals and identities tables, sessions
rework, credentials on identity rows, `decide` with Backend A, every console
route moved onto the seam, `/api/me` rework. User-visible behavior is almost
unchanged; the deliverable is the spine plus honest capability rendering.
Phase A ships no new admin surface: the existing source-tree screens are
gated on `administer` (registration becomes deployment-level in hosted
mode), and `ROTOTO_CONSOLE_ADMINS` covers bootstrap. Offboarding rides on
GitHub and fails closed: a deprovisioned user's token dies, so every
decision and write fails on live GitHub facts, leaving any surviving
session an authenticated shell over nothing. At most, a read-only list of
principals seen may ride along for operator sanity.

**Phase B (mixed org).** OIDC provider, enrollment policies, invitations,
identity linking, groups, grants, admin surface, grant diagnostics, lineage
closure and redacted impact. Backend B enforcement becomes authoritative
wherever the Layer 2 app-credential write path is used; those two land
together.

The unresolved product decision that separates them: whether the first
shipped org release includes non-GitHub internal stakeholders. If not,
Phase A alone ships and B follows; if so, they ship together.

## 10. Deferred, with hooks

- **Tenants**: runtime-configurable per-tenant identity, tenant binding on
  principals, closure-crossing-tenant-boundary policy, approval-covers-the-
  gap. Hooks: principal `kind` and the boundary rules in 6.2.
- **Service principals / API tokens**: principal `kind` reserves the space.
- **IdP group import / SCIM**: arrives as a mapping onto console groups.
- **Multiple OIDC providers**: config shape allows it later; v1 is one.
- **Custom roles / named permission bundles**: grants are raw
  (grantee, action, resource) triples until the fixed ladder pinches.
- **Policy engine (OpenFGA or similar)**: reconsider only if grant volume or
  list-shaped questions ("everything this principal may see") outgrow
  hand-rolled evaluation; the `decide` seam is the insertion point.

## 11. Resolved positions and revisit triggers

Earlier open questions are resolved into the body above. What remains is
the trigger that should reopen each one:

- **Action ordering** (5.1): reopen when Layer 4 surfaces introduce named
  approver policies, which reference groups directly and shrink the ladder
  to a fallback rule, or if a compliance requirement demands that
  administrator self-escalation be a granted act rather than an implied
  power.
- **Package identity** (5.2): reopen if a declared package name lands in
  `rototo-package.toml` for reasons of its own; grants should then switch
  to it.
- **CODEOWNERS fidelity** (4.1): reopen if the coarse pre-proposal
  prediction (maintain-or-admin approves) misleads users often enough to
  hurt.
- **Enrollment timing** (3.4): no trigger expected; sign-in-time creation
  stands.
- **Phase A admin surface** (9): reopen only if a hosted Phase A deployment
  needs principal disablement before Phase B lands.

One genuinely open product decision remains, owned by phasing (section 9):
whether the first shipped org release includes non-GitHub internal
stakeholders, which decides whether Phase A ships alone or together with B.
