use std::path::Path;

use rototo::lint::{ModelEntityRef, workspace_semantic_model};

#[tokio::test]
async fn semantic_model_projects_entities_references_and_ranges() {
    let model = workspace_semantic_model(Path::new("examples/basic"))
        .await
        .expect("examples/basic should produce a semantic model");

    assert_eq!(model.version, 3);
    assert!(!model.qualifiers.is_empty());
    assert!(!model.schemas.is_empty());
    assert!(!model.linters.is_empty());

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
        rule.qualifier
            .as_ref()
            .and_then(|field| field.value.as_deref()),
        Some("mobile-users")
    );
    assert!(
        rule.qualifier
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

    // The reference graph covers rule qualifiers and selected catalog values.
    let rule_qualifier_edge = model.references.iter().any(|reference| {
        matches!(&reference.from, ModelEntityRef::Variable { id } if id == "support-banner")
            && matches!(&reference.to, ModelEntityRef::Qualifier { id } if id == "mobile-users")
    });
    assert!(rule_qualifier_edge, "variable rule -> qualifier edge");
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
