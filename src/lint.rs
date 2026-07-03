use std::collections::BTreeMap;
use std::path::Path;

use crate::diagnostics::{
    DiagnosticCatalogEntry, LintDiagnostic, RototoRuleId, SemanticEntity, SourcePosition,
};
use crate::error::{Result, RototoError};
use crate::model::{CatalogLint, PackageDiff, PackageLint, VariableLint};

mod builtins;
mod catalog_schema;
mod custom;
mod diff;
mod engine;
mod evaluation_context;
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

pub(crate) use evaluation_context::EvaluationContextCompatibility;
use index::*;
pub(crate) use input::{LintInput, OverlayDocument};
pub(crate) use inspect::inspect_snapshot;
use references::ReferenceIndex;
pub(crate) use runtime::{
    RuntimeCatalogQuery, RuntimePackage, RuntimeRule, RuntimeRuleSelection, RuntimeSelectedValue,
    compile_runtime_package, compile_runtime_package_from_snapshot,
};
pub use semantic_model::{
    CatalogEntryModel, CatalogModel, DeclarationModel, EvaluationContextModel,
    EvaluationContextSampleModel, LinterModel, LinterRuleModel, ModelEntityRef, ModelField,
    ModelLocation, ModelReferenceVia, ModelValueField, PackageSemanticModel, ReferenceModel,
    ResolveModel, RuleModel, ValueModel, VariableEvaluationContextModel, VariableModel,
};
pub(crate) use symbols::{
    PackageCompletionItem, PackageCompletionItemKind, PackageDefinition, PackageDocumentSymbol,
    PackageDocumentSymbolKind, PackageHover, PackageReference,
};

const PACKAGE_MANIFEST: &str = "rototo-package.toml";
/// Lints the package and projects its semantic and reference indexes into
/// the serializable model that tools consume instead of parsing files.
pub async fn package_semantic_model(package_root: &Path) -> Result<PackageSemanticModel> {
    let snapshot = lint_package_snapshot(LintInput::new(package_root.to_path_buf())).await?;
    Ok(snapshot.semantic_model())
}

pub async fn lint_package(package_root: &Path) -> Result<PackageLint> {
    lint_package_with_input(LintInput::new(package_root.to_path_buf())).await
}

pub async fn diff_packages(
    before_root: &Path,
    after_root: &Path,
    context: Option<&serde_json::Value>,
) -> Result<PackageDiff> {
    diff::diff_packages(before_root, after_root, context).await
}

pub async fn lint_variable(package_root: &Path, id: &str) -> Result<VariableLint> {
    let lint = lint_package(package_root).await?;
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

pub async fn lint_catalog(package_root: &Path, id: &str) -> Result<CatalogLint> {
    let lint = lint_package(package_root).await?;
    let path = format!("model/catalogs/{id}.schema.json");
    if !lint.documents.iter().any(|document| document.path == path) {
        return Err(RototoError::new(format!(
            "catalog not found: catalog://{id}"
        )));
    }

    Ok(CatalogLint {
        root: lint.root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic_belongs_to_catalog(diagnostic, id, &path))
            .collect(),
    })
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    matches!(&diagnostic.target.entity, SemanticEntity::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == path
}

fn diagnostic_belongs_to_catalog(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    let entries_prefix = format!("data/catalogs/{id}/");
    matches!(&diagnostic.target.entity, SemanticEntity::Catalog { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::CatalogEntry { catalog, .. } if catalog == id)
        || diagnostic.primary.path == path
        || diagnostic.primary.path.starts_with(&entries_prefix)
}

pub(crate) async fn lint_package_with_input(input: LintInput) -> Result<PackageLint> {
    Ok(lint_package_snapshot(input).await?.lint)
}

pub(crate) async fn lint_package_snapshot(input: LintInput) -> Result<PackageLintSnapshot> {
    engine::lint_package_snapshot(input).await
}

pub(crate) struct PackageLintSnapshot {
    pub(crate) lint: PackageLint,
    index: SemanticIndex,
    references: ReferenceIndex,
    source_texts: BTreeMap<String, String>,
}

impl PackageLintSnapshot {
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

    pub(crate) fn document_symbols(&self, path: &str) -> Vec<PackageDocumentSymbol> {
        symbols::document_symbols(&self.index, path)
    }

    pub(crate) fn completion_items(
        &self,
        path: &str,
        position: SourcePosition,
    ) -> Vec<PackageCompletionItem> {
        symbols::completion_items(self, path, position)
    }

    pub(crate) fn hover(&self, path: &str, position: SourcePosition) -> Option<PackageHover> {
        symbols::hover(self, path, position)
    }

    pub(crate) fn definition(
        &self,
        path: &str,
        position: SourcePosition,
    ) -> Option<PackageDefinition> {
        symbols::definition(self, path, position)
    }

    pub(crate) fn references(
        &self,
        path: &str,
        position: SourcePosition,
        include_declaration: bool,
    ) -> Vec<PackageReference> {
        symbols::references(self, path, position, include_declaration)
    }

    pub(crate) fn evaluation_context_compatibility(&self) -> EvaluationContextCompatibility {
        evaluation_context::compatibility(self)
    }

    pub(crate) fn source_text(&self, path: &str) -> Option<&str> {
        self.source_texts.get(path).map(String::as_str)
    }
}
