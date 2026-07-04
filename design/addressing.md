# One addressing grammar for everything in a package

Status: draft for review. Nothing here is implemented yet.

## The problem

rototo already has four ways to point at things, grown independently:

1. `x-rototo-ref: "catalog:acme/banner"` pins a schema field to a catalog.
2. Catalog entry values like `"welcome#/variants/default/subject"` name an
   entry and a JSON pointer inside it, resolved against whatever catalog
   the schema pinned.
3. The schema compiler uses internal URIs like
   `rototo://catalogs/banner.schema.json`.
4. Custom lint targets use path addresses like `/variables/flag/rules/0`.

They are fragments of one idea, but they disagree with each other, and the
lint spelling is broken: an id may contain `/` for namespacing, so
`/variables/acme/in_trial` cannot be told apart from structural segments,
and namespaced entities cannot be targeted at all (matrix review finding
6). Widening that grammar naively would make addresses silently bind to
the wrong thing, which for a lint system is worse than rejection.

This document defines one grammar that all of these become dialects of.
The conceptual hierarchy:

```
package -> entity class -> (namespaced) entity id -> nested entity -> JSON pointer within a document
```

with two extra requirements:

- **Prefixes are addresses too.** You can address the whole package, a
  whole class, a namespace subtree, an entity, or a field. Consumers
  decide which depths they accept.
- **Addresses can be relative.** A bare pointer, or a bare id plus
  pointer, resolves against a base the context supplies. This is what
  entry references already do; the rule just becomes general.

## The grammar

```
address      = entity-path [ "#" json-pointer ]
entity-path  = segment *( "/" segment )
segment      = class ":" [ id-part ]        ; a class marker starts a new step
             | id-part                      ; extends the current step's id
json-pointer = RFC 6901 pointer ("" or "/a/b/0", with ~0 and ~1 escapes)
```

Parsing is purely lexical, which is the whole point:

- Split at the first `#`. Everything after it is a JSON pointer into a
  document. Everything before it is the entity path.
- Walk the entity path segments. A segment containing `:` starts a new
  `(class, id)` step; segments without `:` append to the current step's
  id. This is unambiguous because ids are lowercase snake_case with `/`;
  they can never contain `:` or `#`.
- A step whose id is empty (`variable:`) addresses the class collective.
- A step whose id ends with `/` (`variable:payments/`) addresses the
  namespace subtree under that prefix.

The class vocabulary (singular, matching the existing `catalog:` /
`enum:` idiom in `x-rototo-ref`):

| class | what it holds | singleton? |
|---|---|---|
| `package:` | the whole package | yes (id always empty) |
| `manifest:` | `rototo-package.toml` | yes |
| `governance:` | `governance.toml` | yes |
| `variable:` | `variables/**.toml` | no |
| `catalog:` | `model/catalogs/**.schema.json` | no |
| `entry:` | `data/catalogs/<catalog>/**.toml`, nested under a catalog | no |
| `enum:` | `model/enums/**.toml` | no |
| `members:` | `data/enums/<enum>.toml`, nested under an enum | yes per enum |
| `evaluation-context:` | `model/context/**.schema.json` | no |
| `sample:` | `model/context/<id>-samples/*.json`, nested under a context | no |
| `layer:` | `layers/**.toml` | no |
| `linter:` | `lint/*.lua` | no |

Nesting exists **only** where the child is a separate document: entries
under a catalog, samples under a context, the members file under an enum.
Everything inside one document is addressed with a real JSON pointer, not
a made-up logical segment. In particular, rules stop being pseudo-entities
in addresses: rule 0 of a variable is `#/resolve/rule/0`, the actual TOML
path, not `/rules/0`. One meaning for `#` everywhere.

Reserved characters: `:` and `#` never appear in ids (the
`id-not-snake-case` rule already guarantees the character set). `/` inside
a pointer token uses the standard `~1` escape; ids never need escaping.

Diagnostic rule ids (`rototo/<rule-id>`, `<authority>/<rule-id>`) are a
separate namespace and stay out of this grammar entirely.

## Examples: addressing everything in a package

Take this package:

```
rototo-package.toml                      ([[trace]] policy inside)
governance.toml                          ([variable."payments/max_tokens"] block)
variables/
  checkout_redesign.toml
  payments/max_tokens.toml
  payments/retry_budget.toml
model/catalogs/
  support_banner.schema.json
  acme/banner.schema.json
data/catalogs/
  support_banner/default.toml
  acme/banner/default.toml
  acme/banner/promo/summer.toml          (namespaced entry, post task #60)
model/enums/tier.toml
data/enums/tier.toml
model/context/request.schema.json
model/context/request-samples/premium.json
layers/rollout.toml
lint/budget.lua
```

