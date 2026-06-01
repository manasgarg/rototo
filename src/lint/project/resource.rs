use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, table_location};
use super::fields::{integer_field, json_from_toml_value, optional_string_field, string_field};

pub(crate) fn project_resource(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
) -> ResourceNode {
    let root = toml.root_table();
    let location = document.document_location();
    let schema_version = root
        .map(|root| integer_field(document, root, "schema_version", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let description = root.and_then(|root| optional_string_field(document, root, "description"));
    let schema = root
        .map(|root| string_field(document, root, "schema", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });

    ResourceNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        schema,
    }
}

pub(crate) fn project_resource_object(
    document: &SourceDocument,
    toml: &ParsedToml,
    resource_id: &str,
    key: &str,
) -> ResourceObjectNode {
    let root = toml.root();
    ResourceObjectNode {
        resource_id: resource_id.to_owned(),
        key: key.to_owned(),
        location: table_location(document, root),
        value: json_from_toml_value(root),
    }
}
