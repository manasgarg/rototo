# tenancy-decisioning

A platform hosts many tenants' sites. The platform team owns the base
experience; each tenant customizes it within limits the platform sets. Two
packages: `base/` (the platform's) and `acme-tenant/` (one tenant's overlay,
extending the base). This pair is the canonical demonstration of rototo's
layering and governance model. Each package lints and resolves on its own:

```sh
rototo lint examples/tenancy-decisioning/base
rototo lint examples/tenancy-decisioning/acme-tenant
rototo resolve examples/tenancy-decisioning/acme-tenant \
  --variable homepage_banner \
  --context '{"visitor": {"id": "v3", "visits": 5, "lifetime_spend": 0}}'
```

Use cases demonstrated (numbers are the use-case groups on the `use-cases` docs page):

| Use case | Where |
| --- | --- |
| 5 Per-tenant overrides | `acme-tenant/` end to end |
| 5 White-label composition: add, patch, tombstone | `acme-tenant/data/catalogs/banners/` |
| 5 Delegated administration | `acme-tenant/governance.toml` narrowing for Acme's own teams |
| 6 Ranked selection | `base/variables/homepage_banner.toml` |
| 6 Audience-conditioned content | `base/data/catalogs/audiences/` |
| 9 Campaign windows | `base/data/catalogs/banners/spring_sale.toml` |

## Every composition mechanism, once

- **Add**: `acme_flash_sale.toml` unions a new banner in.
- **Patch**: `welcome.patch.toml` re-words creative; only the fields the base's
  `update_policy` allows.
- **Tombstone**: `spring_sale.tombstone.toml` removes a base banner for this
  tenant.
- **Override**: `acme-tenant/variables/homepage_banner.toml` replaces the
  `[resolve]` block atomically. It runs over the composed catalog, so Acme's add
  is a candidate and Acme's tombstone is not.
- **Narrowing**: Acme's `governance.toml` revokes delete and restricts update to
  Acme's own banner entries for the layers below it, strictly inside the base's
  grant.

## How audiences work here

An audience is a named, reusable condition over visitor facts, stored as data
bounds (`min_visits`, `max_visits`, `min_lifetime_spend`). Banners reference
audiences through `x-rototo-ref`, so a typo'd audience id is a lint failure,
and the banner query dereferences them at resolve time: `entry.audiences` is
hydrated before the filter runs, so each element is the full audience entry and
an `exists()` comprehension checks its bounds against the visitor.

## What the tests assert

Covered by `tests/examples.rs` and `tests/package_lint.rs`:

- Composing `acme-tenant` over `base` yields: `acme_flash_sale` present,
  `welcome` re-worded, `spring_sale` absent, `default_banner` untouched.
- A returning Acme visitor resolves `homepage_banner` to `acme_flash_sale`
  (priority 95 beats everything); a first-time visitor gets the patched
  `welcome`; in the base, a high-spend visitor gets `loyalty_thanks` and a
  visitor matching no audience falls back to `default_banner`.
- Governance rejections at compose time: a patch touching `priority` fails with
  "governance denies update of field priority on catalog.banners", and a
  tombstone for `default_banner` fails with "governance denies delete of entry
  default_banner on catalog.banners".

## Hard parts

Demonstrated here:

- **Override is wholesale.** Granting `override` hands Acme the entire outcome
  of `homepage_banner`. The base cannot grant "may only change the default";
  that honesty is the point of the binary grant.
- **The ID-list boundary.** Audiences are conditions over visitor facts.
  "These 5,000 visitor ids" is not an audience; that set belongs in the
  application's database, surfaced as a context fact.
- **Ceiling checks are conservative with globs.** The design had Acme granting
  `acme_*` updates to its sub-teams. A glob in a lower layer's allowlist must
  be granted verbatim by the layer above (glob-inside-glob containment is not
  solved), and the base scopes updates with a denylist, so Acme names its
  banners literally in `allowed_entries`.

Simplified against the design:

- **Audience conditions are data bounds, not expressions.** The design gave
  each audience an expression-typed `condition` and a `matches_audience()`
  built-in (roadmap item 12) to dereference and evaluate it. The engine does
  not evaluate expression-typed data, so audiences carry structured bounds and
  the banner filter evaluates them inline with `exists()`. The expressible
  audience shapes are narrower (numeric bounds only) until the built-in lands.

Open design questions this package is waiting on:

- **The compatibility gate** (roadmap item 13). The base team wants to rename a
  banner field or retire an audience. Which changes break Acme's overlay, and
  how does the base find out before releasing rather than after? This is the
  single biggest operational risk of running many overlays.
- **Overlay drift at scale** (use-case 5). With 200 tenants, "how far has each
  overlay diverged" needs tooling (`rototo diff` per overlay), not review memory.
