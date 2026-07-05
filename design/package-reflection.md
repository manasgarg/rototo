# Package reflection and lookup for apps

Status: landed (2026-07-05) for Rust and all four language SDKs (the v1
slice: discovery plus lookup; the visitor is Rust-only until a non-Rust
consumer appears). Open question 1 resolved yes (resolve_reference_at), 2
resolved no (read_entry stays raw), 3 stays deferred.

## Why

The matrix review (finding 1) settled a boundary: hydration is for
resolution, not for apps. Query filters keep evaluating against hydrated
entry views, but the value an app receives from `resolve_variable` is the
raw entry: if a field holds `"welcome#/body"`, that is what the app sees.
That keeps resolved values method-independent and cheap, and it stops the
SDK from guessing how much of the reference graph an app wants inlined.

The cost is a gap: an app holding a raw reference has no sanctioned way to
follow it, and an app that wants to enumerate what a package offers (all
plans, all enum members, all banners) has no read surface for catalogs and
enums at runtime. Today's answer is "reimplement ref parsing yourself,"
which is exactly the kind of semantic duplication the SDK exists to
prevent.

This design adds the missing surface in three layers: **discovery** (what
is in the package), **lookup** (follow one reference on demand), and a
**visitor** (walk a value with its schema, finding references
generically). Addresses from `design/addressing.md` are the lingua franca
throughout; the reflection API does not invent a second naming scheme.

## The runtime data model

Everything reflects the loaded, flattened package the SDK already holds
(the same staged snapshot resolution reads). Nothing here touches disk or
re-stages sources; refresh swaps the whole reflected view atomically with
the package, so reflection is as consistent as resolution.

## Layer 1: discovery

Rust first, mirroring the existing app-facing read APIs (`list_variables`,
`read_catalog`), completing the entity kinds:

```rust
impl Package {
    // Existing: list_variables, list_catalogs, read_variable, read_catalog.
    pub fn list_enums(&self) -> Vec<EnumSummary>;          // id, description
    pub fn read_enum(&self, id: &str) -> Result<EnumConfig>;
    // EnumConfig { id, description, member_type, members: Vec<JsonValue> }

    pub fn read_entry(&self, catalog: &str, entry: &str) -> Result<JsonValue>;
    pub fn list_entries(&self, catalog: &str) -> Result<Vec<String>>;
}
```

Notes:

- `read_entry` returns the raw entry (no hydration, consistent with the
  finding-1 boundary), with the entry id available to the caller already.
- `list_entries` returns ids, not values: enumerating ten thousand entries
  should not deserialize ten thousand values.
- These extend `CatalogConfig`'s role, not replace it: `read_catalog`
  stays the schema-and-metadata view; `read_entry` is the value view.

## Layer 2: lookup (the core API)

One call that follows a reference the way hydration would have, on demand
and under the app's control:

```rust
impl Package {
    /// Follows one reference value against the schema context it came
    /// from, returning the referenced value (with the pointer applied).
    pub fn resolve_reference(&self, reference: &ValueRef) -> Result<JsonValue>;
}

/// A parsed reference, obtained from the visitor (layer 3) or built
/// directly from an address string.
pub struct ValueRef { /* catalog, entry, pointer */ }

impl ValueRef {
    /// From an address: catalog=email_template:entry=welcome#/body
    pub fn parse(address: &str) -> Result<Self>;
    /// From a raw entry-reference string plus the pinned target class,
    /// mirroring x-rototo-ref semantics ("welcome#/body" against
    /// catalog=email_template, including multi-catalog pins).
    pub fn from_entry_ref(value: &str, pins: &[&str]) -> Result<Self>;
    /// From a dynamic ref object ({catalog, entry, pointer}).
    pub fn from_dynamic(value: &JsonValue) -> Result<Self>;
    pub fn address(&self) -> String;   // canonical address rendering
}
```

Semantics are exactly hydration's, factored out: same entry lookup, same
pointer application, same ambiguity rules for multi-catalog pins, and a
cycle is the caller's problem only in the sense that lookup is one hop;
there is no recursive inlining to cycle through. Errors name the address.

This is the API the billing example uses after finding-1 lands: resolve
`active_plan`, get the raw entry, call
`resolve_reference(ValueRef::from_entry_ref(plan["features"][0], &["features"]))`
for the features it actually renders.

## Layer 3: the visitor

Generic reference discovery for tools that do not know the schema shapes
in advance (the console's entry browser, doc generators, audit scripts):

```rust
impl Package {
    /// Walks a value together with its catalog's schema, yielding every
    /// field whose schema carries x-rototo-ref, as (pointer-into-value,
    /// parsed ValueRef).
    pub fn references_in(&self, catalog: &str, value: &JsonValue)
        -> Result<Vec<(String, ValueRef)>>;
}
```

This reuses the schema walk hydration already does (`$ref` indirection
included, once finding 2's relative-`$ref` fix lands); the difference is
it reports instead of splicing. An app can then hydrate selectively:
`references_in` + `resolve_reference` on the two fields it renders is the
app-side spelling of what old hydration did to everything.

Deferred from v1: a push-style visitor trait (callback per node) and
mutation helpers ("replace each reference with its target"). The
Vec-returning form covers the known consumers; a trait can wrap it later
without breaking anyone.

## Cross-language rollout

Per the SDK policy (thin bindings, same concepts intentionally):

- **v1 (Rust)**: all three layers, plus contract cases for lookup
  (`resolve_reference` on the catalog-refs fixture shapes: plain, pointer,
  multi-catalog, dynamic) and discovery (`read_enum`, `read_entry`).
- **v1 (Python/TS/Go/Java)**: `read_entry`, `read_enum`, `list_enums`,
  `list_entries`, and `resolve_reference` taking an address string or a
  (value, pins) pair; JSON-compatible values in each language's native
  form, errors mapped normally. The visitor waits for a concrete non-Rust
  consumer.
- Contract cases live in `tests/sdk-contract/cases.jsonl` as data, one
  runner arm per language, per the established pattern.

## Consumers to validate against

- The billing example (entitlement following) after finding-1 lands: the
  test asserts the app-side lookup produces what hydration used to.
- The console entry browser: list catalogs, list entries, read one, show
  its references by name with follow-on-click (`references_in` +
  `resolve_reference`).
- The reconciler pattern: set-returning desired state reads entries and
  follows refs explicitly; read-only SDK surface is preserved.

## Open questions

1. Should `resolve_reference` also accept a bare address string overload
   (`resolve_reference_at("catalog=x:entry=y#/ptr")`)? Leaning yes for
   scripting ergonomics; it is one line over `ValueRef::parse`.
2. Should `read_entry` inject the `id` field the way hydration does?
   Leaning no: raw means raw, and the caller passed the id in. The
   query-path `id` injection question belongs to finding-1's task.
3. Namespacing filters on discovery (`list_entries` under a prefix,
   `variable=payments/`-style)? Cheap to add, but wait for a consumer;
   the address module's subtree matching is ready when one appears.
