# Addressing test matrix

The address module (`src/address.rs`) is the source of truth for the one
addressing grammar (`design/addressing.md`):
`<class>=<id>[:<class>=<id>]*[#<json-pointer>]`. This file inventories its
promises in the same form as the other matrices. Tests are the inline
suite in `src/address.rs`.

Consumer behavior (what depths a lint target accepts, how `x-rototo-ref`
uses class references) belongs to the consumers' matrices as they port;
these rows are the grammar itself.

## 1. Parsing and rendering

| # | Given / When | Then | Coverage |
|---|---|---|---|
| P1 | every address form in the design doc's example table (singletons, collectives, subtrees, entities, nested entities, pointers, `~1` escapes) | parses and renders back byte-identically: the canonical form round-trips | `every_design_doc_example_round_trips` |
| P2 | `catalog=acme/banner:entry=promo/summer#/message` | splits lexically: two steps with namespaced ids, one pointer; every `/` is namespacing, the `:` is containment | `the_worked_parse_from_the_design_doc` |
| P3 | `variable=payments/rules` | one entity named `payments/rules`: ids own everything after `=`, no reserved words | `the_worked_parse_from_the_design_doc` |
| P4 | a malformed address (missing `=`, unknown class, id on a singleton, `variable=/`, bad id charset, bad pointer syntax, `~` without 0/1) | rejected with the reason named | `malformed_addresses_are_rejected_with_the_reason` |

## 2. Structure rules

| # | Given / When | Then | Coverage |
|---|---|---|---|
| S1 | nesting | `entry=` only under `catalog=`, `sample=` only under `evaluation-context=`, root classes never nest, and the parent step must carry a concrete id | `malformed_addresses_are_rejected_with_the_reason` |
| S2 | depth classification | `package=` is Package; a singleton's empty id is Entity; an empty id elsewhere is Collective; a trailing `/` is Subtree; a concrete id is Entity | `depths_follow_the_acceptance_model` |
| S3 | pointers | only on concrete entities whose class has a document (`package=` and collectives/subtrees reject; `linter=` has no document) | `malformed_addresses_are_rejected_with_the_reason`, `fragment_only_references_resolve_against_an_entity_base` |

## 3. Relative resolution

| # | Given a reference shaped as... | Then... | Coverage |
|---|---|---|---|
| R1 | `#/resolve/default` (fragment-only) | resolves against an entity base with a document; collectives and `linter=` bases reject | `fragment_only_references_resolve_against_an_entity_base` |
| R2 | `welcome#/body` (bare id) | fills a base ending in an open id slot (`catalog=x:entry=`), the generalization of today's entry references; namespaced ids fill slots too; a base with no open slot rejects | `bare_id_references_fill_the_open_slot` |
| R3 | `variable=eu_users` (class-marked) | package-absolute; the base is ignored | `class_marked_references_are_package_absolute` |
| R4 | a malformed reference | rejected at parse | `malformed_references_are_rejected` |

## Current gap tally

0 GAP rows. As consumers port (custom lint targets, the `=` binder
migration, diagnostics rendering), their acceptance rows land in their own
matrices and reference this one for the grammar.
