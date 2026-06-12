use std::path::Path;

use crate::diagnostics::{
    DiagnosticCatalogEntry, LintDiagnostic, LintStage, RototoRuleId, SemanticEntity, SourcePosition,
};
use crate::error::{Result, RototoError};
use crate::model::{QualifierLint, ResourceLint, VariableLint, WorkspaceDiff, WorkspaceLint};

mod builtins;
mod custom;
mod diff;
mod engine;
mod index;
pub(crate) mod input;
mod inspect;
mod output;
mod project;
mod references;
mod runtime;
mod semantic_model;
mod source;
mod stages;
mod symbols;
mod syntax;

use index::*;
pub(crate) use input::{LintInput, OverlayDocument};
pub(crate) use inspect::inspect_snapshot;
use references::ReferenceIndex;
pub(crate) use runtime::{
    RuntimeAttribute, RuntimeCompareOp, RuntimePredicate, RuntimeWorkspace,
    compile_runtime_workspace, compile_runtime_workspace_from_snapshot,
};
pub use semantic_model::{
    DeclarationModel, LinterModel, LinterRuleModel, ModelEntityRef, ModelField, ModelLocation,
    ModelReferenceVia, PredicateModel, QualifierModel, ReferenceModel, ResolveModel, ResourceModel,
    ResourceObjectModel, RuleModel, SchemaModel, ValueModel, VariableModel, WorkspaceSemanticModel,
};
pub(crate) use symbols::{
    WorkspaceCompletionItem, WorkspaceCompletionItemKind, WorkspaceDefinition,
    WorkspaceDocumentSymbol, WorkspaceDocumentSymbolKind, WorkspaceHover, WorkspaceReference,
};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
/// Lints the workspace and projects its semantic and reference indexes into
/// the serializable model that tools consume instead of parsing files.
pub async fn workspace_semantic_model(workspace_root: &Path) -> Result<WorkspaceSemanticModel> {
    let snapshot = lint_workspace_snapshot(LintInput::new(workspace_root.to_path_buf())).await?;
    Ok(snapshot.semantic_model())
}

pub async fn lint_workspace(workspace_root: &Path) -> Result<WorkspaceLint> {
    lint_workspace_with_input(LintInput::new(workspace_root.to_path_buf())).await
}

pub async fn diff_workspaces(
    before_root: &Path,
    after_root: &Path,
    context: Option<&serde_json::Value>,
) -> Result<WorkspaceDiff> {
    diff::diff_workspaces(before_root, after_root, context).await
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

pub async fn lint_resource(workspace_root: &Path, id: &str) -> Result<ResourceLint> {
    let lint = lint_workspace(workspace_root).await?;
    let path = format!("resources/{id}.toml");
    if !lint.documents.iter().any(|document| document.path == path) {
        return Err(RototoError::new(format!(
            "resource not found: resource://{id}"
        )));
    }

    Ok(ResourceLint {
        root: lint.root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic_belongs_to_resource(diagnostic, id, &path))
            .collect(),
    })
}

fn diagnostic_belongs_to_qualifier(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    matches!(&diagnostic.target.entity, SemanticEntity::Qualifier { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Predicate { qualifier, .. } if qualifier == id)
        || diagnostic.primary.path == path
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    matches!(&diagnostic.target.entity, SemanticEntity::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == path
}

fn diagnostic_belongs_to_resource(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    let objects_prefix = format!("resources/{id}-objects/");
    matches!(&diagnostic.target.entity, SemanticEntity::Resource { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::ResourceObject { resource, .. } if resource == id)
        || diagnostic.primary.path == path
        || diagnostic.primary.path.starts_with(&objects_prefix)
}

pub(crate) async fn lint_workspace_with_input(input: LintInput) -> Result<WorkspaceLint> {
    Ok(lint_workspace_snapshot(input).await?.lint)
}

pub(crate) async fn lint_workspace_snapshot(input: LintInput) -> Result<WorkspaceLintSnapshot> {
    engine::lint_workspace_snapshot(input).await
}

#[allow(dead_code)]
pub(crate) async fn lint_workspace_until(
    input: LintInput,
    stage: LintStage,
) -> Result<engine::LintContext> {
    engine::lint_workspace_until(input, stage).await
}

pub(crate) struct WorkspaceLintSnapshot {
    pub(crate) lint: WorkspaceLint,
    index: SemanticIndex,
    references: ReferenceIndex,
}

impl WorkspaceLintSnapshot {
    pub(crate) fn diagnostic_catalog_entries(&self) -> Vec<DiagnosticCatalogEntry> {
        let mut entries = RototoRuleId::iter()
            .map(DiagnosticCatalogEntry::from_rototo)
            .collect::<Vec<_>>();
        entries.extend(
            self.index
                .custom_lints
                .rules
                .values()
                .map(|rule| DiagnosticCatalogEntry::from_custom(&rule.definition)),
        );
        entries.sort_by(|left, right| left.rule.cmp(&right.rule));
        entries
    }

    pub(crate) fn document_symbols(&self, path: &str) -> Vec<WorkspaceDocumentSymbol> {
        symbols::document_symbols(&self.index, path)
    }

    pub(crate) fn completion_items(
        &self,
        path: &str,
        position: SourcePosition,
    ) -> Vec<WorkspaceCompletionItem> {
        symbols::completion_items(&self.index, path, position)
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
