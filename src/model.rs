use std::path::PathBuf;

use crate::diagnostics::{
    DiagnosticCatalogEntry, DiagnosticLocation, DocId, LintDiagnostic, SemanticTarget, Severity,
};

#[derive(Debug)]
pub struct WorkspaceInspection {
    pub root: PathBuf,
    pub schemas: Vec<SchemaInspection>,
    pub resources: Vec<ResourceInspection>,
    pub qualifiers: Vec<QualifierInspection>,
    pub variables: Vec<VariableInspection>,
    pub linters: Vec<LinterInspection>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SchemaInspection {
    pub id: String,
    pub path: PathBuf,
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

#[derive(Clone, Debug)]
pub struct ResourceInspection {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LinterInspection {
    pub id: String,
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
pub struct ResourceConfig {
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

    pub fn diagnostics_by_document(&self) -> Vec<DocumentDiagnostics<'_>> {
        self.documents
            .iter()
            .map(|document| DocumentDiagnostics {
                document,
                diagnostics: self.diagnostics_for_doc(document.id).collect(),
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct DocumentDiagnostics<'a> {
    pub document: &'a SourceDocumentSummary,
    pub diagnostics: Vec<&'a LintDiagnostic>,
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
    Resource,
    ResourceObject,
    Schema,
    CustomLint,
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

#[derive(Debug)]
pub struct ResourceLint {
    pub root: PathBuf,
    pub id: String,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct QualifierResolution {
    pub id: String,
    pub value: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct VariableResolution {
    pub id: String,
    pub value_key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, serde::Serialize)]
pub struct WorkspaceDiff {
    pub before: String,
    pub after: String,
    pub changes: Vec<SemanticChange>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resolution_impacts: Vec<ResolutionImpact>,
}

#[derive(Debug, serde::Serialize)]
pub struct SemanticChange {
    pub kind: String,
    pub target: SemanticTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_location: Option<DiagnosticLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_location: Option<DiagnosticLocation>,
}

#[derive(Debug, serde::Serialize)]
pub struct ResolutionImpact {
    pub variable: String,
    pub before: VariableResolution,
    pub after: VariableResolution,
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

#[derive(Clone, Debug, Default)]
pub struct WorkspaceInspectRequest {
    pub variables: InspectSelection,
    pub resources: InspectSelection,
    pub qualifiers: InspectSelection,
    pub lint_rules: InspectSelection,
    pub lint_authorities: InspectSelection,
    pub linters: InspectSelection,
    pub context: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default)]
pub enum InspectSelection {
    #[default]
    None,
    Some(Vec<String>),
    All,
}

impl InspectSelection {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn is_some_or_all(&self) -> bool {
        !self.is_none()
    }

    pub fn explicit_values(&self) -> &[String] {
        match self {
            Self::Some(values) => values,
            Self::None | Self::All => &[],
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct WorkspaceInspectReport {
    pub workspace: String,
    pub documents: Vec<SourceDocumentSummary>,
    pub runtime: InspectRuntimeStatus,
    pub diagnostics: Vec<LintDiagnostic>,
    pub schemas: Vec<SchemaInspectReport>,
    pub resources: Vec<ResourceInspectReport>,
    pub variables: Vec<VariableInspectReport>,
    pub qualifiers: Vec<QualifierInspectReport>,
    pub lint_rules: Vec<LintRuleInspectReport>,
    pub lint_authorities: Vec<LintAuthorityInspectReport>,
    pub linters: Vec<LinterInspectReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct ResourceInspectReport {
    pub id: String,
    pub uri: String,
    pub path: String,
    pub schema: Option<String>,
    pub objects: Vec<ResourceObjectInspectReport>,
    pub dependencies: DependencyInspectReport,
    pub consumers: Vec<ReferenceInspectReport>,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
pub struct ResourceObjectInspectReport {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InspectRuntimeStatus {
    Available,
    Unavailable { reason: String },
}

#[derive(Debug, serde::Serialize)]
pub struct VariableInspectReport {
    pub id: String,
    pub uri: String,
    pub path: String,
    pub type_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub values: Vec<ValueInspectReport>,
    pub resolve: ResolveInspectReport,
    pub dependencies: DependencyInspectReport,
    pub diagnostics: Vec<LintDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<VariableResolutionTrace>,
}

#[derive(Debug, serde::Serialize)]
pub struct ValueInspectReport {
    pub key: String,
    pub origin: String,
    pub value: serde_json::Value,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Debug, serde::Serialize)]
pub struct ResolveInspectReport {
    pub default_value: Option<String>,
    pub rules: Vec<RulePathwayInspectReport>,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Debug, serde::Serialize)]
pub struct RulePathwayInspectReport {
    pub index: usize,
    pub qualifier: Option<String>,
    pub value: Option<String>,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Default, Debug, serde::Serialize)]
pub struct DependencyInspectReport {
    pub qualifiers: Vec<String>,
    pub context_paths: Vec<String>,
    pub schemas: Vec<String>,
    pub resources: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct QualifierInspectReport {
    pub id: String,
    pub uri: String,
    pub path: String,
    pub predicates: Vec<PredicateInspectReport>,
    pub dependencies: DependencyInspectReport,
    pub consumers: Vec<ReferenceInspectReport>,
    pub diagnostics: Vec<LintDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<QualifierResolutionTrace>,
}

#[derive(Debug, serde::Serialize)]
pub struct SchemaInspectReport {
    pub id: String,
    pub path: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub consumers: Vec<ReferenceInspectReport>,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
pub struct PredicateInspectReport {
    pub index: usize,
    pub attribute: Option<String>,
    pub op: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Vec<i64>>,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Debug, serde::Serialize)]
pub struct ReferenceInspectReport {
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Debug, serde::Serialize)]
pub struct LintRuleInspectReport {
    pub rule: String,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    pub title: String,
    pub help: String,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
pub struct LintAuthorityInspectReport {
    pub authority: String,
    pub rules: Vec<LintRuleInspectReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct LinterInspectReport {
    pub id: String,
    pub path: String,
    pub registrations: Vec<LinterRegistrationInspectReport>,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
pub struct LinterRegistrationInspectReport {
    pub stage: String,
    pub entity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub rule: String,
    pub handler: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QualifierResolutionTrace {
    pub id: String,
    pub value: bool,
    pub predicates: Vec<PredicateResolutionTrace>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PredicateResolutionTrace {
    pub index: usize,
    pub kind: String,
    pub attribute: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<BucketResolutionTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    pub result: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BucketResolutionTrace {
    pub salt: String,
    pub start: i64,
    pub end: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<u16>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VariableResolutionTrace {
    pub resolution: VariableResolution,
    pub default_value: String,
    pub rules: Vec<VariableRuleResolutionTrace>,
    pub qualifier_traces: Vec<QualifierResolutionTrace>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VariableRuleResolutionTrace {
    pub index: usize,
    pub qualifier: String,
    pub value: String,
    pub matched: bool,
}
