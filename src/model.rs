use std::path::PathBuf;

use crate::diagnostics::{DiagnosticCatalogEntry, DocId, LintDiagnostic, Severity};

#[derive(Debug)]
pub struct WorkspaceInspection {
    pub root: PathBuf,
    pub environments: Vec<String>,
    pub qualifiers: Vec<QualifierInspection>,
    pub variables: Vec<VariableInspection>,
}

#[derive(Clone, Debug)]
pub struct QualifierInspection {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct VariableInspection {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct QualifierConfig {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
    pub value: serde_json::Value,
}

#[derive(Debug)]
pub struct VariableConfig {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
    pub value: serde_json::Value,
}

#[derive(Debug)]
pub struct WorkspaceLint {
    pub root: PathBuf,
    pub documents: Vec<SourceDocumentSummary>,
    pub diagnostics: Vec<LintDiagnostic>,
}

impl WorkspaceLint {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn diagnostics_for_doc(&self, doc: DocId) -> impl Iterator<Item = &LintDiagnostic> + '_ {
        self.diagnostics
            .iter()
            .filter(move |diagnostic| diagnostic.primary.doc() == Some(doc))
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SourceDocumentSummary {
    pub id: DocId,
    pub path: String,
    pub uri: String,
    pub version: Option<i32>,
    pub kind: SourceKind,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Manifest,
    Qualifier,
    Variable,
    Schema,
    CustomLint,
    ExternalValue,
}

#[derive(Debug)]
pub struct QualifierLint {
    pub root: PathBuf,
    pub id: String,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug)]
pub struct VariableLint {
    pub root: PathBuf,
    pub id: String,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
pub struct QualifierResolution {
    pub id: String,
    pub value: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct VariableResolution {
    pub id: String,
    pub environment: String,
    pub value_key: String,
    pub value: serde_json::Value,
}

#[derive(Debug)]
pub struct DiagnosticCatalog {
    pub scope: DiagnosticCatalogScope,
    pub subject: String,
    pub diagnostics: Vec<DiagnosticCatalogEntry>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticCatalogScope {
    Global,
    Workspace,
}
