//! Direct tests for the semantic index: the data structure every lint stage,
//! symbol provider, and inspection view reads from. These tests pin the
//! index's own promises (discovery, isolation, locations, targets) rather
//! than any single consumer's behavior. The row inventory lives in
//! `tests/docs/semantic-index-matrix.md`.

use std::path::Path;

use crate::diagnostics::SemanticEntity;
use crate::lint::input::LintInput;
use crate::lint::{PackageLintSnapshot, lint_package_snapshot};

use super::*;

async fn scratch_snapshot(files: &[(&str, &str)]) -> (tempfile::TempDir, PackageLintSnapshot) {
    let tempdir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let full = tempdir.path().join(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&full, text).await.unwrap();
    }
    let snapshot = lint_package_snapshot(LintInput::new(tempdir.path().to_path_buf()))
        .await
        .unwrap();
    (tempdir, snapshot)
}

/// One file of every kind the package format defines, including namespaced
/// ids. Each must land as exactly one node in the right index map.
fn full_package() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "rototo-package.toml",
            r#"schema_version = 1

[[trace]]
when = 'env.resolving.variable == "flag"'
"#,
        ),
        (
            "variables/flag.toml",
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.user.tier == \"premium\""
value = true
"#,
        ),
        (
            "variables/acme/in_trial.toml",
            r#"schema_version = 1
type = "bool"

[resolve]
default = false
"#,
        ),
        (
            "lists/tier.toml",
            r#"schema_version = 1
type = "string"
members = ["basic", "premium"]
"#,
        ),
        (
            "lists/acme/plan.toml",
            r#"schema_version = 1
type = "string"
members = ["trial"]
"#,
        ),
        (
            "model/catalogs/banner.schema.json",
            r#"{
  "type": "object",
  "properties": { "message": { "type": "string" } },
  "required": ["message"],
  "additionalProperties": false
}"#,
        ),
        ("data/catalogs/banner/default.toml", "message = \"hello\"\n"),
        (
            "data/catalogs/banner/promo/summer.toml",
            "message = \"sunny\"\n",
        ),
        (
            "model/context/request.schema.json",
            r#"{
  "type": "object",
  "properties": {
    "user": {
      "type": "object",
      "properties": { "tier": { "type": "string" }, "id": { "type": "string" } }
    }
  }
}"#,
        ),
        (
            "model/context/request-samples/basic.json",
            r#"{ "user": { "tier": "premium", "id": "user-1" } }"#,
        ),
        (
            "model/context/request-samples/eu/premium.json",
            r#"{ "user": { "tier": "premium", "id": "user-2" } }"#,
        ),
        (
            "layers/rollout.toml",
            r#"schema_version = 1
unit = "context.user.id"
buckets = 100

[[allocation]]
id = "cta_copy"

[[allocation.arm]]
name = "control"
buckets = "0-49"

[[allocation.arm]]
name = "treatment"
buckets = "50-99"
"#,
        ),
        (
            "governance.toml",
            r#"[variable.flag]
allowed_operations = ["update"]
"#,
        ),
        (
            "lint/checks.lua",
            r#"function register(lint)
  lint:rule({
    id = "acme/flag-check",
    title = "Flag check",
    help = "Test rule.",
    target = "variable=flag",
    handler = "check_flag",
  })
end

function check_flag(package, target)
  return {}
end
"#,
        ),
    ]
}

