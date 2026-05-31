use crate::diagnostics::DiagnosticLocation;

use super::super::source::SourceDocument;

pub(crate) fn item_location(
    document: &SourceDocument,
    item: &::toml_edit::Item,
) -> DiagnosticLocation {
    item.span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

pub(crate) fn table_location(
    document: &SourceDocument,
    table: &::toml_edit::Table,
) -> DiagnosticLocation {
    table
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

pub(crate) fn value_location(
    document: &SourceDocument,
    value: &::toml_edit::Value,
) -> DiagnosticLocation {
    value
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}
