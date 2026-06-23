use std::fs;
use std::path::Path;

use rototo::lint::{ModelEntityRef, ModelReferenceVia, workspace_semantic_model};

#[tokio::test]
async fn semantic_model_projects_entities_references_and_ranges() {
    let model = workspace_semantic_model(Path::new("examples/basic"))
        .await
        .expect("examples/basic should produce a semantic model");

    assert_eq!(model.version, 3);
    assert!(!model.qualifiers.is_empty());
    assert!(!model.catalogs.is_empty());
    assert!(!model.linters.is_empty());

    let catalog = model
        .catalogs
        .iter()
        .find(|catalog| catalog.id == "support-banner")
        .expect("support-banner catalog");
    assert!(
        catalog
            .path
            .ends_with("catalogs/support-banner.schema.json")
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
        .find(|variable| variable.id == "support-banner")
        .expect("support-banner variable");
    assert_eq!(variable.declaration.kind, "catalog");
    assert_eq!(
        variable.declaration.value.as_deref(),
        Some("support-banner")
    );
    assert!(
        variable
            .location
            .path
            .ends_with("variables/support-banner.toml")
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
        Some("qualifier[\"mobile-users\"]")
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
        .find(|object| object.catalog == "support-banner" && object.key == "mobile_help")
        .expect("support-banner/mobile_help object");
    assert!(object.value.is_object());

    // The reference graph covers rule conditions and selected catalog values.
    let rule_condition_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "support-banner")
            && matches!(&reference.to, ModelEntityRef::Qualifier { id } if id == "mobile-users")
    });
    assert!(
        rule_condition_edge,
        "variable rule condition -> qualifier edge"
    );
    let object_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "support-banner")
            && matches!(
                &reference.to,
                ModelEntityRef::CatalogEntry { catalog, key }
                    if catalog == "support-banner" && key == "mobile_help"
            )
    });
    assert!(object_edge, "variable value -> catalog value edge");

    // The model serializes with camelCase keys and tagged entity refs.
    let json = serde_json::to_value(&model).expect("model serializes");
    assert!(json["catalogEntries"].is_array());
    assert_eq!(json["references"][0]["from"]["kind"], "qualifier");
}

#[tokio::test]
async fn semantic_model_projects_query_rules_and_request_context_compatibility() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    write_file(root, "rototo-workspace.toml", "schema_version = 1\n");
    write_file(
        root,
        "request-contexts/request.schema.json",
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
        "request-contexts/request-entries/premium-email.json",
        r#"{
  "channel": "email",
  "user": { "tier": "premium" }
}
"#,
    );
    write_file(
        root,
        "qualifiers/premium.toml",
        r#"schema_version = 1

description = "Premium users"
when = 'context.user.tier == "premium"'
"#,
    );
    write_file(
        root,
        "catalogs/message-template.schema.json",
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
        "catalogs/message-template-entries/email.toml",
        r#"channel = "email"
active = true
body = "Email body"
"#,
    );
    write_file(
        root,
        "variables/templates.toml",
        r#"schema_version = 1

type = "list<catalog:message-template>"

[resolve]
default = []

[[resolve.rule]]
query = 'entry.channel == context.channel && entry.active == true && qualifier["premium"]'
"#,
    );

    let model = workspace_semantic_model(root)
        .await
        .expect("temp workspace should produce a semantic model");

    let variable = model
        .variables
        .iter()
        .find(|variable| variable.id == "templates")
        .expect("templates variable");
    assert_eq!(variable.declaration.kind, "primitive");
    assert_eq!(
        variable.declaration.value.as_deref(),
        Some("list<catalog:message-template>")
    );
    let rule = &variable.resolve.as_ref().expect("resolve section").rules[0];
    assert_eq!(rule.index, 0);
    assert!(rule.when.is_none());
    let query = rule.query.as_ref().expect("query field");
    assert_eq!(
        query.value.as_deref(),
        Some(r#"entry.channel == context.channel && entry.active == true && qualifier["premium"]"#)
    );
    assert!(
        query.location.range.is_some(),
        "query fields carry source ranges for editor edits"
    );

    let sample = model
        .request_context_entries
        .iter()
        .find(|entry| entry.request_context == "request" && entry.key == "premium-email")
        .expect("request context sample");
    assert_eq!(sample.value.as_ref().unwrap()["channel"], "email");

    let qualifier_contexts = model
        .qualifier_request_contexts
        .iter()
        .find(|entry| entry.qualifier == "premium")
        .expect("premium request-context compatibility");
    assert_eq!(qualifier_contexts.request_contexts, vec!["request"]);

    let variable_contexts = model
        .variable_request_contexts
        .iter()
        .find(|entry| entry.variable == "templates")
        .expect("templates request-context compatibility");
    assert_eq!(variable_contexts.request_contexts, vec!["request"]);

    let query_qualifier_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "templates")
            && matches!(&reference.to, ModelEntityRef::Qualifier { id } if id == "premium")
            && matches!(&reference.via, ModelReferenceVia::RuleCondition { index } if *index == 0)
    });
    assert!(query_qualifier_edge, "query qualifier reference edge");

    let context_attribute_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Qualifier { id } if id == "premium")
            && matches!(
                &reference.to,
                ModelEntityRef::ContextAttribute { name } if name == "user.tier"
            )
            && matches!(
                reference.via,
                ModelReferenceVia::QualifierWhenContextAttribute
            )
    });
    assert!(
        context_attribute_edge,
        "qualifier context attribute reference edge"
    );
}

fn write_file(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}