#[tokio::test]
async fn every_package_file_kind_projects_to_exactly_one_node() {
    let (_tempdir, snapshot) = scratch_snapshot(&full_package()).await;
    let index = &snapshot.index;

    let manifest = index.manifest.as_ref().expect("manifest node");
    assert_eq!(manifest.trace.len(), 1);
    assert!(matches!(
        manifest.extends,
        PackageExtendsCollection::Missing
    ));
    assert_eq!(
        manifest.target().entity,
        crate::diagnostics::SemanticEntity::Manifest
    );

    let variable_ids = index.variables.keys().cloned().collect::<Vec<_>>();
    assert_eq!(variable_ids, vec!["acme/in_trial", "flag"]);
    let flag = &index.variables["flag"];
    assert_eq!(flag.id, "flag");
    assert!(matches!(
        flag.target().entity,
        SemanticEntity::Variable { ref id } if id == "flag"
    ));
    assert!(matches!(&flag.type_source, TypeSourceNode::Primitive(name) if name.value == "bool"));
    match &flag.resolve {
        ResolveNode::Resolve { rules, .. } => match rules {
            RuleCollection::Rules(rules) => assert_eq!(rules.len(), 1),
            RuleCollection::Invalid { .. } => panic!("rules should project"),
        },
        _ => panic!("resolve should project"),
    }

    assert_eq!(
        index.lists.keys().cloned().collect::<Vec<_>>(),
        vec!["acme/plan", "tier"]
    );
    match &index.lists["tier"].members {
        ProjectField::Present(members) => assert_eq!(members.value.len(), 2),
        _ => panic!("tier members should project"),
    }

    let banner = index.catalogs.get("banner").expect("banner catalog node");
    assert!(banner.json.is_some());
    assert!(banner.validator.is_some());
    assert!(banner.invalid_message.is_none());
    let entries = index
        .catalog_entries
        .get("banner")
        .expect("banner entries map");
    assert_eq!(
        entries.keys().cloned().collect::<Vec<_>>(),
        vec!["default", "promo/summer"]
    );
    assert_eq!(entries["default"].catalog_id, "banner");
    assert_eq!(entries["default"].key, "default");
    assert_eq!(entries["promo/summer"].key, "promo/summer");

    let request = index
        .evaluation_contexts
        .get("request")
        .expect("request context node");
    assert!(request.validator.is_some());
    let samples = index
        .evaluation_context_samples
        .get("request")
        .expect("request samples map");
    assert_eq!(
        samples.keys().cloned().collect::<Vec<_>>(),
        vec!["basic", "eu/premium"]
    );
    assert!(samples["basic"].value.is_some());
    assert_eq!(
        samples["eu/premium"].location.path,
        "model/context/request-samples/eu/premium.json"
    );

    let rollout = index.layers.get("rollout").expect("rollout layer node");
    assert_eq!(rollout.allocations.len(), 1);
    assert_eq!(rollout.allocations[0].arms.len(), 2);
    assert!(!rollout.allocations[0].arms_invalid);

    let governance = index.governance.as_ref().expect("governance node");
    assert_eq!(governance.blocks.len(), 1);
    assert_eq!(governance.blocks[0].kind, "variable");
    assert_eq!(governance.blocks[0].id, "flag");
    assert!(governance.unknown_kinds.is_empty());

    assert!(index.custom_lints.files.contains_key("lint/checks.lua"));
    let registration = index
        .custom_lints
        .registrations
        .iter()
        .find(|registration| registration.rule.as_str() == "acme/flag-check")
        .expect("registered rule");
    assert_eq!(registration.selector.address.to_string(), "variable=flag");
}

#[tokio::test]
async fn unclaimed_files_produce_no_nodes_and_a_discover_warning() {
    let mut files = vec![(
        "rototo-package.toml",
        r#"schema_version = 1
"#,
    )];
    files.push(("variables/readme.md", "not a variable\n"));
    files.push(("data/catalogs/orphan/entry.toml", "message = \"lost\"\n"));
    files.push(("model/lists/tier.json", "{}\n"));
    let (_tempdir, snapshot) = scratch_snapshot(&files).await;

    assert!(snapshot.index.variables.is_empty());
    assert!(snapshot.index.catalog_entries.is_empty());
    assert!(snapshot.index.lists.is_empty());

    let unrecognized = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule.as_string() == "rototo/unrecognized-file")
        .map(|diagnostic| diagnostic.primary.path.clone())
        .collect::<Vec<_>>();
    for expected in [
        "variables/readme.md",
        "data/catalogs/orphan/entry.toml",
        "model/lists/tier.json",
    ] {
        assert!(
            unrecognized.iter().any(|path| path == expected),
            "expected an unrecognized-file warning for {expected}, got {unrecognized:?}"
        );
    }
}

#[tokio::test]
async fn a_broken_file_never_drops_sibling_nodes() {
    let (_tempdir, snapshot) = scratch_snapshot(&[
        (
            "rototo-package.toml",
            r#"schema_version = 1
"#,
        ),
        ("variables/broken.toml", "= this is not toml\n"),
        (
            "variables/healthy.toml",
            r#"schema_version = 1
type = "string"

[resolve]
default = "hello"
"#,
        ),
    ])
    .await;

    assert!(snapshot.index.variables.contains_key("healthy"));
    assert!(!snapshot.index.variables.contains_key("broken"));
    assert!(snapshot.lint.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule.as_string() == "rototo/variable-parse-failed"
            && diagnostic.primary.path == "variables/broken.toml"
    }));
}

