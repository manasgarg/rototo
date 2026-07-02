# Demonstration packages (design stage)

These five packages are the demonstration side of `../use-cases.md`: independent,
brand-neutral SaaS scenarios, each owning the use cases it can show best. They
are written in the designed package format (see `../tenant-configuration.md`),
which the engine does not implement yet, so nothing here is gated by `just check`.

They have a defined exit: when an implementation tranche makes a slice of a
package lint and resolve, that slice moves to `examples/` with expected-output
fixtures under `tests/fixtures/`, in the same commit series as the engine change
that enabled it. CI enforces it from that moment. When everything has moved,
this directory is deleted.

| Package | Scenario | Use-case groups |
| --- | --- | --- |
| `release-ops/` | a team shipping a collaborative document editor | 1 release control, 2 experimentation, 3 ops tuning, 9 time-based |
| `billing/` | an API platform selling tiered plans | 4 plans, entitlements, pricing |
| `tenancy-decisioning/` | a site platform whose tenants customize content within governed limits | 5 tenancy, 6 decisioning, 9 campaign windows |
| `regional-policy/` | a messaging SaaS operating across jurisdictions | 7 compliance, 8 provider routing |
| `environments/` | one small service across dev, staging, prod | 10 environment separation, 3 knobs |

Each package README leads with its scenario and closes with a "hard parts"
section: the production complications it demonstrates, and the ones that are
still open design questions (tracked in the roadmap section of
`../use-cases.md`), stated honestly rather than papered over.
