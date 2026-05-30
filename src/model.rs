use std::path::PathBuf;

use crate::diagnostics::{Diagnostic, DiagnosticCatalogEntry};

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
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct QualifierLint {
    pub root: PathBuf,
    pub id: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct VariableLint {
    pub root: PathBuf,
    pub id: String,
    pub diagnostics: Vec<Diagnostic>,
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
