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

#[derive(Clone, Debug, serde::Serialize)]
pub struct EnumConfig {
    pub id: String,
    pub description: Option<String>,
    pub member_type: String,
    pub members: Vec<serde_json::Value>,
}

impl EnumConfig {
    /// The camelCase wire shape the SDK bindings hand to apps.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "description": self.description,
            "memberType": self.member_type,
            "members": self.members,
        })
    }
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
    Layer,
    Governance,
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
    CatalogArray {
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
    /// Change-kind-specific classification, e.g. the bucket blast radius of
    /// an allocation arms change.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
pub struct ResolutionImpact {
    pub variable: String,
    pub before: VariableResolution,
    pub after: VariableResolution,
}

/// A context for multi-context diff impact, carrying the display label the
/// caller knows it by (a sample key, a synthesized-case id).
#[derive(Clone, Debug)]
pub struct LabeledContext {
    pub label: String,
    pub context: serde_json::Value,
}

/// The two-package diff evaluated under several contexts at once: one set of
/// semantic changes, plus per-context resolution impacts. Review panels use
/// this shape so an approver sees the same edit through every context that
/// matters, with the comparison's honest scale attached.
#[derive(Debug, serde::Serialize)]
pub struct PackageDiffWithContexts {
    pub before: String,
    pub after: String,
    pub changes: Vec<SemanticChange>,
    pub context_impacts: Vec<ContextImpact>,
    /// Set when either side cannot compile; semantic changes still stand,
    /// resolution impacts cannot be computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ContextImpact {
    /// The caller's label for this context.
    pub context: String,
    pub impacts: Vec<OutcomeImpact>,
    /// How many variables were compared under this context (present on both
    /// sides, whether or not they changed).
    pub compared: usize,
}

/// One variable whose outcome differs between the two packages under one
/// context. Each side is either a resolution or the error that stopped it;
/// a side absent entirely means the variable does not exist there.
#[derive(Debug, serde::Serialize)]
pub struct OutcomeImpact {
    pub variable: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<VariableResolution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<VariableResolution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_error: Option<String>,
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
    pub method: String,
    pub default_value: Option<serde_json::Value>,
    pub rules: Vec<RulePathwayInspectReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryInspectReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation: Option<AllocationInspectReport>,
    #[serde(skip_serializing)]
    pub location: DiagnosticLocation,
}

#[derive(Debug, serde::Serialize)]
pub struct AllocationInspectReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buckets: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eligibility: Option<String>,
    pub arms: Vec<AllocationArmInspectReport>,
    pub assigns: Vec<AssignInspectReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct AllocationArmInspectReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buckets: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AssignInspectReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
pub struct QueryInspectReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct RulePathwayInspectReport {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
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
    /// The layer whose `[resolve]` block produced this resolution, when the
    /// package was composed from layers. One field, not a rule-stack walk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    /// The arm assignment behind a `method = "allocation"` resolution: which
    /// layer and allocation were consulted, whether the unit was enrolled, and
    /// which bucket and arm it landed in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation: Option<VariableAllocationTrace>,
}

/// One variable's traced resolution in a lenient batch: either the trace or
/// the error that stopped it. A variable whose rules read a context key the
/// caller did not supply fails alone instead of failing the whole batch,
/// which is what lets a package overview stay honest about partial contexts.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VariableTraceOutcome {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<VariableResolutionTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct VariableAllocationTrace {
    pub layer: String,
    pub allocation: String,
    /// False when the allocation is not running or the unit failed the
    /// eligibility gate; the variable then resolves to its default.
    pub enrolled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arm: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VariableRuleResolutionTrace {
    pub index: usize,
    pub condition: String,
    pub value: serde_json::Value,
    pub source: VariableResolutionSource,
    pub matched: bool,
}
