use crate::diagnostics::RototoRuleId;
use crate::diagnostics::{LintDiagnostic, LintStage, SemanticEntity};

use super::super::source::{DocumentKind, SourceDocument};

pub(super) fn read_error_diagnostic(document: &SourceDocument, read_error: &str) -> LintDiagnostic {
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        document.document_location(),
        format!("failed to read {}: {read_error}", document.path),
    )
}

pub(super) fn toml_span_parse_diagnostic(
    document: &SourceDocument,
    err: &::toml_span::Error,
) -> LintDiagnostic {
    let start = floor_char_boundary(&document.text, err.span.start.min(document.text.len()));
    let end = parse_error_end(document, start, err.span.end);
    let location = if start == end {
        document.document_location()
    } else {
        document.span_location(start..end)
    };
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        location,
        format!("failed to parse {}: {err}", document.path),
    )
}

fn parse_error_end(document: &SourceDocument, start: usize, raw_end: usize) -> usize {
    let bounded_end =
        ceil_char_boundary(&document.text, raw_end.min(document.text.len())).max(start);
    let Some(slice) = document.text.get(start..bounded_end) else {
        return start;
    };
    let Some(relative_newline) = slice.find('\n') else {
        return bounded_end;
    };
    start + relative_newline + 1
}

pub(super) fn json_parse_diagnostic(
    document: &SourceDocument,
    err: &::serde_json::Error,
) -> LintDiagnostic {
    let line = err.line().saturating_sub(1);
    let column = err.column().saturating_sub(1);
    let start = document.line_index.offset_for_line_character(line, column);
    let end = start.saturating_add(1).min(document.text.len());
    LintDiagnostic::rototo(
        RototoRuleId::SchemaParseFailed,
        LintStage::Parse,
        entity_for_document(document),
        document.span_location(start..end),
        format!("failed to parse {}: {err}", document.path),
    )
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

fn parse_failed_rule(kind: &DocumentKind) -> RototoRuleId {
    match kind {
        DocumentKind::Manifest => RototoRuleId::WorkspaceManifestParseFailed,
        DocumentKind::Qualifier { .. } => RototoRuleId::QualifierParseFailed,
        DocumentKind::Variable { .. } => RototoRuleId::VariableParseFailed,
        DocumentKind::Resource { .. } => RototoRuleId::ResourceParseFailed,
        DocumentKind::ResourceObject { .. } => RototoRuleId::ResourceObjectParseFailed,
        DocumentKind::Schema => RototoRuleId::SchemaParseFailed,
        DocumentKind::CustomLint => RototoRuleId::CustomLintFailed,
    }
}

fn entity_for_document(document: &SourceDocument) -> SemanticEntity {
    match &document.kind {
        DocumentKind::Manifest => SemanticEntity::Manifest,
        DocumentKind::Qualifier { id } => SemanticEntity::Qualifier { id: id.clone() },
        DocumentKind::Variable { id } => SemanticEntity::Variable { id: id.clone() },
        DocumentKind::Resource { id } => SemanticEntity::Resource { id: id.clone() },
        DocumentKind::ResourceObject {
            resource_id,
            object_id,
        } => SemanticEntity::ResourceObject {
            resource: resource_id.clone(),
            key: object_id.clone(),
        },
        DocumentKind::Schema => SemanticEntity::Schema {
            path: document.path.clone(),
        },
        DocumentKind::CustomLint => SemanticEntity::CustomLint {
            path: document.path.clone(),
        },
    }
}
