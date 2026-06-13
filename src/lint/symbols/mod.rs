mod common;
mod completion;
mod definition;
mod document_symbols;
mod hover;
mod references;

use crate::diagnostics::DiagnosticLocation;

pub(super) use completion::completion_items;
pub(super) use definition::definition;
pub(super) use document_symbols::document_symbols;
pub(super) use hover::hover;
pub(super) use references::references;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDocumentSymbol {
    pub(crate) name: String,
    pub(crate) kind: WorkspaceDocumentSymbolKind,
    pub(crate) location: DiagnosticLocation,
    pub(crate) selection_location: DiagnosticLocation,
    pub(crate) children: Vec<WorkspaceDocumentSymbol>,
}

impl WorkspaceDocumentSymbol {
    fn new(
        name: impl Into<String>,
        kind: WorkspaceDocumentSymbolKind,
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
pub(crate) enum WorkspaceDocumentSymbolKind {
    WorkspaceExtends,
    WorkspaceExtendSource,
    Qualifier,
    Predicate,
    Variable,
    Catalog,
    CatalogEntry,
    Values,
    Value,
    Resolve,
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceCompletionItem {
    pub(crate) label: String,
    pub(crate) kind: WorkspaceCompletionItemKind,
    pub(crate) detail: &'static str,
}

impl WorkspaceCompletionItem {
    fn new(
        label: impl Into<String>,
        kind: WorkspaceCompletionItemKind,
        detail: &'static str,
    ) -> Self {
        Self {
            label: label.into(),
            kind,
            detail,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceCompletionItemKind {
    Qualifier,
    Value,
    PredicateOperator,
    FieldSelector,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceHover {
    pub(crate) contents: String,
    pub(crate) location: DiagnosticLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDefinition {
    pub(crate) uri: String,
    pub(crate) location: DiagnosticLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceReference {
    pub(crate) uri: String,
    pub(crate) location: DiagnosticLocation,
}
