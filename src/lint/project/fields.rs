use serde_json::Value as JsonValue;
use toml_span::Value as TomlValue;
use toml_span::value::Table;

use crate::diagnostics::{DiagnosticLocation, Severity};

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{item_location, plain_toml_from_span_value};

pub(super) fn integer_field(
    document: &SourceDocument,
    table: &Table<'_>,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<i64> {
    match table.get(key) {
        Some(item) => match item.as_integer() {
            Some(value) => ProjectField::Present(Spanned {
                value,
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

pub(super) fn string_field(
    document: &SourceDocument,
    table: &Table<'_>,
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

pub(super) fn optional_string_field(
    document: &SourceDocument,
    table: &Table<'_>,
    key: &str,
) -> Option<ProjectField<String>> {
    let item = table.get(key)?;
    Some(match item.as_str() {
        Some(value) => ProjectField::Present(Spanned {
            value: value.to_owned(),
            location: item_location(document, item),
        }),
        None => ProjectField::Invalid {
            location: item_location(document, item),
        },
    })
}

pub(super) fn optional_severity_field(
    document: &SourceDocument,
    table: &Table<'_>,
    key: &str,
) -> Option<ProjectField<Severity>> {
    let item = table.get(key)?;
    Some(match item.as_str().and_then(Severity::parse) {
        Some(value) => ProjectField::Present(Spanned {
            value,
            location: item_location(document, item),
        }),
        None => ProjectField::Invalid {
            location: item_location(document, item),
        },
    })
}

pub(super) fn predicate_op_field(
    document: &SourceDocument,
    table: &Table<'_>,
    missing_location: DiagnosticLocation,
) -> ProjectField<PredicateOp> {
    match string_field(document, table, "op", missing_location) {
        ProjectField::Present(op) => ProjectField::Present(Spanned {
            value: PredicateOp::from_str(&op.value),
            location: op.location,
        }),
        ProjectField::Invalid { location } => ProjectField::Invalid { location },
        ProjectField::Missing { location } => ProjectField::Missing { location },
    }
}

pub(super) fn project_value_shape(
    document: &SourceDocument,
    item: &TomlValue<'_>,
) -> ValueShapeNode {
    ValueShapeNode {
        location: item_location(document, item),
        shape: value_shape(item),
    }
}

pub(super) fn project_bucket_range(
    document: &SourceDocument,
    item: &TomlValue<'_>,
) -> BucketRangeNode {
    let location = item_location(document, item);
    let Some(array) = item.as_array() else {
        return BucketRangeNode {
            location,
            is_array: false,
            len: 0,
            start: None,
            end: None,
        };
    };
    let values: Vec<_> = array.iter().collect();
    BucketRangeNode {
        location,
        is_array: true,
        len: values.len(),
        start: values.first().and_then(|value| value.as_integer()),
        end: values.get(1).and_then(|value| value.as_integer()),
    }
}

fn value_shape(item: &TomlValue<'_>) -> ValueShape {
    if item.as_str().is_some() {
        ValueShape::String
    } else if item.as_integer().is_some() {
        ValueShape::Integer
    } else if item.as_float().is_some() {
        ValueShape::Float
    } else if item.as_bool().is_some() {
        ValueShape::Boolean
    } else if item.as_array().is_some() {
        ValueShape::Array
    } else {
        ValueShape::Table
    }
}

pub(crate) fn json_from_toml_value(value: &TomlValue<'_>) -> JsonValue {
    serde_json::to_value(plain_toml_from_span_value(value)).unwrap_or(JsonValue::Null)
}