Absolute (package-relative) addresses, from coarse to fine:

| address | what it points at |
|---|---|
| `package:` | the whole package |
| `manifest:` | the manifest document |
| `manifest:#/trace/0/when` | the first trace policy's `when` expression |
| `governance:` | the governance contract |
| `governance:#/variable/payments~1max_tokens/allowed_operations` | one grant list (note `~1` for the `/` in the id, per RFC 6901) |
| `variable:` | all variables (the collective) |
| `variable:payments/` | the namespace subtree: `payments/max_tokens`, `payments/retry_budget` |
| `variable:checkout_redesign` | one variable |
| `variable:payments/max_tokens` | one namespaced variable (unaddressable today; the motivating case) |
| `variable:payments/max_tokens#/type` | its declared type |
| `variable:payments/max_tokens#/resolve/default` | its default value |
| `variable:payments/max_tokens#/resolve/rule/0` | its first rule (a document pointer, not a `rules/0` pseudo-segment) |
| `variable:payments/max_tokens#/resolve/rule/0/when` | that rule's condition |
| `catalog:` | all catalogs |
| `catalog:support_banner` | one catalog (the schema document) |
| `catalog:support_banner#/properties/message` | a field declaration in the schema |
| `catalog:acme/banner` | a namespaced catalog |
| `catalog:acme/banner/entry:` | all entries of that catalog |
| `catalog:acme/banner/entry:default` | one entry |
| `catalog:acme/banner/entry:promo/summer` | a namespaced entry (works because the id slot owns everything after `entry:`) |
| `catalog:acme/banner/entry:default#/message` | a field of an entry |
| `enum:tier` | the enum declaration (`model/enums/tier.toml`) |
| `enum:tier#/type` | its member type |
| `enum:tier/members:` | the member set (`data/enums/tier.toml`; singleton child, id empty) |
| `enum:tier/members:#/members/1` | the second member |
| `evaluation-context:request` | the context schema |
| `evaluation-context:request#/properties/user/properties/tier` | one declared context path |
| `evaluation-context:request/sample:` | all samples of that context |
| `evaluation-context:request/sample:premium` | one sample |
| `evaluation-context:request/sample:premium#/user/tier` | a value inside the sample |
| `layer:rollout` | one layer |
| `layer:rollout#/allocation/0/arm/1/buckets` | an arm's bucket range |
| `linter:budget` | one Lua lint file (no `#` support: Lua is not a JSON document) |

Worked parses, to show the lexical rule doing its job:

- `catalog:acme/banner/entry:promo/summer#/message` splits at `#`; the
  entity path segments are `catalog:acme`, `banner`, `entry:promo`,
  `summer`. `catalog:` starts step one and collects `acme/banner`;
  `entry:` starts step two and collects `promo/summer`. No reserved words,
  no precedence.
- `variable:payments/rules` is the variable named `payments/rules`,
  full stop. The old ambiguity is gone because "the rules of a variable"
  is now `variable:payments#/resolve/rule`, on the other side of `#`.

## Relative addresses

A reference is resolved against a **base** supplied by the context, RFC
3986 style. Three reference shapes, from most to least bound:

| reference shape | example | resolves against | result |
|---|---|---|---|
| fragment-only | `#/resolve/default` | a base entity | that field of the base entity |
| bare id (+ fragment) | `welcome#/body` | a base ending in an open class slot | the id fills the slot |
| class-marked path | `variable:eu_users` | the package | package-absolute, base ignored |

Where the bases come from in practice:

- **Entry references in catalog values.** A schema field pinned with
  `x-rototo-ref: "catalog:email_template"` gives the value string the
  base `catalog:email_template/entry:` (a path ending in an open id
  slot). The value `welcome#/body` fills the slot:
  `catalog:email_template/entry:welcome#/body`. This is exactly today's
  behavior, restated as the general rule.
- **Custom lint handlers.** A rule targeted at
  `variable:payments/max_tokens` can report a diagnostic location as
  `#/resolve/rule/1/value`; it resolves against the target. A rule
  targeted at the subtree `variable:payments/` receives each member as
  its base in turn.
