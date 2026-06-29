mod common;
mod completion;
mod definition;
mod document_symbols;
mod hover;
mod references;

use crate::diagnostics::{DiagnosticLocation, SourceRange};

pub(super) use completion::completion_items;
pub(super) use definition::definition;
pub(super) use document_symbols::document_symbols;
pub(super) use hover::hover;
pub(super) use references::references;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageDocumentSymbol {
    pub(crate) name: String,
    pub(crate) kind: PackageDocumentSymbolKind,
    pub(crate) location: DiagnosticLocation,
    pub(crate) selection_location: DiagnosticLocation,
    pub(crate) children: Vec<PackageDocumentSymbol>,
}

impl PackageDocumentSymbol {
    fn new(
        name: impl Into<String>,
        kind: PackageDocumentSymbolKind,
        location: DiagnosticLocation,
        children: Vec<Self>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            selection_location: location.clone(),
            location,
            children,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageDocumentSymbolKind {
    PackageExtends,
    PackageExtendSource,
    Qualifier,
    Variable,
    Catalog,
    CatalogEntry,
    Values,
    Value,
    Resolve,
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageCompletionItem {
    pub(crate) label: String,
    pub(crate) kind: PackageCompletionItemKind,
    pub(crate) detail: &'static str,
    pub(crate) insert_text: Option<String>,
    /// Source span the completion replaces (the token under the cursor). The LSP
    /// layer turns this into an explicit `textEdit` so the editor does not guess a
    /// word boundary, which it gets wrong for tokens containing `.`, `"`, `[`, or
    /// `&`.
    pub(crate) replace: Option<SourceRange>,
}

impl PackageCompletionItem {
    fn new(
        label: impl Into<String>,
        kind: PackageCompletionItemKind,
        detail: &'static str,
    ) -> Self {
        Self {
            label: label.into(),
            kind,
            detail,
            insert_text: None,
            replace: None,
        }
    }

    fn with_insert_text(mut self, insert_text: impl Into<String>) -> Self {
        self.insert_text = Some(insert_text.into());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageCompletionItemKind {
    Qualifier,
    Value,
    FieldSelector,
    Function,
    Operator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageHover {
    pub(crate) contents: String,
    pub(crate) location: DiagnosticLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageDefinition {
    pub(crate) uri: String,
    pub(crate) location: DiagnosticLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageReference {
    pub(crate) uri: String,
    pub(crate) location: DiagnosticLocation,
}
