use std::fs;
use std::path::Path;

use rototo::lint::{ModelEntityRef, ModelReferenceVia, package_semantic_model};

#[tokio::test]
async fn semantic_model_projects_entities_references_and_ranges() {
    let model = package_semantic_model(Path::new("examples/basic"))
        .await
        .expect("examples/basic should produce a semantic model");

    assert_eq!(model.version, 5);
    assert!(!model.variables.is_empty());
    assert!(!model.catalogs.is_empty());
    assert!(!model.linters.is_empty());

    let catalog = model
        .catalogs
        .iter()
        .find(|catalog| catalog.id == "support_banner")
        .expect("support_banner catalog");
    assert!(
        catalog
            .path
            .ends_with("model/catalogs/support_banner.schema.json")
    );
    assert_eq!(
        catalog
            .json
            .as_ref()
            .and_then(|json| json.get("type"))
            .and_then(|value| value.as_str()),
        Some("object")
    );

    let variable = model
        .variables
        .iter()
        .find(|variable| variable.id == "support_banner")
        .expect("support_banner variable");
    assert_eq!(variable.declaration.kind, "catalog");
    assert!(
        variable.uses_context,
        "context use follows the referenced condition variable"
    );
    assert!(
        variable
            .context_paths
            .iter()
            .any(|path| path == "device.platform"),
        "read context paths follow the referenced condition variable"
    );
    assert_eq!(
        variable.declaration.value.as_deref(),
        Some("support_banner")
    );
    assert!(
        variable
            .location
            .path
            .ends_with("variables/support_banner.toml")
    );

    let resolve = variable.resolve.as_ref().expect("resolve section");
    let default = resolve.default.as_ref().expect("default field");
    assert_eq!(
        default.value.as_ref().and_then(|value| value.as_str()),
        Some("hidden")
    );
    assert!(
        default.location.range.is_some(),
        "field locations carry ranges for range-based edits"
    );
    assert_eq!(resolve.rules.len(), 2);
    let rule = &resolve.rules[0];
    assert_eq!(
        rule.when.as_ref().and_then(|field| field.value.as_deref()),
        Some("variables[\"mobile_users\"]")
    );
    assert!(
        rule.when
            .as_ref()
            .and_then(|field| field.location.range)
            .is_some()
    );

    let object = model
        .catalog_entries
        .iter()
        .find(|object| object.catalog == "support_banner" && object.key == "mobile_help")
        .expect("support_banner/mobile_help object");
    assert!(object.value.is_object());

    // The reference graph covers rule conditions and selected catalog values.
    let rule_condition_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "support_banner")
            && matches!(&reference.to, ModelEntityRef::Variable { id } if id == "mobile_users")
    });
    assert!(
        rule_condition_edge,
        "variable rule condition -> condition variable edge"
    );
    let object_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "support_banner")
            && matches!(
                &reference.to,
                ModelEntityRef::CatalogEntry { catalog, key }
                    if catalog == "support_banner" && key == "mobile_help"
            )
    });
    assert!(object_edge, "variable value -> catalog value edge");

    // Both directions of the promoted reference queries answer from the
    // same edge list: what an entity uses, and who uses it.
    let mobile_users = ModelEntityRef::Variable {
        id: "mobile_users".to_owned(),
    };
    let referencing: Vec<_> = model.references_to(&mobile_users).collect();
    assert!(
        referencing
            .iter()
            .any(|reference| matches!(&reference.from, ModelEntityRef::Variable { id } if id == "support_banner")),
        "references_to answers who references mobile_users"
    );
    assert!(
        referencing
            .iter()
            .all(|reference| reference.declaration.is_some()),
        "in-package references carry their target's declaration"
    );
    let support_banner = ModelEntityRef::Variable {
        id: "support_banner".to_owned(),
    };
    assert!(
        model
            .references_from(&support_banner)
            .any(|reference| matches!(&reference.to, ModelEntityRef::Variable { id } if id == "mobile_users")),
        "references_from answers what support_banner uses"
    );

    // A standalone package declares no extends edges.
    assert!(model.extends.is_empty());

    // The model serializes with camelCase keys and tagged entity refs.
    let json = serde_json::to_value(&model).expect("model serializes");
    assert!(json["catalogEntries"].is_array());
    assert!(json["variables"][0]["usesContext"].is_boolean());
    assert_eq!(json["references"][0]["from"]["kind"], "variable");
}

