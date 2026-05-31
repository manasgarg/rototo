mod diagnostics;
mod json;
mod location;
mod toml;

use std::collections::BTreeMap;

use crate::diagnostics::{DocId, LintDiagnostic};

use super::source::{DocumentKind, SourceStore};

pub(super) use location::{item_location, table_location, value_location};

#[derive(Default)]
pub(super) struct SyntaxIndex {
    pub(super) toml: BTreeMap<DocId, ParsedToml>,
    pub(super) json: BTreeMap<DocId, ::serde_json::Value>,
}

pub(super) struct ParsedToml {
    pub(super) edit: ::toml_edit::ImDocument<String>,
    pub(super) plain: ::toml::Value,
}

pub(super) fn parse_sources(
    source: &SourceStore,
    diagnostics: &mut Vec<LintDiagnostic>,
) -> SyntaxIndex {
    let mut syntax = SyntaxIndex::default();
    for document in source.documents.values() {
        if let Some(read_error) = &document.read_error {
            if !matches!(&document.kind, DocumentKind::CustomLint) {
                diagnostics.push(diagnostics::read_error_diagnostic(document, read_error));
            }
            continue;
        }

        match &document.kind {
            DocumentKind::Manifest
            | DocumentKind::Qualifier { .. }
            | DocumentKind::Variable { .. }
            | DocumentKind::ExternalValue { .. } => {
                toml::parse_toml_document(document, &mut syntax, diagnostics);
            }
            DocumentKind::Schema => {
                json::parse_json_document(document, &mut syntax, diagnostics);
            }
            DocumentKind::CustomLint => {}
        }
    }
    syntax
}
