use std::path::PathBuf;

use crate::diagnostics::{
    DiagnosticCatalogEntry, DiagnosticLocation, DocId, LintDiagnostic, SemanticTarget, Severity,
};

#[derive(Debug)]
pub struct PackageInspection {
    pub root: PathBuf,
    pub evaluation_contexts: Vec<EvaluationContextInspection>,
    pub catalogs: Vec<CatalogInspection>,
    pub variables: Vec<VariableInspection>,
    pub linters: Vec<LinterInspection>,
}

#[derive(Clone, Debug)]
pub struct VariableInspection {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CatalogInspection {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct EvaluationContextInspection {
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
pub struct VariableConfig {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
    pub value: serde_json::Value,
}

#[derive(Debug)]
pub struct CatalogConfig {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
    pub value: serde_json::Value,
}

#[derive(Debug)]
pub struct PackageLint {
    pub root: PathBuf,
    pub documents: Vec<SourceDocumentSummary>,
    pub diagnostics: Vec<LintDiagnostic>,
}

impl PackageLint {
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
    Variable,
    Enum,
    EnumMembers,
    Catalog,
    CatalogEntry,
    EvaluationContext,
    EvaluationContextSample,
    CustomLint,
}

#[derive(Debug)]
pub struct VariableLint {
    pub root: PathBuf,
    pub id: String,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug)]
pub struct CatalogLint {
    pub root: PathBuf,
    pub id: String,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct VariableResolution {
    pub id: String,
    pub value: serde_json::Value,
    pub source: VariableResolutionSource,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum VariableResolutionSource {
    Literal,
    Catalog {
        catalog: String,
        value: String,
    },
    CatalogList {
        catalog: String,
        values: Vec<String>,
    },
}

#[derive(Debug, serde::Serialize)]
pub struct PackageDiff {
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
    Package,
}

#[derive(Clone, Debug, Default)]
pub struct PackageInspectRequest {
    pub variables: InspectSelection,
    pub catalogs: InspectSelection,
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
pub struct PackageInspectReport {
    pub package: String,
    pub documents: Vec<SourceDocumentSummary>,
    pub runtime: InspectRuntimeStatus,
    pub diagnostics: Vec<LintDiagnostic>,
    pub evaluation_contexts: Vec<EvaluationContextInspectReport>,
    pub catalogs: Vec<CatalogInspectReport>,
    pub variables: Vec<VariableInspectReport>,
    pub lint_rules: Vec<LintRuleInspectReport>,
    pub lint_authorities: Vec<LintAuthorityInspectReport>,
    pub linters: Vec<LinterInspectReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct CatalogInspectReport {
    pub id: String,
    pub uri: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub schema: Option<String>,
    pub entries: Vec<CatalogEntryInspectReport>,
    pub dependencies: DependencyInspectReport,
    pub consumers: Vec<ReferenceInspectReport>,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
pub struct CatalogEntryInspectReport {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub evaluation_contexts: Vec<String>,
    pub context_attributes: Vec<ContextAttributeInspectReport>,
    pub type_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub values: Vec<ValueInspectReport>,
    pub resolve: ResolveInspectReport,
    pub dependencies: DependencyInspectReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_coverage: Option<VariableSampleCoverageReport>,
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
    pub default_value: Option<serde_json::Value>,
    pub rules: Vec<RulePathwayInspectReport>,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Debug, serde::Serialize)]
pub struct RulePathwayInspectReport {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Default, Debug, serde::Serialize)]
pub struct DependencyInspectReport {
    pub variables: Vec<String>,
    pub context_paths: Vec<String>,
    pub catalogs: Vec<String>,
}

/// How a single context attribute used by a variable lines up with
/// the evaluation context schemas: the scalar types the expression expects of
/// it, where it is declared and with what type, and whether that agrees.
#[derive(Debug, serde::Serialize)]
pub struct ContextAttributeInspectReport {
    pub path: String,
    /// Scalar types the expression requires, inferred from how the path is used.
    /// Empty when the use does not pin a scalar type (for example a `bucket`
    /// value argument).
    pub expected_types: Vec<String>,
    /// One of `ok`, `undeclared`, or `type_mismatch`.
    pub status: String,
    pub declarations: Vec<ContextAttributeDeclarationReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct ContextAttributeDeclarationReport {
    pub evaluation_context: String,
    /// The JSON Schema type tokens the context declares for this path. Empty
    /// when the path is declared without a checkable type.
    pub declared_types: Vec<String>,
}

/// Which resolution branches of a variable the available evaluation context
/// samples actually exercise. A rule (or the default) with `covered = false` is
/// an opportunity: add a sample that selects it.
#[derive(Debug, serde::Serialize)]
pub struct VariableSampleCoverageReport {
    pub sample_count: usize,
    pub default_covered: bool,
    pub rules: Vec<RuleSampleCoverageReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct RuleSampleCoverageReport {
    pub index: usize,
    pub covered: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct EvaluationContextInspectReport {
    pub id: String,
    pub path: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub samples: Vec<EvaluationContextSampleInspectReport>,
    pub diagnostics: Vec<LintDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
pub struct EvaluationContextSampleInspectReport {
    pub key: String,
    pub value: serde_json::Value,
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
    pub target: String,
    pub rule: String,
    pub handler: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VariableResolutionTrace {
    pub resolution: VariableResolution,
    pub default_value: serde_json::Value,
    pub default_source: VariableResolutionSource,
    pub rules: Vec<VariableRuleResolutionTrace>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VariableRuleResolutionTrace {
    pub index: usize,
    pub condition: String,
    pub value: serde_json::Value,
    pub source: VariableResolutionSource,
    pub matched: bool,
}