/// Discovery composes each package's declared `extends` edges into the
/// composition tree, and lists are first-class entities in the model.
#[tokio::test]
async fn semantic_model_projects_extends_edges_and_lists() {
    let overlay = package_semantic_model(Path::new("examples/acme-overlay"))
        .await
        .expect("examples/acme-overlay should produce a semantic model");
    assert_eq!(
        overlay
            .extends
            .iter()
            .map(|extend| extend.source.as_str())
            .collect::<Vec<_>>(),
        vec!["../basic"]
    );
    assert!(
        overlay.extends[0]
            .location
            .path
            .ends_with("rototo-package.toml")
    );

    let release_ops = package_semantic_model(Path::new("examples/release-ops"))
        .await
        .expect("examples/release-ops should produce a semantic model");
    let log_levels = release_ops
        .lists
        .iter()
        .find(|entry| entry.id == "log_levels")
        .expect("log_levels list");
    assert!(log_levels.location.path.ends_with("lists/log_levels.toml"));
    assert_eq!(log_levels.member_type.value.as_deref(), Some("string"));
    assert_eq!(
        log_levels
            .members
            .iter()
            .filter_map(|member| member.value.as_ref().and_then(|value| value.as_str()))
            .collect::<Vec<_>>(),
        vec!["error", "warn", "info", "debug"]
    );
    assert!(
        log_levels
            .members
            .iter()
            .all(|member| member.location.range.is_some()),
        "member locations carry ranges for member-level edits"
    );

    // A list-typed variable references its list through the declaration, so
    // "who uses this list" answers from the same edge list as catalogs.
    let type_edge = release_ops
        .references
        .iter()
        .find(|reference| {
            matches!(&reference.from, ModelEntityRef::Variable { id } if id == "log_level")
                && matches!(&reference.to, ModelEntityRef::List { id } if id == "log_levels")
        })
        .expect("list-typed variable -> list edge");
    assert!(matches!(type_edge.via, ModelReferenceVia::VariableList));
    assert!(
        type_edge.declaration.is_some(),
        "in-package list references carry the list's declaration"
    );
}

/// A membership test in a rule condition (`context.tier in lists.plan_tiers`)
/// references the list it reads, attributed to the rule.
#[tokio::test]
async fn semantic_model_projects_expression_list_references() {
    let model = package_semantic_model(Path::new(
        "tests/fixtures/packages/schema-enum-context-types",
    ))
    .await
    .expect("schema-enum-context-types should produce a semantic model");
    let rule_edge = model
        .references
        .iter()
        .find(|reference| {
            matches!(&reference.from, ModelEntityRef::Variable { id } if id == "tier_gate")
                && matches!(&reference.to, ModelEntityRef::List { id } if id == "plan_tiers")
        })
        .expect("rule membership -> list edge");
    assert!(matches!(
        rule_edge.via,
        ModelReferenceVia::RuleCondition { index: 0 }
    ));
}

