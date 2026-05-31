use std::path::Path;

use crate::diagnostics::{EntityId, LintDiagnostic, SourcePosition};
use crate::error::{Result, RototoError};
use crate::model::{QualifierLint, VariableLint, WorkspaceLint};

mod builtins;
mod custom;
mod engine;
pub(crate) mod input;
mod nodes;
mod output;
mod project;
mod source;
mod stages;
mod symbols;
mod syntax;

pub(crate) use input::{LintInput, OverlayDocument};
use nodes::*;
pub(crate) use symbols::{
    WorkspaceCompletionItem, WorkspaceCompletionItemKind, WorkspaceDefinition,
    WorkspaceDocumentSymbol, WorkspaceDocumentSymbolKind, WorkspaceHover, WorkspaceReference,
};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
pub async fn lint_workspace(workspace_root: &Path) -> Result<WorkspaceLint> {
    lint_workspace_with_input(LintInput::new(workspace_root.to_path_buf())).await
}

pub async fn lint_qualifier(workspace_root: &Path, id: &str) -> Result<QualifierLint> {
    let lint = lint_workspace(workspace_root).await?;
    let path = format!("qualifiers/{id}.toml");
    if !lint.documents.iter().any(|document| document.path == path) {
        return Err(RototoError::new(format!(
            "qualifier not found: qualifier://{id}"
        )));
    }

    Ok(QualifierLint {
        root: lint.root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic_belongs_to_qualifier(diagnostic, id, &path))
            .collect(),
    })
}

pub async fn lint_variable(workspace_root: &Path, id: &str) -> Result<VariableLint> {
    let lint = lint_workspace(workspace_root).await?;
    let path = format!("variables/{id}.toml");
    if !lint.documents.iter().any(|document| document.path == path) {
        return Err(RototoError::new(format!(
            "variable not found: variable://{id}"
        )));
    }

    Ok(VariableLint {
        root: lint.root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic_belongs_to_variable(diagnostic, id, &path))
            .collect(),
    })
}

fn diagnostic_belongs_to_qualifier(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    matches!(&diagnostic.entity, EntityId::Qualifier { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Predicate { qualifier, .. } if qualifier == id)
        || diagnostic.primary.path == path
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    matches!(&diagnostic.entity, EntityId::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.entity, EntityId::EnvironmentBlock { variable, .. } if variable == id)
        || matches!(&diagnostic.entity, EntityId::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == path
}

pub(crate) async fn lint_workspace_with_input(input: LintInput) -> Result<WorkspaceLint> {
    Ok(lint_workspace_snapshot(input).await?.lint)
}

pub(crate) async fn lint_workspace_snapshot(input: LintInput) -> Result<WorkspaceLintSnapshot> {
    engine::lint_workspace_snapshot(input).await
}

pub(crate) struct WorkspaceLintSnapshot {
    pub(crate) lint: WorkspaceLint,
    index: SemanticIndex,
}

impl WorkspaceLintSnapshot {
    pub(crate) fn document_symbols(&self, path: &str) -> Vec<WorkspaceDocumentSymbol> {
        symbols::document_symbols(&self.index, path)
    }

    pub(crate) fn completion_items(&self, path: &str) -> Vec<WorkspaceCompletionItem> {
        symbols::completion_items(&self.index, path)
    }

    pub(crate) fn hover(&self, path: &str, position: SourcePosition) -> Option<WorkspaceHover> {
        symbols::hover(self, path, position)
    }

    pub(crate) fn definition(
        &self,
        path: &str,
        position: SourcePosition,
    ) -> Option<WorkspaceDefinition> {
        symbols::definition(self, path, position)
    }

    pub(crate) fn references(
        &self,
        path: &str,
        position: SourcePosition,
        include_declaration: bool,
    ) -> Vec<WorkspaceReference> {
        symbols::references(self, path, position, include_declaration)
    }
}
