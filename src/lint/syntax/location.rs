use crate::diagnostics::DiagnosticLocation;

use super::super::source::SourceDocument;

pub(crate) fn item_location(
    document: &SourceDocument,
    value: &::toml_span::Value<'_>,
) -> DiagnosticLocation {
    span_location(document, value_span(document, value))
}

pub(crate) fn table_location(
    document: &SourceDocument,
    value: &::toml_span::Value<'_>,
) -> DiagnosticLocation {
    span_location(document, value.span)
}

pub(crate) fn value_location(
    document: &SourceDocument,
    value: &::toml_span::Value<'_>,
) -> DiagnosticLocation {
    span_location(document, value_span(document, value))
}

fn span_location(document: &SourceDocument, span: ::toml_span::Span) -> DiagnosticLocation {
    let start = floor_char_boundary(&document.text, span.start.min(document.text.len()));
    let end = ceil_char_boundary(&document.text, span.end.min(document.text.len())).max(start);
    if start == end {
        document.document_location()
    } else {
        document.span_location(start..end)
    }
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn ceil_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

fn value_span(document: &SourceDocument, value: &::toml_span::Value<'_>) -> ::toml_span::Span {
    if value.as_str().is_none() {
        return value.span;
    }

    let bytes = document.text.as_bytes();
    let mut start = value.span.start.min(bytes.len());
    let mut end = value.span.end.min(bytes.len()).max(start);
    if start == 0 || end >= bytes.len() {
        return ::toml_span::Span::new(start, end);
    }

    let quote = bytes[start - 1];
    if !matches!(quote, b'"' | b'\'') || bytes[end] != quote {
        return ::toml_span::Span::new(start, end);
    }

    while start > 0 && bytes[start - 1] == quote {
        start -= 1;
    }
    while end < bytes.len() && bytes[end] == quote {
        end += 1;
    }
    ::toml_span::Span::new(start, end)
}
