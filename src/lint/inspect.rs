use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{DiagnosticCatalogEntry, LintDiagnostic, RototoRuleId, SemanticEntity};
use crate::error::{Result, RototoError};
use crate::expression::Expression;
use crate::model::{
    CatalogEntryInspectReport, CatalogInspectReport, DependencyInspectReport, InspectRuntimeStatus,
    InspectSelection, LintAuthorityInspectReport, LintRuleInspectReport, LinterInspectReport,
    LinterRegistrationInspectReport, QualifierInspectReport, ReferenceInspectReport,
    RequestContextEntryInspectReport, RequestContextInspectReport, ResolveInspectReport,
    RulePathwayInspectReport, ValueInspectReport, VariableInspectReport, WorkspaceInspectReport,
    WorkspaceInspectRequest,
};
use crate::resolve::{trace_qualifier_unchecked, trace_variable_unchecked};

use super::index::*;
use super::references::{ReferenceSource, ReferenceTarget};
use super::{RuntimeWorkspace, WorkspaceLintSnapshot};

pub(crate) async fn inspect_snapshot(
    snapshot: &WorkspaceLintSnapshot,
    runtime: Option<&RuntimeWorkspace>,
    runtime_error: Option<String>,
    request: &WorkspaceInspectRequest,
) -> Result<WorkspaceInspectReport> {
    validate_context_request(request)?;

    let catalog = catalog_from_snapshot(snapshot);
    validate_request(snapshot, request, &catalog)?;

    let inventory = request.variables.is_none()
        && request.catalogs.is_none()
        && request.qualifiers.is_none()
        && request.lint_rules.is_none()
        && request.lint_authorities.is_none()
        && request.linters.is_none();
    let variable_ids = selected_ids(
        &request.variables,
        snapshot.index.variables.keys().map(String::as_str),
        inventory,
    );
    let qualifier_ids = selected_ids(
        &request.qualifiers,
        snapshot.index.qualifiers.keys().map(String::as_str),
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

    let mut qualifiers = Vec::new();
    for id in qualifier_ids {
        qualifiers.push(inspect_qualifier(snapshot, runtime, request, &id).await?);
    }

    let mut catalogs = Vec::new();
    for id in catalog_ids {
        catalogs.push(inspect_catalog(snapshot, &id)?);
    }

    let request_contexts = selected_request_contexts(snapshot, inventory);
    let lint_rules = selected_lint_rules(snapshot, request, &catalog);
    let lint_authorities = selected_lint_authorities(snapshot, request, &catalog, inventory);
    let linters = selected_linters(snapshot, request, inventory);
    let diagnostics = selected_diagnostics(snapshot, request, inventory);

    Ok(WorkspaceInspectReport {
        workspace: snapshot.lint.root.display().to_string(),
        documents: snapshot.lint.documents.clone(),
        runtime: match runtime_error {
            Some(reason) => InspectRuntimeStatus::Unavailable { reason },
            None => InspectRuntimeStatus::Available,
        },
        diagnostics,
        request_contexts,
        catalogs,
        variables,
        qualifiers,
        lint_rules,
        lint_authorities,
        linters,
    })
}

fn validate_context_request(request: &WorkspaceInspectRequest) -> Result<()> {
    if request.context.is_none() {
        return Ok(());
    }
    if !request.variables.is_some_or_all() && !request.qualifiers.is_some_or_all() {
        return Err(RototoError::new(
            "inspect --context requires at least one --variable, --variables, --qualifier, or --qualifiers selector",
        ));
    }
    Ok(())
}

fn validate_request(
    snapshot: &WorkspaceLintSnapshot,
    request: &WorkspaceInspectRequest,
    catalog: &[DiagnosticCatalogEntry],
) -> Result<()> {
    for id in request.variables.explicit_values() {
        if !snapshot.index.variables.contains_key(id) {
            return Err(RototoError::new(format!(
                "variable not found: variable://{id}"
            )));
        }
    }
    for id in request.qualifiers.explicit_values() {
        if !snapshot.index.qualifiers.contains_key(id) {
            return Err(RototoError::new(format!(
                "qualifier not found: qualifier://{id}"
            )));
        }
    }
    for id in request.catalogs.explicit_values() {
        if !snapshot.index.catalogs.contains_key(id) {
            return Err(RototoError::new(format!(
                "catalog not found: catalog://{id}"
            )));
        }
    }
    for rule in request.lint_rules.explicit_values() {
        diagnostic_for_rule_in_entries(catalog, rule)?;
    }

    let authorities = catalog
        .iter()
        .filter_map(|entry| authority_of(&entry.rule).map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for authority in request.lint_authorities.explicit_values() {
        if !authorities.contains(authority) {
            return Err(RototoError::new(format!(
                "lint authority not found: {authority}"
            )));
        }
    }

    let linters = snapshot
        .index
        .custom_lints
        .files
        .keys()
        .filter_map(|path| linter_id(path))
        .collect::<BTreeSet<_>>();
    for id in request.linters.explicit_values() {
        if !linters.contains(id) {
            return Err(RototoError::new(format!("linter not found: {id}")));
        }
    }

    Ok(())
}

fn selected_request_contexts(
    snapshot: &WorkspaceLintSnapshot,
    include_all_for_none: bool,
) -> Vec<RequestContextInspectReport> {
    if !include_all_for_none {
        return Vec::new();
    }
    snapshot
        .index
        .request_contexts
        .values()
        .map(|request_context| request_context_report(snapshot, request_context))
        .collect()
}

fn request_context_report(
    snapshot: &WorkspaceLintSnapshot,
    request_context: &RequestContextNode,
) -> RequestContextInspectReport {
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_request_context(diagnostic, &request_context.id))
        .cloned()
        .collect();
    let (status, error) = if let Some(message) = &request_context.invalid_message {
        ("invalid".to_owned(), Some(message.clone()))
    } else if request_context.validator.is_some() {
        ("valid".to_owned(), None)
    } else {
        ("unavailable".to_owned(), None)
    };
    let json = request_context.json.as_ref();

    RequestContextInspectReport {
        id: request_context.id.clone(),
        path: request_context.path.clone(),
        status,
        error,
        title: json
            .and_then(|json| json.get("title"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        description: json
            .and_then(|json| json.get("description"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        entries: request_context_entries(snapshot, &request_context.id),
        diagnostics,
    }
}

fn request_context_entries(
    snapshot: &WorkspaceLintSnapshot,
    request_context: &str,
) -> Vec<RequestContextEntryInspectReport> {
    snapshot
        .index
        .request_context_entries
        .get(request_context)
        .into_iter()
        .flat_map(|entries| entries.values())
        .filter_map(|entry| {
            entry
                .value
                .as_ref()
                .map(|value| RequestContextEntryInspectReport {
                    key: entry.key.clone(),
                    value: value.clone(),
                    location: entry.location.clone(),
                })
        })
        .collect()
}

async fn inspect_variable(
    snapshot: &WorkspaceLintSnapshot,
    runtime: Option<&RuntimeWorkspace>,
    request: &WorkspaceInspectRequest,
    id: &str,
) -> Result<VariableInspectReport> {
    let variable = snapshot
        .index
        .variables
        .get(id)
        .ok_or_else(|| RototoError::new(format!("variable not found: variable://{id}")))?;
    let (_source_uri, path) = document_uri_path(snapshot, variable.doc);
    let dependencies = variable_dependencies(snapshot, id);
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_variable(diagnostic, id))
        .cloned()
        .collect();
    let trace = match (runtime, &request.context) {
        (Some(runtime), Some(context)) => {
            runtime.validate_context_for_variable(id, context)?;
            Some(trace_variable_unchecked(runtime, id, context).await?)
        }
        _ => None,
    };
    let request_contexts = snapshot
        .request_context_compatibility()
        .variables
        .remove(id)
        .unwrap_or_default()
        .into_iter()
        .collect();

    Ok(VariableInspectReport {
        id: id.to_owned(),
        uri: format!("variable://{id}"),
        path,
        description: variable.description.as_ref().and_then(present_string_value),
        request_contexts,
        type_source: variable_type_source_label(variable),
        schema: variable_schema_dependency(snapshot, id),
        values: variable_values(variable, &snapshot.index),
        resolve: variable_resolve(variable),
        dependencies,
        diagnostics,
        trace,
    })
}

async fn inspect_qualifier(
    snapshot: &WorkspaceLintSnapshot,
    runtime: Option<&RuntimeWorkspace>,
    request: &WorkspaceInspectRequest,
    id: &str,
) -> Result<QualifierInspectReport> {
    let qualifier = snapshot
        .index
        .qualifiers
        .get(id)
        .ok_or_else(|| RototoError::new(format!("qualifier not found: qualifier://{id}")))?;
    let (_source_uri, path) = document_uri_path(snapshot, qualifier.doc);
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_qualifier(diagnostic, id))
        .cloned()
        .collect();
    let trace = match (runtime, &request.context) {
        (Some(runtime), Some(context)) => {
            runtime.validate_context_for_qualifier(id, context)?;
            Some(trace_qualifier_unchecked(runtime, id, context).await?)
        }
        _ => None,
    };
    let request_contexts = snapshot
        .request_context_compatibility()
        .qualifiers
        .remove(id)
        .unwrap_or_default()
        .into_iter()
        .collect();

    Ok(QualifierInspectReport {
        id: id.to_owned(),
        uri: format!("qualifier://{id}"),
        path,
        description: qualifier
            .description
            .as_ref()
            .and_then(present_string_value),
        request_contexts,
        when: qualifier_when(qualifier),
        predicates: Vec::new(),
        dependencies: qualifier_dependencies(snapshot, id),
        consumers: qualifier_consumers(snapshot, id),
        diagnostics,
        trace,
    })
}

fn inspect_catalog(snapshot: &WorkspaceLintSnapshot, id: &str) -> Result<CatalogInspectReport> {
    let catalog = snapshot
        .index
        .catalogs
        .get(id)
        .ok_or_else(|| RototoError::new(format!("catalog not found: catalog://{id}")))?;
    let (_source_uri, path) = document_uri_path(snapshot, catalog.doc);
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_catalog(diagnostic, id))
        .cloned()
        .collect();

    Ok(CatalogInspectReport {
        id: id.to_owned(),
        uri: format!("catalog://{id}"),
        path,
        description: catalog
            .json
            .as_ref()
            .and_then(|json| json.get("description"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        schema: Some(catalog.path.clone()),
        entries: catalog_entries(snapshot, id),
        dependencies: catalog_dependencies(snapshot, id),
        consumers: catalog_consumers(snapshot, id),
        diagnostics,
    })
}

fn catalog_entries(snapshot: &WorkspaceLintSnapshot, id: &str) -> Vec<CatalogEntryInspectReport> {
    snapshot
        .index
        .catalog_entries
        .get(id)
        .into_iter()
        .flat_map(|entries| entries.values())
        .map(|entry| CatalogEntryInspectReport {
            key: entry.key.clone(),
            value: entry.value.clone(),
            location: entry.location.clone(),
        })
        .collect()
}

fn document_uri_path(
    snapshot: &WorkspaceLintSnapshot,
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

fn selected_ids<'a>(
    selection: &'a InspectSelection,
    all_ids: impl Iterator<Item = &'a str>,
    include_all_for_none: bool,
) -> Vec<String> {
    match selection {
        InspectSelection::All => all_ids.map(str::to_owned).collect(),
        InspectSelection::Some(ids) => {
            let mut ordered = Vec::new();
            let requested = ids.iter().cloned().collect::<BTreeSet<_>>();
            for id in all_ids {
                if requested.contains(id) {
                    ordered.push(id.to_owned());
                }
            }
            for id in ids {
                if !ordered.iter().any(|ordered_id| ordered_id == id) {
                    ordered.push(id.clone());
                }
            }
            ordered
        }
        InspectSelection::None if include_all_for_none => all_ids.map(str::to_owned).collect(),
        InspectSelection::None => Vec::new(),
    }
}

fn variable_type_source_label(variable: &VariableNode) -> String {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => type_name.value.clone(),
        TypeSourceNode::Catalog(catalog) => format!("catalog:{}", catalog.value),
        TypeSourceNode::Schema(schema) => format!("schema {}", schema.value),
        TypeSourceNode::Missing { .. } => "missing".to_owned(),
        TypeSourceNode::Conflict { .. } => "conflict".to_owned(),
        TypeSourceNode::Invalid { .. } => "invalid".to_owned(),
    }
}

fn variable_schema_dependency(
    _snapshot: &WorkspaceLintSnapshot,
    _variable: &str,
) -> Option<String> {
    None
}

fn variable_values(variable: &VariableNode, _index: &SemanticIndex) -> Vec<ValueInspectReport> {
    let mut values = Vec::new();
    for value in variable.values.inline_values.values() {
        values.push(value_report(value));
    }
    values.sort_by(|left, right| left.key.cmp(&right.key));
    values
}

fn value_report(value: &ValueNode) -> ValueInspectReport {
    let origin = match &value.origin {
        ValueOrigin::Inline { .. } => "inline".to_owned(),
    };
    ValueInspectReport {
        key: value.key.clone(),
        origin,
        value: value.value.clone(),
        location: value.location.clone(),
    }
}

fn variable_resolve(variable: &VariableNode) -> ResolveInspectReport {
    let ResolveNode::Resolve {
        location,
        default,
        rules,
    } = &variable.resolve
    else {
        return ResolveInspectReport {
            default_value: None,
            rules: Vec::new(),
            location: variable.resolve.location(),
        };
    };
    let rules = match rules {
        RuleCollection::Rules(rules) => rules
            .iter()
            .map(|rule| RulePathwayInspectReport {
                index: rule.index,
                when: present_expression_source(&rule.when),
                query: present_expression_source(&rule.query),
                value: present_json_value(&rule.value),
                location: rule.location.clone(),
            })
            .collect(),
        RuleCollection::Invalid { .. } => Vec::new(),
    };
    ResolveInspectReport {
        default_value: present_json_value(default),
        rules,
        location: location.clone(),
    }
}

fn present_string_value(field: &ProjectField<String>) -> Option<String> {
    match field {
        ProjectField::Present(value) => Some(value.value.clone()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn present_json_value(field: &ProjectField<serde_json::Value>) -> Option<serde_json::Value> {
    match field {
        ProjectField::Present(value) => Some(value.value.clone()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn present_expression_source(field: &Option<ProjectField<Expression>>) -> Option<String> {
    match field {
        Some(ProjectField::Present(value)) => Some(value.value.source().to_owned()),
        Some(ProjectField::Invalid { .. } | ProjectField::Missing { .. }) | None => None,
    }
}

fn variable_dependencies(
    snapshot: &WorkspaceLintSnapshot,
    variable: &str,
) -> DependencyInspectReport {
    let mut qualifiers = BTreeSet::new();
    let mut context_paths = BTreeSet::new();
    let mut catalogs = BTreeSet::new();

    for edge in snapshot.references.edges() {
        match (&edge.source, &edge.target) {
            (
                ReferenceSource::VariableRuleConditionQualifier {
                    variable: source_variable,
                    ..
                },
                ReferenceTarget::Qualifier(qualifier),
            ) if source_variable == variable && edge.is_resolved() => {
                collect_qualifier_dependencies(
                    snapshot,
                    qualifier,
                    &mut qualifiers,
                    &mut context_paths,
                    &mut BTreeSet::new(),
                );
            }
            (
                ReferenceSource::VariableCatalog {
                    variable: source_variable,
                },
                ReferenceTarget::Catalog(catalog),
            ) if source_variable == variable && edge.is_resolved() => {
                catalogs.insert(catalog.clone());
            }
            _ => {}
        }
    }

    DependencyInspectReport {
        qualifiers: qualifiers.into_iter().collect(),
        context_paths: context_paths.into_iter().collect(),
        catalogs: catalogs.into_iter().collect(),
    }
}

fn catalog_dependencies(
    _snapshot: &WorkspaceLintSnapshot,
    _catalog: &str,
) -> DependencyInspectReport {
    DependencyInspectReport {
        qualifiers: Vec::new(),
        context_paths: Vec::new(),
        catalogs: Vec::new(),
    }
}

fn qualifier_dependencies(
    snapshot: &WorkspaceLintSnapshot,
    qualifier: &str,
) -> DependencyInspectReport {
    let mut qualifiers = BTreeSet::new();
    let mut context_paths = BTreeSet::new();
    collect_qualifier_dependencies(
        snapshot,
        qualifier,
        &mut qualifiers,
        &mut context_paths,
        &mut BTreeSet::new(),
    );
    qualifiers.remove(qualifier);
    DependencyInspectReport {
        qualifiers: qualifiers.into_iter().collect(),
        context_paths: context_paths.into_iter().collect(),
        catalogs: Vec::new(),
    }
}

fn collect_qualifier_dependencies(
    snapshot: &WorkspaceLintSnapshot,
    qualifier: &str,
    qualifiers: &mut BTreeSet<String>,
    context_paths: &mut BTreeSet<String>,
    seen: &mut BTreeSet<String>,
) {
    if !seen.insert(qualifier.to_owned()) {
        return;
    }
    qualifiers.insert(qualifier.to_owned());
    for edge in snapshot.references.edges() {
        match (&edge.source, &edge.target) {
            (
                ReferenceSource::QualifierWhenContextAttribute {
                    qualifier: source_qualifier,
                },
                ReferenceTarget::ContextAttribute(context_path),
            ) if source_qualifier == qualifier => {
                context_paths.insert(context_path.clone());
            }
            (
                ReferenceSource::QualifierWhenQualifier {
                    qualifier: source_qualifier,
                },
                ReferenceTarget::Qualifier(nested),
            ) if source_qualifier == qualifier && edge.is_resolved() => {
                collect_qualifier_dependencies(snapshot, nested, qualifiers, context_paths, seen);
            }
            _ => {}
        }
    }
}

fn qualifier_when(qualifier: &QualifierNode) -> Option<String> {
    match &qualifier.when {
        ProjectField::Present(when) => Some(when.value.source().to_owned()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn qualifier_consumers(
    snapshot: &WorkspaceLintSnapshot,
    qualifier: &str,
) -> Vec<ReferenceInspectReport> {
    snapshot
        .references
        .edges()
        .iter()
        .filter_map(|edge| {
            let ReferenceTarget::Qualifier(target) = &edge.target else {
                return None;
            };
            if target != qualifier {
                return None;
            }
            Some(ReferenceInspectReport {
                kind: reference_source_kind(&edge.source).to_owned(),
                label: reference_source_label(&edge.source),
                location: edge.location.clone(),
            })
        })
        .collect()
}

fn catalog_consumers(
    snapshot: &WorkspaceLintSnapshot,
    catalog: &str,
) -> Vec<ReferenceInspectReport> {
    snapshot
        .references
        .edges()
        .iter()
        .filter_map(|edge| {
            let ReferenceTarget::Catalog(target) = &edge.target else {
                return None;
            };
            if target != catalog {
                return None;
            }
            Some(ReferenceInspectReport {
                kind: reference_source_kind(&edge.source).to_owned(),
                label: reference_source_label(&edge.source),
                location: edge.location.clone(),
            })
        })
        .collect()
}

fn reference_source_kind(source: &ReferenceSource) -> &'static str {
    match source {
        ReferenceSource::QualifierWhenQualifier { .. }
        | ReferenceSource::QualifierWhenContextAttribute { .. } => "qualifier",
        ReferenceSource::VariableRuleConditionQualifier { .. }
        | ReferenceSource::VariableRuleValue { .. }
        | ReferenceSource::VariableResolveDefault { .. }
        | ReferenceSource::VariableCatalog { .. } => "variable",
    }
}

fn reference_source_label(source: &ReferenceSource) -> String {
    match source {
        ReferenceSource::QualifierWhenQualifier { qualifier }
        | ReferenceSource::QualifierWhenContextAttribute { qualifier } => {
            format!("qualifier {qualifier} when")
        }
        ReferenceSource::VariableRuleConditionQualifier { variable, rule }
        | ReferenceSource::VariableRuleValue { variable, rule } => {
            format!("variable {variable} resolve.rule[{rule}]")
        }
        ReferenceSource::VariableResolveDefault { variable } => {
            format!("variable {variable} resolve.default")
        }
        ReferenceSource::VariableCatalog { variable } => format!("variable {variable}"),
    }
}

fn selected_diagnostics(
    snapshot: &WorkspaceLintSnapshot,
    request: &WorkspaceInspectRequest,
    inventory: bool,
) -> Vec<LintDiagnostic> {
    if inventory {
        return snapshot.lint.diagnostics.clone();
    }
    snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_matches_request(diagnostic, request))
        .cloned()
        .collect()
}

fn diagnostic_matches_request(
    diagnostic: &LintDiagnostic,
    request: &WorkspaceInspectRequest,
) -> bool {
    selection_matches_variable(&request.variables, diagnostic)
        || selection_matches_catalog(&request.catalogs, diagnostic)
        || selection_matches_qualifier(&request.qualifiers, diagnostic)
        || selection_matches_lint_rule(&request.lint_rules, diagnostic)
        || selection_matches_lint_authority(&request.lint_authorities, diagnostic)
        || selection_matches_linter(&request.linters, diagnostic)
}

fn selection_matches_variable(selection: &InspectSelection, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_variable_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_variable(diagnostic, id)),
    }
}

fn selection_matches_qualifier(selection: &InspectSelection, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_qualifier_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_qualifier(diagnostic, id)),
    }
}

fn selection_matches_catalog(selection: &InspectSelection, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_catalog_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_catalog(diagnostic, id)),
    }
}

fn selection_matches_lint_rule(selection: &InspectSelection, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => true,
        InspectSelection::Some(rules) => rules.contains(&diagnostic.rule.as_string()),
    }
}

fn selection_matches_lint_authority(
    selection: &InspectSelection,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => true,
        InspectSelection::Some(authorities) => authority_of(&diagnostic.rule.as_string())
            .is_some_and(|authority| authorities.iter().any(|selected| selected == authority)),
    }
}

fn selection_matches_linter(selection: &InspectSelection, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_linter_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_linter(diagnostic, id)),
    }
}

