use serde_json::Value as JsonValue;
use toml_span::Value as TomlValue;
use toml_span::value::Table;

use crate::diagnostics::DiagnosticLocation;
use crate::expression::Expression;

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

pub(super) fn json_field(
    document: &SourceDocument,
    table: &Table<'_>,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<JsonValue> {
    match table.get(key) {
        Some(item) => ProjectField::Present(Spanned {
            value: json_from_toml_value(item),
            location: item_location(document, item),
        }),
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

pub(super) fn expression_field(
    document: &SourceDocument,
    table: &Table<'_>,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<Expression> {
    match table.get(key) {
        Some(item) => match item.as_str() {
            Some(value) => match Expression::parse(value) {
                Ok(expression) => ProjectField::Present(Spanned {
                    value: expression,
                    location: item_location(document, item),
                }),
                Err(_) => ProjectField::Invalid {
                    location: item_location(document, item),
                },
            },
            None => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Missing {
            location: missing_location,
        },
    }
}

pub(super) fn optional_expression_field(
    document: &SourceDocument,
    table: &Table<'_>,
    key: &str,
) -> Option<ProjectField<Expression>> {
    let item = table.get(key)?;
    Some(match item.as_str() {
        Some(value) => match Expression::parse(value) {
            Ok(expression) => ProjectField::Present(Spanned {
                value: expression,
                location: item_location(document, item),
            }),
            Err(_) => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Invalid {
            location: item_location(document, item),
        },
    })
}

pub(crate) fn json_from_toml_value(value: &TomlValue<'_>) -> JsonValue {
    serde_json::to_value(plain_toml_from_span_value(value)).unwrap_or(JsonValue::Null)
}
