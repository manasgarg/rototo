use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{DiagnosticCatalogEntry, LintDiagnostic, RototoRuleId, SemanticEntity};
use crate::error::{Result, RototoError};
use crate::expression::{ContextScalarType, Expression};
use crate::model::{
    AllocationArmInspectReport, AllocationInspectReport, AssignInspectReport,
    CatalogEntryInspectReport, CatalogInspectReport, ContextAttributeDeclarationReport,
    ContextAttributeInspectReport, DependencyInspectReport, EvaluationContextInspectReport,
    EvaluationContextSampleInspectReport, InspectRuntimeStatus, InspectSelection,
    LintAuthorityInspectReport, LintRuleInspectReport, LinterInspectReport,
    LinterRegistrationInspectReport, PackageInspectReport, PackageInspectRequest,
    QueryInspectReport, ReferenceInspectReport, ResolveInspectReport, RulePathwayInspectReport,
    RuleSampleCoverageReport, ValueInspectReport, VariableInspectReport,
    VariableSampleCoverageReport,
};
use crate::resolve::trace_variable_unchecked;

use super::evaluation_context::{
    ContextPathTypeFit, context_path_declaration, context_path_type_fit, variable_resolve_rules,
};
use super::index::*;
use super::references::{ReferenceSource, ReferenceTarget};
use super::{PackageLintSnapshot, RuntimePackage};

mod catalog;
mod context;
mod request;
mod variable;

use catalog::*;
use context::*;
use request::*;
use variable::*;

pub(crate) async fn inspect_snapshot(
    snapshot: &PackageLintSnapshot,
    runtime: Option<&RuntimePackage>,
    runtime_error: Option<String>,
    request: &PackageInspectRequest,
) -> Result<PackageInspectReport> {
    validate_context_request(request)?;

    let catalog = catalog_from_snapshot(snapshot);
    validate_request(snapshot, request, &catalog)?;

    let inventory = request.variables.is_none()
        && request.catalogs.is_none()
        && request.lint_rules.is_none()
        && request.lint_authorities.is_none()
        && request.linters.is_none();
    let variable_ids = selected_ids(
        &request.variables,
        snapshot.index.variables.keys().map(String::as_str),
        inventory,
    );
    let catalog_ids = selected_ids(
        &request.catalogs,
        snapshot.index.catalogs.keys().map(String::as_str),
        inventory,
    );

    let mut variables = Vec::new();
    for id in variable_ids {
        variables.push(inspect_variable(snapshot, runtime, request, &id).await?);
    }

    let mut catalogs = Vec::new();
    for id in catalog_ids {
        catalogs.push(inspect_catalog(snapshot, &id)?);
    }

    let evaluation_contexts = selected_evaluation_contexts(snapshot, inventory);
    let lint_rules = selected_lint_rules(snapshot, request, &catalog);
    let lint_authorities = selected_lint_authorities(snapshot, request, &catalog, inventory);
    let linters = selected_linters(snapshot, request, inventory);
    let diagnostics = selected_diagnostics(snapshot, request, inventory);

    Ok(PackageInspectReport {
        package: snapshot.lint.root.display().to_string(),
        documents: snapshot.lint.documents.clone(),
        runtime: match runtime_error {
            Some(reason) => InspectRuntimeStatus::Unavailable { reason },
            None => InspectRuntimeStatus::Available,
        },
        diagnostics,
        evaluation_contexts,
        catalogs,
        variables,
        lint_rules,
        lint_authorities,
        linters,
    })
}

pub(super) fn document_uri_path(
    snapshot: &PackageLintSnapshot,
    doc: crate::diagnostics::DocId,
) -> (String, String) {
    snapshot
        .lint
        .documents
        .iter()
        .find(|document| document.id == doc)
        .map(|document| (document.uri.clone(), document.path.clone()))
        .unwrap_or_else(|| (String::new(), String::new()))
}

pub(super) fn present_string_value(field: &ProjectField<String>) -> Option<String> {
    match field {
        ProjectField::Present(value) => Some(value.value.clone()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

pub(super) fn present_json_value(
    field: &ProjectField<serde_json::Value>,
) -> Option<serde_json::Value> {
    match field {
        ProjectField::Present(value) => Some(value.value.clone()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

pub(super) fn present_expression_source(
    field: &Option<ProjectField<Expression>>,
) -> Option<String> {
    match field {
        Some(ProjectField::Present(value)) => Some(value.value.source().to_owned()),
        Some(ProjectField::Invalid { .. } | ProjectField::Missing { .. }) | None => None,
    }
}