fn diagnostic_is_variable_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Variable { .. }
            | SemanticEntity::Value { .. }
            | SemanticEntity::Rule { .. }
    ) || diagnostic.primary.path.starts_with("variables/")
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let variable_path = format!("variables/{id}.toml");
    matches!(&diagnostic.target.entity, SemanticEntity::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == variable_path
}

fn diagnostic_is_catalog_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Catalog { .. } | SemanticEntity::CatalogEntry { .. }
    ) || diagnostic.primary.path.starts_with("catalogs/")
}

fn diagnostic_belongs_to_catalog(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let catalog_path = format!("catalogs/{id}.schema.json");
    let catalog_entries_prefix = format!("catalogs/{id}-entries/");
    matches!(&diagnostic.target.entity, SemanticEntity::Catalog { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::CatalogEntry { catalog, .. } if catalog == id)
        || diagnostic.primary.path == catalog_path
        || diagnostic.primary.path.starts_with(&catalog_entries_prefix)
}

fn diagnostic_is_qualifier_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Qualifier { .. } | SemanticEntity::Predicate { .. }
    ) || diagnostic.primary.path.starts_with("qualifiers/")
}

fn diagnostic_belongs_to_qualifier(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let qualifier_path = format!("qualifiers/{id}.toml");
    matches!(&diagnostic.target.entity, SemanticEntity::Qualifier { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Predicate { qualifier, .. } if qualifier == id)
        || diagnostic.primary.path == qualifier_path
}

fn diagnostic_is_linter_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(diagnostic.target.entity, SemanticEntity::CustomLint { .. })
        || diagnostic.primary.path.starts_with("lint/")
        || authority_of(&diagnostic.rule.as_string()).is_some_and(|authority| authority != "rototo")
}