#[tokio::test]
async fn semantic_model_projects_query_rules_and_evaluation_context_compatibility() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    write_file(root, "rototo-package.toml", "schema_version = 1\n");
    write_file(
        root,
        "model/context/request.schema.json",
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "channel": { "type": "string" },
    "user": {
      "type": "object",
      "properties": {
        "tier": { "type": "string" }
      }
    }
  }
}
"#,
    );
    write_file(
        root,
        "model/context/request-samples/premium-email.json",
        r#"{
  "channel": "email",
  "user": { "tier": "premium" }
}
"#,
    );
    write_file(
        root,
        "variables/premium.toml",
        r#"schema_version = 1

description = "Premium users"
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'context.user.tier == "premium"'
value = true
"#,
    );
    write_file(
        root,
        "model/catalogs/message_template.schema.json",
        r#"{
  "type": "object",
  "required": ["channel", "active", "body"],
  "properties": {
    "channel": { "type": "string" },
    "active": { "type": "boolean" },
    "body": { "type": "string" }
  }
}
"#,
    );
    write_file(
        root,
        "data/catalogs/message_template/email.toml",
        r#"channel = "email"
active = true
body = "Email body"
"#,
    );
    write_file(
        root,
        "variables/templates.toml",
        r#"schema_version = 1

type = "array<catalog=message_template>"

[resolve]
method = "query"
from = "message_template"
filter = 'entry.channel == context.channel && entry.active == true && variables["premium"]'
"#,
    );
    write_file(
        root,
        "variables/static_gate.toml",
        r#"schema_version = 1

type = "bool"

[resolve]
default = true
"#,
    );
    write_file(
        root,
        "variables/static_templates.toml",
        r#"schema_version = 1

type = "array<catalog=message_template>"

[resolve]
method = "query"
from = "message_template"
filter = 'entry.active == variables["static_gate"]'
"#,
    );

    let model = package_semantic_model(root)
        .await
        .expect("temp package should produce a semantic model");

    let variable = model
        .variables
        .iter()
        .find(|variable| variable.id == "templates")
        .expect("templates variable");
    assert!(variable.uses_context);
    assert_eq!(variable.declaration.kind, "primitive");
    assert_eq!(
        variable.declaration.value.as_deref(),
        Some("array<catalog=message_template>")
    );
    let resolve = variable.resolve.as_ref().expect("resolve section");
    assert!(resolve.rules.is_empty());
    assert_eq!(
        resolve
            .method
            .as_ref()
            .and_then(|method| method.value.as_deref()),
        Some("query")
    );
    let query = resolve.query.as_ref().expect("query section");
    assert_eq!(
        query.from.as_ref().and_then(|from| from.value.as_deref()),
        Some("message_template")
    );
    let filter = query.filter.as_ref().expect("filter field");
    assert_eq!(
        filter.value.as_deref(),
        Some(r#"entry.channel == context.channel && entry.active == true && variables["premium"]"#)
    );
    assert!(
        filter.location.range.is_some(),
        "query fields carry source ranges for editor edits"
    );

    let sample = model
        .evaluation_context_samples
        .iter()
        .find(|entry| entry.evaluation_context == "request" && entry.key == "premium-email")
        .expect("evaluation context sample");
    assert_eq!(sample.value.as_ref().unwrap()["channel"], "email");

    let premium_contexts = model
        .variable_evaluation_contexts
        .iter()
        .find(|entry| entry.variable == "premium")
        .expect("premium evaluation-context compatibility");
    assert_eq!(premium_contexts.evaluation_contexts, vec!["request"]);

    let variable_contexts = model
        .variable_evaluation_contexts
        .iter()
        .find(|entry| entry.variable == "templates")
        .expect("templates evaluation-context compatibility");
    assert_eq!(variable_contexts.evaluation_contexts, vec!["request"]);

    let static_variable = model
        .variables
        .iter()
        .find(|variable| variable.id == "static_templates")
        .expect("static_templates variable");
    assert!(
        !static_variable.uses_context,
        "a query that reaches only a context-free variable stays context-free"
    );
    let static_trace =
        rototo::trace_variable_resolution(root, "static_templates", &serde_json::json!({}))
            .await
            .expect("a transitively context-free query resolves with an empty context");
    assert!(matches!(
        static_trace.resolution.source,
        rototo::model::VariableResolutionSource::CatalogArray { catalog, values }
            if catalog == "message_template" && values == ["email"]
    ));

    let query_reference_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "templates")
            && matches!(&reference.to, ModelEntityRef::Variable { id } if id == "premium")
            && matches!(&reference.via, ModelReferenceVia::Query)
    });
    assert!(query_reference_edge, "query filter reference edge");
}

fn write_file(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

/// Two independent lint pipelines over the same package must project
/// byte-identical semantic models: discovery order, node maps, reference
/// lists, and locations are all deterministic.
#[tokio::test]
async fn semantic_model_projection_is_deterministic_across_runs() {
    let first = package_semantic_model(Path::new("examples/basic"))
        .await
        .expect("first run");
    let second = package_semantic_model(Path::new("examples/basic"))
        .await
        .expect("second run");

    let first_json = serde_json::to_string(&first).expect("serialize first");
    let second_json = serde_json::to_string(&second).expect("serialize second");
    assert_eq!(first_json, second_json);
}
