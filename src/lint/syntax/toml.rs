use crate::diagnostics::LintDiagnostic;

use super::super::source::SourceDocument;
use super::diagnostics::toml_span_parse_diagnostic;
use super::{ParsedToml, SyntaxIndex};

pub(super) fn parse_toml_document(
    document: &SourceDocument,
    syntax: &mut SyntaxIndex,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    match ::toml_span::parse(&document.text) {
        Ok(value) => {
            syntax.toml.insert(document.id, ParsedToml::new(value));
        }
        Err(err) => {
            diagnostics.push(toml_span_parse_diagnostic(document, &err));
        }
    }
}