fn diagnostic_belongs_to_linter(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let path = format!("lint/{id}.lua");
    matches!(&diagnostic.target.entity, SemanticEntity::CustomLint { path: diagnostic_path } if diagnostic_path == &path)
        || diagnostic.primary.path == path
}

fn diagnostic_belongs_to_request_context(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let schema_path = format!("request-contexts/{id}.schema.json");
    let entries_prefix = format!("request-contexts/{id}-entries/");
    matches!(&diagnostic.target.entity, SemanticEntity::RequestContext { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::RequestContextEntry { request_context, .. } if request_context == id)
        || diagnostic.primary.path == schema_path
        || diagnostic.primary.path.starts_with(&entries_prefix)
}

fn selected_lint_rules(
    snapshot: &WorkspaceLintSnapshot,
    request: &WorkspaceInspectRequest,
    catalog: &[DiagnosticCatalogEntry],
) -> Vec<LintRuleInspectReport> {
    let entries = match &request.lint_rules {
        InspectSelection::None => Vec::new(),
        InspectSelection::All => catalog.iter().collect(),
        InspectSelection::Some(rules) => catalog
            .iter()
            .filter(|entry| rules.contains(&entry.rule))
            .collect(),
    };
    entries
        .into_iter()
        .map(|entry| lint_rule_report(snapshot, entry))
        .collect()
}

fn selected_lint_authorities(
    snapshot: &WorkspaceLintSnapshot,
    request: &WorkspaceInspectRequest,
    catalog: &[DiagnosticCatalogEntry],
    include_workspace_rules_for_none: bool,
) -> Vec<LintAuthorityInspectReport> {
    let (selected, workspace_rules_only) = match &request.lint_authorities {
        InspectSelection::None if include_workspace_rules_for_none => (None, true),
        InspectSelection::None => return Vec::new(),
        InspectSelection::All => (None, false),
        InspectSelection::Some(authorities) => (
            Some(authorities.iter().cloned().collect::<BTreeSet<_>>()),
            false,
        ),
    };
    let mut grouped: BTreeMap<String, Vec<LintRuleInspectReport>> = BTreeMap::new();
    for entry in catalog {
        let Some(authority) = authority_of(&entry.rule) else {
            continue;
        };
        if workspace_rules_only && authority == "rototo" {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|authorities| !authorities.contains(authority))
        {
            continue;
        }
        grouped
            .entry(authority.to_owned())
            .or_default()
            .push(lint_rule_report(snapshot, entry));
    }
    grouped
        .into_iter()
        .map(|(authority, rules)| LintAuthorityInspectReport { authority, rules })
        .collect()
}

fn lint_rule_report(
    snapshot: &WorkspaceLintSnapshot,
    entry: &DiagnosticCatalogEntry,
) -> LintRuleInspectReport {
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule.as_string() == entry.rule)
        .cloned()
        .collect();
    LintRuleInspectReport {
        rule: entry.rule.clone(),
        severity: entry.severity,
        entity: entry
            .entity
            .map(|entity| format!("{entity:?}").to_lowercase()),
        title: entry.title.clone(),
        help: entry.help.clone(),
        diagnostics,
    }
}