- **Sample checks.** A sample validates against its context; inside that
  check, `#/user/tier` is a location in the sample document.

## Prefix acceptance per consumer

The grammar is shared; what depth an address may stop at is per consumer.

| consumer | accepts | notes |
|---|---|---|
| custom lint `target` | `package:`, class collectives, namespace subtrees, entities, nested entities; optionally entity `#` pointer for field-scoped rules | collectives and subtrees fan out, one handler invocation per member. Replaces the whole `/variables/...` grammar; old spellings get a rejection message that shows the new form |
| `x-rototo-ref` | class only (`catalog:acme/banner`, `enum:tier`, or an array of catalog classes) | unchanged semantics, already conformant. Dynamic `{catalog, entry, pointer}` objects could later accept a single address string |
| entry reference values | bare id + fragment, against the pinned class | unchanged semantics (`welcome#/body`) |
| governance targets | entities and namespace subtrees (`variable:payments/`) | today's `[variable."payments/max_tokens"]` TOML keys stay valid; addresses become the string form when policies need subtrees |
| diagnostics (`target` in JSON output) | full entity address `#` field pointer, as the canonical serialization of `SemanticTarget` | today's structured object stays; the string form is additive |
| CLI selectors | entity ids per flag today (`--variable payments/max_tokens`); a future `--target <address>` takes any prefix | no change required |

## What this deliberately does not do

- **No package identity yet.** Every address above is package-relative.
  The hierarchy's top slot is reserved: when packages grow an identity
  (registry, cross-package refs, the reconciler work), the full form can
  become a URI (`rototo://<package>/<address>`), and the `rototo://`
  schema-compiler URIs are the precedent. Nothing in the grammar needs to
  change for that; a package-qualified address is an authority prefix on
  the same path.
- **No namespace entities.** `variable:payments/` selects a subtree; it
  does not name an object. Namespaces stay id prefixes.
- **No logical pointer segments.** If it is after `#`, it is an RFC 6901
  pointer into the addressed document. `/rules/0`-style pseudo-paths are
  gone.

## Migration

One parser module becomes the source of truth (grammar, resolution,
canonical rendering), then consumers port one at a time:

1. Parser + resolver + canonical form, with exhaustive round-trip tests
   (this is also where the matrix rows land).
2. Custom lint targets (fixes finding 6; supersedes the earlier `#`-only
   patch idea). Old spellings rejected with the new spelling in the
   message. Drop the dead `values` forms as part of this (task #59 folds
   in or lands first).
3. Recursive entry/sample discovery (task #60). Note the grammar answers
   #60's ownership wrinkle: `data/catalogs/a/b/c.toml` is ambiguous on
   disk between catalog `a` (entry `b/c`) and catalog `a/b` (entry `c`),
   and no address grammar can fix a filesystem ambiguity. Recommendation:
   make it a lint error for one catalog id to be a path prefix of
   another, which keeps the disk layout bijective with addresses.
4. Diagnostics: render `SemanticTarget` in the canonical form alongside
   the structured object.
5. `x-rototo-ref` and entry references: no behavior change, re-specified
   as dialects; dynamic ref objects optionally accept an address string.
6. Governance subtree grants, if and when wanted.

Breaking changes land before any stability commitment, per project
policy: no compatibility shims, loud rejections with the new spelling.

## Open questions

1. **Enum members spelling.** `enum:tier/members:` (singleton child
   class) is consistent but reads oddly. Alternative: fold members into
   the declaration entity and give fragments two roots
   (`enum:tier#/members/...` reaching into the data file), which breaks
   the "one document per `#`" rule. The singleton child keeps the rule
   clean; the spelling is the price. Leaning: keep `members:`.
2. **Collective vs subtree for singletons.** `variable:` (collective) and
   `variable:payments/` (subtree) are distinct forms; should `variable:/`
   be valid? Proposal: no, reject it; the empty id is the collective and
   a trailing slash requires a non-empty prefix.
3. **Field-scoped lint targets** (`variable:x#/resolve/default` as a
   registration target): include in step 2 or defer? Deferring keeps step
   2 a pure re-spelling; including it is the first genuinely new
   capability. Leaning: defer, the grammar already supports it.
4. **Layer allocations and arms** have ids in their tables
   (`[[allocation]] id = "cta_copy"`). They stay document pointers here
   (`layer:rollout#/allocation/0`), positional like rules. If positional
   addressing proves too brittle for governance of layers, an
   id-keyed pointer convention would need designing; out of scope now.
