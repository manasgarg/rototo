use crate::diagnostics::RototoRuleId;
use crate::diagnostics::{EntityId, LintDiagnostic, LintStage};

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
    let start = err.span.start.min(document.text.len());
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
    let bounded_end = raw_end.min(document.text.len()).max(start);
    let Some(relative_newline) = document.text[start..bounded_end].find('\n') else {
        return bounded_end;
    };
    start + relative_newline + 1
}

pub(super) fn json_parse_diagnostic(
    document: &SourceDocument,
    err: &::serde_json::Error,
) -> LintDiagnostic {
    let line = err.line().saturating_sub(1);
    let column = err.column();
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

fn parse_failed_rule(kind: &DocumentKind) -> RototoRuleId {
    match kind {
        DocumentKind::Manifest => RototoRuleId::WorkspaceManifestParseFailed,
        DocumentKind::Qualifier { .. } => RototoRuleId::QualifierParseFailed,
        DocumentKind::Variable { .. } => RototoRuleId::VariableParseFailed,
        DocumentKind::ExternalValue { .. } => RototoRuleId::VariableExternalValueParseFailed,
        DocumentKind::Schema => RototoRuleId::SchemaParseFailed,
        DocumentKind::CustomLint => RototoRuleId::CustomLintFailed,
    }
}

fn entity_for_document(document: &SourceDocument) -> EntityId {
    match &document.kind {
        DocumentKind::Manifest => EntityId::Manifest,
        DocumentKind::Qualifier { id } => EntityId::Qualifier { id: id.clone() },
        DocumentKind::Variable { id } => EntityId::Variable { id: id.clone() },
        DocumentKind::ExternalValue {
            variable_id,
            value_key,
        } => EntityId::Value {
            variable: variable_id.clone(),
            key: value_key.clone(),
        },
        DocumentKind::Schema => EntityId::Schema {
            path: document.path.clone(),
        },
        DocumentKind::CustomLint => EntityId::CustomLint {
            path: document.path.clone(),
        },
    }
}