fn selected_linters(
    snapshot: &WorkspaceLintSnapshot,
    request: &WorkspaceInspectRequest,
    include_all_for_none: bool,
) -> Vec<LinterInspectReport> {
    let selected = match &request.linters {
        InspectSelection::None if include_all_for_none => None,
        InspectSelection::None => return Vec::new(),
        InspectSelection::All => None,
        InspectSelection::Some(ids) => Some(ids.iter().cloned().collect::<BTreeSet<_>>()),
    };
    snapshot
        .index
        .custom_lints
        .files
        .values()
        .filter_map(|file| {
            let id = linter_id(&file.path)?;
            if selected
                .as_ref()
                .is_some_and(|selected| !selected.contains(&id))
            {
                return None;
            }
            let registrations = snapshot
                .index
                .custom_lints
                .registrations
                .iter()
                .filter(|registration| registration.file_path == file.path)
                .map(|registration| LinterRegistrationInspectReport {
                    stage: format!("{:?}", registration.stage).to_lowercase(),
                    target: registered_address_label(&registration.selector.address),
                    rule: registration.rule.as_str().to_owned(),
                    handler: registration.handler.clone(),
                })
                .collect();
            let diagnostics = snapshot
                .lint
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic_belongs_to_linter(diagnostic, &id))
                .cloned()
                .collect();
            Some(LinterInspectReport {
                id,
                path: file.path.clone(),
                registrations,
                diagnostics,
            })
        })
        .collect()
}

