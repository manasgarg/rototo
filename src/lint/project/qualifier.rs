use toml_edit::Table;

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
    let root = toml.edit.as_table();
    let location = document.document_location();
    let schema_version = integer_field(document, root, "schema_version", location.clone());
    let description = optional_string_field(document, root, "description");
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

fn project_predicates(document: &SourceDocument, root: &Table) -> PredicateCollection {
    let Some(item) = root.get("predicate") else {
        return PredicateCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(predicates) = item.as_array_of_tables() else {
        return PredicateCollection::Invalid {
            location: item_location(document, item),
        };
    };

    PredicateCollection::Predicates(
        predicates
            .iter()
            .enumerate()
            .map(|(index, table)| project_predicate(document, index, table))
            .collect(),
    )
}

fn project_predicate(document: &SourceDocument, index: usize, table: &Table) -> PredicateNode {
    let location = table_location(document, table);
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
