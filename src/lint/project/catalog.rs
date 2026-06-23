use std::sync::Arc;

use super::super::catalog_schema::catalog_schema_uri;
use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, SyntaxIndex, table_location};
use super::fields::json_from_toml_value;

pub(crate) fn project_catalog(
    document: &SourceDocument,
    syntax: &SyntaxIndex,
    id: &str,
) -> CatalogNode {
    let json = syntax.json.get(&document.id).cloned();

    CatalogNode {
        doc: document.id,
        id: id.to_owned(),
        path: document.path.clone(),
        location: document.document_location(),
        json,
        validator: None,
        invalid_message: None,
    }
}

pub(crate) fn compile_catalog_validators(index: &mut SemanticIndex) {
    let resources = catalog_schema_resources(index);
    for catalog in index.catalogs.values_mut() {
        let Some(json) = catalog.json.as_ref() else {
            continue;
        };
        let base_uri = catalog_schema_uri(&catalog.id);
        match jsonschema::options()
            .with_base_uri(base_uri)
            .with_resources(resources.clone().into_iter())
            .build(json)
        {
            Ok(validator) => {
                catalog.validator = Some(Arc::new(validator));
                catalog.invalid_message = None;
            }
            Err(err) => {
                catalog.validator = None;
                catalog.invalid_message = Some(err.to_string());
            }
        }
    }
}

fn catalog_schema_resources(index: &SemanticIndex) -> Vec<(String, jsonschema::Resource)> {
    let mut resources = Vec::new();
    for catalog in index.catalogs.values() {
        let Some(json) = catalog.json.as_ref() else {
            continue;
        };
        let Ok(resource) = jsonschema::Resource::from_contents(json.clone()) else {
            continue;
        };

        resources.push((catalog_schema_uri(&catalog.id), resource.clone()));
        if let Some(id) = json
            .get("$id")
            .and_then(serde_json::Value::as_str)
            .map(normalize_schema_uri)
            .filter(|id| !id.is_empty())
        {
            resources.push((id, resource));
        }
    }
    resources
}

fn normalize_schema_uri(uri: &str) -> String {
    uri.trim_end_matches('#').to_owned()
}

pub(crate) fn project_catalog_entry(
    document: &SourceDocument,
    toml: &ParsedToml,
    catalog_id: &str,
    key: &str,
) -> CatalogEntryNode {
    let root = toml.root();
    CatalogEntryNode {
        catalog_id: catalog_id.to_owned(),
        key: key.to_owned(),
        location: table_location(document, root),
        value: json_from_toml_value(root),
    }
}
