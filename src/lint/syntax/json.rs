use crate::diagnostics::LintDiagnostic;

use super::super::source::SourceDocument;
use super::SyntaxIndex;
use super::diagnostics::json_parse_diagnostic;

pub(super) fn parse_json_document(
    document: &SourceDocument,
    syntax: &mut SyntaxIndex,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    match ::serde_json::from_str::<::serde_json::Value>(&document.text) {
        Ok(value) => {
            syntax.json.insert(document.id, value);
        }
        Err(err) => {
            diagnostics.push(json_parse_diagnostic(document, &err));
        }
    }
}
