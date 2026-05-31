use crate::diagnostics::LintDiagnostic;

use super::super::source::SourceDocument;
use super::diagnostics::{toml_de_parse_diagnostic, toml_edit_parse_diagnostic};
use super::{ParsedToml, SyntaxIndex};

pub(super) fn parse_toml_document(
    document: &SourceDocument,
    syntax: &mut SyntaxIndex,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    match ::toml_edit::ImDocument::parse(document.text.clone()) {
        Ok(edit) => match document.text.parse::<::toml::Value>() {
            Ok(plain) => {
                syntax.toml.insert(document.id, ParsedToml { edit, plain });
            }
            Err(err) => {
                diagnostics.push(toml_de_parse_diagnostic(document, &err));
            }
        },
        Err(err) => {
            diagnostics.push(toml_edit_parse_diagnostic(document, &err));
        }
    }
}
