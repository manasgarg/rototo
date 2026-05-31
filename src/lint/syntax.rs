use std::collections::BTreeMap;

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml_edit::{ImDocument, Item, Table, Value as EditValue};

use crate::diagnostics::RototoRuleId;
use crate::diagnostics::{DiagnosticLocation, DocId, EntityId, LintDiagnostic, LintStage};

use super::source::{DocumentKind, SourceDocument};

#[derive(Default)]
pub(super) struct SyntaxIndex {
    pub(super) toml: BTreeMap<DocId, ParsedToml>,
    pub(super) json: BTreeMap<DocId, JsonValue>,
}

pub(super) struct ParsedToml {
    pub(super) edit: ImDocument<String>,
    pub(super) plain: TomlValue,
}

pub(super) fn item_location(document: &SourceDocument, item: &Item) -> DiagnosticLocation {
    item.span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

pub(super) fn table_location(document: &SourceDocument, table: &Table) -> DiagnosticLocation {
    table
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

pub(super) fn value_location(document: &SourceDocument, value: &EditValue) -> DiagnosticLocation {
    value
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

pub(super) fn read_error_diagnostic(document: &SourceDocument, read_error: &str) -> LintDiagnostic {
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        document.document_location(),
        format!("failed to read {}: {read_error}", document.path),
    )
}

pub(super) fn toml_edit_parse_diagnostic(
    document: &SourceDocument,
    err: &toml_edit::TomlError,
) -> LintDiagnostic {
    let location = err
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location());
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        location,
        format!("failed to parse {}: {err}", document.path),
    )
}

pub(super) fn toml_de_parse_diagnostic(
    document: &SourceDocument,
    err: &toml::de::Error,
) -> LintDiagnostic {
    let location = err
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location());
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        location,
        format!("failed to parse {}: {err}", document.path),
    )
}

pub(super) fn json_parse_diagnostic(
    document: &SourceDocument,
    err: &serde_json::Error,
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
