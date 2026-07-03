use std::fs;
use std::path::Path;

use rototo::lint::{ModelEntityRef, ModelReferenceVia, package_semantic_model};

#[tokio::test]
async fn semantic_model_projects_entities_references_and_ranges() {
    let model = package_semantic_model(Path::new("examples/basic"))
        .await
        .expect("examples/basic should produce a semantic model");

    assert_eq!(model.version, 3);
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

    // The model serializes with camelCase keys and tagged entity refs.
    let json = serde_json::to_value(&model).expect("model serializes");
    assert!(json["catalogEntries"].is_array());
    assert_eq!(json["references"][0]["from"]["kind"], "variable");
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

type = "list<catalog:message_template>"

[resolve]
default = []

[[resolve.rule]]
query = 'entry.channel == context.channel && entry.active == true && variables["premium"]'
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
    assert_eq!(variable.declaration.kind, "primitive");
    assert_eq!(
        variable.declaration.value.as_deref(),
        Some("list<catalog:message_template>")
    );
    let rule = &variable.resolve.as_ref().expect("resolve section").rules[0];
    assert_eq!(rule.index, 0);
    assert!(rule.when.is_none());
    let query = rule.query.as_ref().expect("query field");
    assert_eq!(
        query.value.as_deref(),
        Some(r#"entry.channel == context.channel && entry.active == true && variables["premium"]"#)
    );
    assert!(
        query.location.range.is_some(),
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

    let query_condition_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "templates")
            && matches!(&reference.to, ModelEntityRef::Variable { id } if id == "premium")
            && matches!(&reference.via, ModelReferenceVia::RuleCondition { index } if *index == 0)
    });
    assert!(query_condition_edge, "query condition reference edge");
}

fn write_file(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}
