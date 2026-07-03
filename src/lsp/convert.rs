use std::collections::BTreeMap;

use crate::diagnostics::{DiagnosticLocation, LintDiagnostic, Severity, SourceRange};
use crate::lint::{
    PackageCompletionItem, PackageCompletionItemKind, PackageDefinition, PackageDocumentSymbol,
    PackageDocumentSymbolKind, PackageHover, PackageReference,
};
use crate::model::PackageLint;

use super::protocol::{
    LspCompletionItem, LspDiagnostic, LspDiagnosticData, LspDiagnosticRelatedInformation,
    LspDocumentSymbol, LspHover, LspLocation, LspMarkupContent, LspPosition, LspRange, LspTextEdit,
    PublishDiagnosticsParams,
};

pub(super) fn publish_diagnostics_params(lint: &PackageLint) -> Vec<PublishDiagnosticsParams> {
    let uri_by_path = lint
        .documents
        .iter()
        .map(|document| (document.path.clone(), document.uri.clone()))
        .collect::<BTreeMap<_, _>>();
    lint.diagnostics_by_document()
        .into_iter()
        .map(|group| PublishDiagnosticsParams {
            uri: group.document.uri.clone(),
            version: group.document.version,
            diagnostics: group
                .diagnostics
                .into_iter()
                .map(|diagnostic| lsp_diagnostic(diagnostic, &uri_by_path))
                .collect(),
        })
        .collect()
}

pub(super) fn lsp_diagnostic(
    diagnostic: &LintDiagnostic,
    uri_by_path: &BTreeMap<String, String>,
) -> LspDiagnostic {
    LspDiagnostic {
        range: lsp_range(&diagnostic.primary),
        severity: lsp_severity(diagnostic.severity),
        source: "rototo",
        code: diagnostic.rule.as_string(),
        message: diagnostic.message.clone(),
        related_information: diagnostic
            .related
            .iter()
            .map(|related| LspDiagnosticRelatedInformation {
                location: LspLocation {
                    uri: uri_by_path
                        .get(&related.location.path)
                        .cloned()
                        .unwrap_or_else(|| related.location.path.clone()),
                    range: lsp_range(&related.location),
                },
                message: related.message.clone(),
            })
            .collect(),
        data: LspDiagnosticData {
            rule: diagnostic.rule.as_string(),
            stage: lint_stage_label(diagnostic.stage).to_owned(),
            help: diagnostic.help.clone(),
        },
    }
}

pub(super) fn lsp_document_symbol(symbol: &PackageDocumentSymbol) -> LspDocumentSymbol {
    LspDocumentSymbol {
        name: symbol.name.clone(),
        kind: lsp_symbol_kind(symbol.kind),
        range: lsp_range(&symbol.location),
        selection_range: lsp_range(&symbol.selection_location),
        children: symbol.children.iter().map(lsp_document_symbol).collect(),
    }
}

pub(super) fn lsp_completion_item(item: &PackageCompletionItem) -> LspCompletionItem {
    // An explicit textEdit makes the editor replace the exact token under the
    // cursor instead of guessing a word boundary, and filterText lets it match
    // what the user typed against the human-readable label.
    let (text_edit, filter_text) = match item.replace {
        Some(range) => {
            let new_text = item
                .insert_text
                .clone()
                .unwrap_or_else(|| item.label.clone());
            let text_edit = LspTextEdit {
                range: lsp_range_from_source(range),
                new_text,
            };
            (Some(text_edit), Some(item.label.clone()))
        }
        None => (None, None),
    };
    LspCompletionItem {
        label: item.label.clone(),
        kind: lsp_completion_item_kind(item.kind),
        detail: item.detail,
        insert_text: item.insert_text.clone(),
        text_edit,
        filter_text,
    }
}

pub(super) fn lsp_hover(hover: PackageHover) -> LspHover {
    LspHover {
        range: hover.location.range.map(lsp_range_from_source),
        contents: LspMarkupContent {
            kind: "markdown",
            value: hover.contents,
        },
    }
}

pub(super) fn lsp_location(definition: PackageDefinition) -> LspLocation {
    LspLocation {
        uri: definition.uri,
        range: lsp_range(&definition.location),
    }
}

pub(super) fn lsp_reference(reference: &PackageReference) -> LspLocation {
    LspLocation {
        uri: reference.uri.clone(),
        range: lsp_range(&reference.location),
    }
}

fn lsp_symbol_kind(kind: PackageDocumentSymbolKind) -> u8 {
    match kind {
        PackageDocumentSymbolKind::PackageExtends => 18,
        PackageDocumentSymbolKind::PackageExtendSource => 15,
        PackageDocumentSymbolKind::Variable => 13,
        PackageDocumentSymbolKind::Catalog => 13,
        PackageDocumentSymbolKind::CatalogEntry => 14,
        PackageDocumentSymbolKind::Values => 18,
        PackageDocumentSymbolKind::Value => 14,
        PackageDocumentSymbolKind::Resolve => 3,
        PackageDocumentSymbolKind::Rule => 8,
    }
}

pub(super) fn lsp_completion_item_kind(kind: PackageCompletionItemKind) -> u8 {
    match kind {
        PackageCompletionItemKind::Variable => 18,
        PackageCompletionItemKind::Value => 12,
        PackageCompletionItemKind::FieldSelector => 5,
        PackageCompletionItemKind::Function => 3,
        PackageCompletionItemKind::Operator => 24,
    }
}

fn lsp_range(location: &DiagnosticLocation) -> LspRange {
    location
        .range
        .map_or_else(zero_lsp_range, lsp_range_from_source)
}

fn lsp_range_from_source(range: SourceRange) -> LspRange {
    LspRange {
        start: LspPosition {
            line: range.start.line,
            character: range.start.character,
        },
        end: LspPosition {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

fn zero_lsp_range() -> LspRange {
    LspRange {
        start: LspPosition {
            line: 0,
            character: 0,
        },
        end: LspPosition {
            line: 0,
            character: 0,
        },
    }
}

fn lsp_severity(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 1,
        Severity::Warning => 2,
    }
}

fn lint_stage_label(stage: crate::diagnostics::LintStage) -> &'static str {
    match stage {
        crate::diagnostics::LintStage::Discover => "discover",
        crate::diagnostics::LintStage::Parse => "parse",
        crate::diagnostics::LintStage::Project => "project",
        crate::diagnostics::LintStage::Register => "register",
        crate::diagnostics::LintStage::Reference => "reference",
        crate::diagnostics::LintStage::Value => "value",
        crate::diagnostics::LintStage::Graph => "graph",
        crate::diagnostics::LintStage::Policy => "policy",
    }
}