#[tokio::test]
async fn parseable_files_with_wrong_shapes_keep_their_nodes_with_error_states() {
    let (_tempdir, snapshot) = scratch_snapshot(&[
        (
            "rototo-package.toml",
            r#"schema_version = 1
"#,
        ),
        (
            "variables/odd.toml",
            r#"schema_version = 1
type = 5
"#,
        ),
        (
            "lists/tier.toml",
            "schema_version = 1\ntype = \"string\"\nmembers = \"nope\"\n",
        ),
        ("model/catalogs/bad.schema.json", "{ \"type\": [ }\n"),
        (
            "model/catalogs/uncompilable.schema.json",
            "{ \"type\": 12 }\n",
        ),
    ])
    .await;
    let index = &snapshot.index;

    let odd = index.variables.get("odd").expect("odd keeps its node");
    assert!(matches!(odd.type_source, TypeSourceNode::Invalid { .. }));
    assert!(matches!(odd.resolve, ResolveNode::Missing { .. }));

    let members = index.lists.get("tier").expect("tier keeps a node");
    assert!(matches!(members.members, ProjectField::Invalid { .. }));

    // A schema that fails to parse as JSON keeps its node with no compiled
    // validator; the parse failure itself is a syntax-stage diagnostic.
    let bad = index.catalogs.get("bad").expect("bad catalog keeps a node");
    assert!(bad.json.is_none());
    assert!(bad.validator.is_none());
    assert!(
        snapshot
            .lint
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.primary.path == "model/catalogs/bad.schema.json" })
    );

    // A schema that parses but does not compile keeps its node and records
    // why compilation failed.
    let uncompilable = index
        .catalogs
        .get("uncompilable")
        .expect("uncompilable catalog keeps a node");
    assert!(uncompilable.json.is_some());
    assert!(uncompilable.validator.is_none());
    assert!(uncompilable.invalid_message.is_some());
}

#[tokio::test]
async fn node_and_field_locations_span_their_declaring_lines() {
    let (_tempdir, snapshot) = scratch_snapshot(&full_package()).await;
    let index = &snapshot.index;

    let flag = &index.variables["flag"];
    assert_eq!(flag.location.path, "variables/flag.toml");
    let type_location = flag.type_source.location();
    assert_eq!(type_location.path, "variables/flag.toml");
    assert_eq!(type_location.range.expect("type has a range").start.line, 1);
    match &flag.resolve {
        ResolveNode::Resolve { default, rules, .. } => {
            assert_eq!(
                default.location().range.expect("default range").start.line,
                4
            );
            let RuleCollection::Rules(rules) = rules else {
                panic!("rules should project");
            };
            let when = rules[0].when.as_ref().expect("rule has when");
            assert_eq!(when.location().range.expect("when range").start.line, 7);
        }
        _ => panic!("resolve should project"),
    }

    let entry = &index.catalog_entries["banner"]["default"];
    assert_eq!(entry.location.path, "data/catalogs/banner/default.toml");
    let sample = &index.evaluation_context_samples["request"]["basic"];
    assert_eq!(
        sample.location.path,
        "model/context/request-samples/basic.json"
    );
    let members = &index.lists["tier"];
    assert_eq!(members.location.path, "lists/tier.toml");
    assert!(members.members.location().range.is_some());
}

#[tokio::test]
async fn a_longer_catalog_id_owns_its_data_subtree() {
    // Overlapping catalog ids are a lint error (rototo/catalog-id-overlap),
    // but discovery still resolves ownership deterministically: the longer
    // id owns its subtree, so the index never double-claims a file.
    let (_tempdir, snapshot) = scratch_snapshot(&[
        ("rototo-package.toml", "schema_version = 1\n"),
        (
            "model/catalogs/plans.schema.json",
            r#"{"type":"object","properties":{"name":{"type":"string"}}}"#,
        ),
        (
            "model/catalogs/plans/regional.schema.json",
            r#"{"type":"object","properties":{"name":{"type":"string"}}}"#,
        ),
        ("data/catalogs/plans/basic.toml", "name = \"basic\"\n"),
        ("data/catalogs/plans/regional/eu.toml", "name = \"eu\"\n"),
    ])
    .await;
    let index = &snapshot.index;

    let plans = index.catalog_entries.get("plans").expect("plans entries");
    assert_eq!(plans.keys().cloned().collect::<Vec<_>>(), vec!["basic"]);
    let regional = index
        .catalog_entries
        .get("plans/regional")
        .expect("regional entries");
    assert_eq!(regional.keys().cloned().collect::<Vec<_>>(), vec!["eu"]);

    assert!(
        snapshot
            .lint
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.rule.as_string() == "rototo/catalog-id-overlap" })
    );
}

#[tokio::test]
async fn overlay_marker_paths_are_not_documents() {
    // Update and deleted markers are consumed when layers flatten; an unsaved
    // marker buffer is not a lintable document and must not mint a node.
    let tempdir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tempdir.path().join("rototo-package.toml"),
        "schema_version = 1\n",
    )
    .await
    .unwrap();
    let mut input = LintInput::new(tempdir.path().to_path_buf());
    for path in [
        "variables/flag.update.toml",
        "data/catalogs/banner/default.deleted.toml",
    ] {
        input.overlays.insert(
            path.to_owned(),
            crate::lint::OverlayDocument {
                text: "[resolve]\ndefault = true\n".to_owned(),
                version: Some(1),
            },
        );
    }
    let snapshot = lint_package_snapshot(input).await.unwrap();

    assert!(snapshot.index.variables.is_empty());
    assert!(snapshot.index.catalog_entries.is_empty());
    assert!(
        !snapshot
            .lint
            .documents
            .iter()
            .any(|document| document.path.contains(".update.")
                || document.path.contains(".deleted."))
    );
}