fn catalog_from_snapshot(snapshot: &WorkspaceLintSnapshot) -> Vec<DiagnosticCatalogEntry> {
    let mut entries = RototoRuleId::iter()
        .map(DiagnosticCatalogEntry::from_rototo)
        .collect::<Vec<_>>();
    entries.extend(
        snapshot
            .index
            .custom_lints
            .rules
            .values()
            .map(|rule| DiagnosticCatalogEntry::from_custom(&rule.definition)),
    );
    entries.sort_by(|left, right| left.rule.cmp(&right.rule));
    entries
}

fn diagnostic_for_rule_in_entries<'a>(
    entries: &'a [DiagnosticCatalogEntry],
    rule: &str,
) -> Result<&'a DiagnosticCatalogEntry> {
    entries
        .iter()
        .find(|entry| entry.rule == rule)
        .ok_or_else(|| RototoError::new(format!("diagnostic not found: {rule}")))
}

fn authority_of(rule: &str) -> Option<&str> {
    rule.split_once('/').map(|(authority, _)| authority)
}

fn linter_id(path: &str) -> Option<String> {
    path.strip_prefix("lint/")
        .and_then(|path| path.strip_suffix(".lua"))
        .map(str::to_owned)
}

fn registered_address_label(address: &RegisteredLintAddress) -> String {
    match address {
        RegisteredLintAddress::Workspace => "/".to_owned(),
        RegisteredLintAddress::Qualifiers => "/qualifiers".to_owned(),
        RegisteredLintAddress::Qualifier { id } => format!("/qualifiers/{id}"),
        RegisteredLintAddress::Variables => "/variables".to_owned(),
        RegisteredLintAddress::Variable { id } => format!("/variables/{id}"),
        RegisteredLintAddress::VariableValues { variable } => {
            format!("/variables/{variable}/values")
        }
        RegisteredLintAddress::VariableValue { variable, key } => {
            format!("/variables/{variable}/values/{key}")
        }
        RegisteredLintAddress::VariableRules { variable } => {
            format!("/variables/{variable}/rules")
        }
        RegisteredLintAddress::VariableRule { variable, index } => {
            format!("/variables/{variable}/rules/{index}")
        }
        RegisteredLintAddress::Catalogs => "/catalogs".to_owned(),
        RegisteredLintAddress::Catalog { id } => format!("/catalogs/{id}"),
        RegisteredLintAddress::CatalogEntries { catalog } => {
            format!("/catalogs/{catalog}/entries")
        }
        RegisteredLintAddress::CatalogEntry { catalog, key } => {
            format!("/catalogs/{catalog}/entries/{key}")
        }
        RegisteredLintAddress::RequestContexts => "/request-contexts".to_owned(),
        RegisteredLintAddress::RequestContext { id } => format!("/request-contexts/{id}"),
        RegisteredLintAddress::RequestContextEntries { request_context } => {
            format!("/request-contexts/{request_context}/entries")
        }
        RegisteredLintAddress::RequestContextEntry {
            request_context,
            key,
        } => format!("/request-contexts/{request_context}/entries/{key}"),
    }
}
