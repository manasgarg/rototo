# tenancy-decisioning

A platform hosts many tenants' sites. The platform team owns the base
experience; each tenant customizes it within limits the platform sets. Two
packages: `base/` (the platform's) and `acme-tenant/` (one tenant's overlay,
extending the base). This pair is the canonical demonstration of rototo's
layering and governance model.

Use cases demonstrated (numbers refer to `../../use-cases.md`):

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
  `acme_*` entries for the layers below it, strictly inside the base's grant.

## What a test would assert (once the model is implemented)

- Composing `acme-tenant` over `base` yields: `acme_flash_sale` present,
  `welcome` re-worded, `spring_sale` absent, `default_banner` untouched.
- A returning high-spend visitor resolves `homepage_banner` to
  `acme_flash_sale` (priority 95 beats `loyalty_thanks` at 80); a first-time
  visitor gets the patched `welcome`.
- Governance rejections at compose time: a patch touching `priority`, any edit
  to `default_banner`, a tombstone from one of Acme's sub-teams (Acme revoked
  delete), and an Acme sub-team grant wider than Acme's own ceiling.
- The resolution trace attributes the value to the Acme layer's `[resolve]`
  block and names the package version of each layer.

## Hard parts

Demonstrated here:

- **Override is wholesale.** Granting `override` hands Acme the entire outcome
  of `homepage_banner`. The base cannot grant "may only change the default";
  that honesty is the point of the binary grant.
- **The ID-list boundary.** Audiences are conditions over visitor facts.
  "These 5,000 visitor ids" is not an audience; that set belongs in the
  application's database, surfaced as a context fact.

Open design questions this package is waiting on:

- **The compatibility gate** (roadmap item 13). The base team wants to rename a
  banner field or retire an audience. Which changes break Acme's overlay, and
  how does the base find out before releasing rather than after? This is the
  single biggest operational risk of running many overlays.
- **Overlay drift at scale** (use-case 5). With 200 tenants, "how far has each
  overlay diverged" needs tooling (`rototo diff` per overlay), not review memory.
- **The one-hop dereference built-in** (roadmap item 12). `matches_audience` is
  used here with its intended semantics; its exact name and signature are not
  yet pinned.