#[tokio::test]
async fn index_agrees_with_the_projected_semantic_model_and_symbols() {
    let snapshot = lint_package_snapshot(LintInput::new(Path::new("examples/basic").to_path_buf()))
        .await
        .unwrap();
    let model = snapshot.semantic_model();

    let index_variables = snapshot.index.variables.keys().cloned().collect::<Vec<_>>();
    let mut model_variables = model
        .variables
        .iter()
        .map(|variable| variable.id.clone())
        .collect::<Vec<_>>();
    model_variables.sort();
    assert_eq!(index_variables, model_variables);

    let index_catalogs = snapshot.index.catalogs.keys().cloned().collect::<Vec<_>>();
    let mut model_catalogs = model
        .catalogs
        .iter()
        .map(|catalog| catalog.id.clone())
        .collect::<Vec<_>>();
    model_catalogs.sort();
    assert_eq!(index_catalogs, model_catalogs);

    let index_contexts = snapshot
        .index
        .evaluation_contexts
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut model_contexts = model
        .evaluation_contexts
        .iter()
        .map(|context| context.id.clone())
        .collect::<Vec<_>>();
    model_contexts.sort();
    assert_eq!(index_contexts, model_contexts);

    let index_entry_count: usize = snapshot
        .index
        .catalog_entries
        .values()
        .map(|entries| entries.len())
        .sum();
    assert_eq!(index_entry_count, model.catalog_entries.len());

    let index_lint_files = snapshot
        .index
        .custom_lints
        .files
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut model_linters = model
        .linters
        .iter()
        .map(|linter| linter.path.clone())
        .collect::<Vec<_>>();
    model_linters.sort();
    assert_eq!(index_lint_files, model_linters);

    // Every indexed variable document also yields document symbols rooted at
    // the variable id: the symbol tree and the index describe the same file.
    for variable in snapshot.index.variables.values() {
        let symbols = snapshot.document_symbols(&variable.location.path);
        assert_eq!(
            symbols[0].name, variable.id,
            "document symbols disagree with the index for {}",
            variable.location.path
        );
    }
}

#[tokio::test]
async fn resolved_reference_edges_agree_with_the_index_and_definition() {
    let snapshot = lint_package_snapshot(LintInput::new(Path::new("examples/basic").to_path_buf()))
        .await
        .unwrap();

    let mut resolved = 0;
    for edge in snapshot.references.edges() {
        if !edge.is_resolved() {
            continue;
        }
        resolved += 1;

        // The declaration map must agree with the edge snapshot.
        let declaration = snapshot
            .references
            .declaration(&edge.target)
            .unwrap_or_else(|| panic!("resolved edge lost its declaration: {:?}", edge.target));

        // The entity a resolved edge points at must exist in the index: the
        // reference walker and the index describe one package.
        use crate::lint::references::ReferenceTarget;
        match &edge.target {
            ReferenceTarget::Variable(id) => {
                assert!(snapshot.index.variables.contains_key(id));
            }
            ReferenceTarget::Catalog(id) => {
                assert!(snapshot.index.catalogs.contains_key(id));
            }
            ReferenceTarget::CatalogEntry { catalog, value } => {
                assert!(
                    snapshot
                        .index
                        .catalog_entries
                        .get(catalog)
                        .is_some_and(|entries| entries.contains_key(value)),
                    "reference to {catalog}/{value} has no index entry"
                );
            }
            ReferenceTarget::Allocation(id) => {
                assert!(snapshot.index.layers.values().any(|layer| {
                    layer.allocations.iter().any(|allocation| {
                        matches!(&allocation.id, ProjectField::Present(present) if present.value == *id)
                    })
                }));
            }
            ReferenceTarget::List(id) => {
                assert!(snapshot.index.lists.contains_key(id));
            }
            ReferenceTarget::ContextAttribute(_) | ReferenceTarget::VariableValue { .. } => {}
        }

        // Go-to-definition from the reference site must land somewhere: the
        // same snapshot serves the walker and the definition provider.
        if let Some(range) = edge.location.range {
            let definition = snapshot
                .definition(&edge.location.path, range.start)
                .unwrap_or_else(|| {
                    panic!(
                        "definition returned nothing at {}:{}:{} for {:?}",
                        edge.location.path, range.start.line, range.start.character, edge.target
                    )
                });
            assert!(!definition.location.path.is_empty());
            let _ = declaration;
        }
    }
    assert!(
        resolved >= 10,
        "examples/basic should exercise many resolved references, saw {resolved}"
    );
}
