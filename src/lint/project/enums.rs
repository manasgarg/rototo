use crate::diagnostics::DiagnosticLocation;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location};
use super::fields::{integer_field, optional_string_field};
use super::json_from_toml_value;

pub(crate) fn project_enum_declaration(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
) -> EnumNode {
    let root = toml.root_table();
    let location = document.document_location();
    let schema_version = root
        .map(|root| integer_field(document, root, "schema_version", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let description = root.and_then(|root| optional_string_field(document, root, "description"));
    let member_type = root
        .map(|root| string_field(document, root, "type", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });

    EnumNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        member_type,
    }
}

pub(crate) fn project_enum_members(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
) -> EnumMembersNode {
    let root = toml.root_table();
    let location = document.document_location();
    let members = match root.and_then(|root| root.get("members")) {
        Some(item) => match item.as_array() {
            Some(values) => ProjectField::Present(Spanned {
                value: values
                    .iter()
                    .map(|value| Spanned {
                        value: json_from_toml_value(value),
                        location: item_location(document, value),
                    })
                    .collect(),
                location: item_location(document, item),
            }),
            None => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Missing {
            location: location.clone(),
        },
    };

    EnumMembersNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        members,
    }
}

fn string_field(
    document: &SourceDocument,
    table: &toml_span::value::Table<'_>,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<String> {
    match table.get(key) {
        Some(item) => match item.as_str() {
            Some(value) => ProjectField::Present(Spanned {
                value: value.to_owned(),
                location: item_location(document, item),
            }),
            None => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Missing {
            location: missing_location,
        },
    }
}
