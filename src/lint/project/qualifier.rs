use toml_span::Value as TomlValue;
use toml_span::value::Table;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location, table_location};
use super::fields::{
    integer_field, optional_string_field, predicate_op_field, project_bucket_range,
    project_value_shape, string_field,
};

pub(crate) fn project_qualifier(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
) -> QualifierNode {
    let root = toml.root_table();
    let location = document.document_location();
    let schema_version = root
        .map(|root| integer_field(document, root, "schema_version", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let description = root.and_then(|root| optional_string_field(document, root, "description"));
    let predicates = project_predicates(document, root);

    QualifierNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        predicates,
    }
}

fn project_predicates(document: &SourceDocument, root: Option<&Table<'_>>) -> PredicateCollection {
    let Some(root) = root else {
        return PredicateCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(item) = root.get("predicate") else {
        return PredicateCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(predicates) = item.as_array() else {
        return PredicateCollection::Invalid {
            location: item_location(document, item),
        };
    };

    PredicateCollection::Predicates(
        predicates
            .iter()
            .enumerate()
            .map(|(index, value)| project_predicate(document, index, value))
            .collect(),
    )
}

fn project_predicate(
    document: &SourceDocument,
    index: usize,
    value: &TomlValue<'_>,
) -> PredicateNode {
    let location = table_location(document, value);
    let Some(table) = value.as_table() else {
        return PredicateNode {
            index,
            location: location.clone(),
            attribute: ProjectField::Invalid {
                location: location.clone(),
            },
            op: ProjectField::Invalid {
                location: location.clone(),
            },
            value: None,
            salt: None,
            range: None,
            has_bucket_value: false,
        };
    };
    let attribute = string_field(document, table, "attribute", location.clone());
    let op = predicate_op_field(document, table, location.clone());
    let value = table
        .get("value")
        .map(|item| project_value_shape(document, item));
    let salt = table
        .get("salt")
        .map(|_| string_field(document, table, "salt", location.clone()));
    let range = table
        .get("range")
        .map(|item| project_bucket_range(document, item));
    let has_bucket_value = table.contains_key("value");

    PredicateNode {
        index,
        location,
        attribute,
        op,
        value,
        salt,
        range,
        has_bucket_value,
    }
}
