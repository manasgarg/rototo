use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::diagnostics::{DiagnosticCatalogEntry, EntityId, LintDiagnostic, RototoRuleId};
use crate::error::{Result, RototoError};
use crate::model::{
    DependencyInspectReport, InspectRuntimeStatus, InspectSelection, LintAuthorityInspectReport,
    LintRuleInspectReport, LinterInspectReport, LinterRegistrationInspectReport,
    PredicateInspectReport, QualifierInspectReport, ReferenceInspectReport, ResolveInspectReport,
    ResourceInspectReport, ResourceObjectInspectReport, RulePathwayInspectReport,
    SchemaInspectReport, ValueInspectReport, VariableInspectReport, WorkspaceInspectReport,
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
        && request.resources.is_none()
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
    let resource_ids = selected_ids(
        &request.resources,
        snapshot.index.resources.keys().map(String::as_str),
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

    let mut resources = Vec::new();
    for id in resource_ids {
        resources.push(inspect_resource(snapshot, &id)?);
    }

    let schemas = selected_schemas(snapshot, inventory);
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
        schemas,
        resources,
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
    for id in request.resources.explicit_values() {
        if !snapshot.index.resources.contains_key(id) {
            return Err(RototoError::new(format!(
                "resource not found: resource://{id}"
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

fn selected_schemas(
    snapshot: &WorkspaceLintSnapshot,
    include_all_for_none: bool,
) -> Vec<SchemaInspectReport> {
    if !include_all_for_none {
        return Vec::new();
    }
    snapshot
        .index
        .schemas
        .values()
        .map(|schema| schema_report(snapshot, schema))
        .collect()
}

fn schema_report(snapshot: &WorkspaceLintSnapshot, schema: &SchemaNode) -> SchemaInspectReport {
    let path = schema.path.clone();
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_schema(diagnostic, &path))
        .cloned()
        .collect();
    let (status, error) = if let Some(message) = &schema.invalid_message {
        ("invalid".to_owned(), Some(message.clone()))
    } else if schema.validator.is_some() {
        ("valid".to_owned(), None)
    } else {
        ("unavailable".to_owned(), None)
    };

    SchemaInspectReport {
        id: schema_id(&path),
        path: path.clone(),
        status,
        error,
        consumers: schema_consumers(snapshot, &path),
        diagnostics,
    }
}

fn schema_id(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(path)
        .to_owned()
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
            runtime.validate_context(context)?;
            Some(trace_variable_unchecked(runtime, id, context).await?)
        }
        _ => None,
    };

    Ok(VariableInspectReport {
        id: id.to_owned(),
        uri: format!("variable://{id}"),
        path,
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
            runtime.validate_context(context)?;
            Some(trace_qualifier_unchecked(runtime, id, context).await?)
        }
        _ => None,
    };

    Ok(QualifierInspectReport {
        id: id.to_owned(),
        uri: format!("qualifier://{id}"),
        path,
        predicates: qualifier_predicates(qualifier),
        dependencies: qualifier_dependencies(snapshot, id),
        consumers: qualifier_consumers(snapshot, id),
        diagnostics,
        trace,
    })
}

fn inspect_resource(snapshot: &WorkspaceLintSnapshot, id: &str) -> Result<ResourceInspectReport> {
    let resource = snapshot
        .index
        .resources
        .get(id)
        .ok_or_else(|| RototoError::new(format!("resource not found: resource://{id}")))?;
    let (_source_uri, path) = document_uri_path(snapshot, resource.doc);
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_resource(diagnostic, id))
        .cloned()
        .collect();

    Ok(ResourceInspectReport {
        id: id.to_owned(),
        uri: format!("resource://{id}"),
        path,
        schema: resource_schema_dependency(snapshot, id),
        objects: resource_objects(snapshot, id),
        dependencies: resource_dependencies(snapshot, id),
        consumers: resource_consumers(snapshot, id),
        diagnostics,
    })
}

fn resource_objects(
    snapshot: &WorkspaceLintSnapshot,
    id: &str,
) -> Vec<ResourceObjectInspectReport> {
    snapshot
        .index
        .resource_objects
        .get(id)
        .into_iter()
        .flat_map(|objects| objects.values())
        .map(|object| ResourceObjectInspectReport {
            key: object.key.clone(),
            value: object.value.clone(),
            location: object.location.clone(),
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
        TypeSourceNode::Resource(resource) => format!("resource:{}", resource.value),
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

fn resource_schema_dependency(snapshot: &WorkspaceLintSnapshot, resource: &str) -> Option<String> {
    snapshot
        .references
        .edges()
        .iter()
        .find_map(|edge| match (&edge.source, &edge.target) {
            (
                ReferenceSource::ResourceSchema {
                    resource: source_resource,
                },
                ReferenceTarget::Schema(schema),
            ) if source_resource == resource => Some(schema.clone()),
            _ => None,
        })
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
                qualifier: present_string_value(&rule.qualifier),
                value: present_string_value(&rule.value),
                location: rule.location.clone(),
            })
            .collect(),
        RuleCollection::Invalid { .. } => Vec::new(),
    };
    ResolveInspectReport {
        default_value: present_string_value(default),
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

fn variable_dependencies(
    snapshot: &WorkspaceLintSnapshot,
    variable: &str,
) -> DependencyInspectReport {
    let mut qualifiers = BTreeSet::new();
    let mut context_paths = BTreeSet::new();
    let mut resources = BTreeSet::new();

    for edge in snapshot.references.edges() {
        match (&edge.source, &edge.target) {
            (
                ReferenceSource::VariableRuleQualifier {
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
                ReferenceSource::VariableResource {
                    variable: source_variable,
                },
                ReferenceTarget::Resource(resource),
            ) if source_variable == variable && edge.is_resolved() => {
                resources.insert(resource.clone());
            }
            _ => {}
        }
    }

    DependencyInspectReport {
        qualifiers: qualifiers.into_iter().collect(),
        context_paths: context_paths.into_iter().collect(),
        schemas: Vec::new(),
        resources: resources.into_iter().collect(),
    }
}

fn resource_dependencies(
    snapshot: &WorkspaceLintSnapshot,
    resource: &str,
) -> DependencyInspectReport {
    let mut schemas = BTreeSet::new();

    for edge in snapshot.references.edges() {
        match (&edge.source, &edge.target) {
            (
                ReferenceSource::ResourceSchema {
                    resource: source_resource,
                },
                ReferenceTarget::Schema(schema),
            ) if source_resource == resource && edge.is_resolved() => {
                schemas.insert(schema.clone());
            }
            _ => {}
        }
    }

    DependencyInspectReport {
        qualifiers: Vec::new(),
        context_paths: Vec::new(),
        schemas: schemas.into_iter().collect(),
        resources: Vec::new(),
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
        schemas: Vec::new(),
        resources: Vec::new(),
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
                ReferenceSource::QualifierPredicateContextAttribute {
                    qualifier: source_qualifier,
                    ..
                },
                ReferenceTarget::ContextAttribute(context_path),
            ) if source_qualifier == qualifier => {
                context_paths.insert(context_path.clone());
            }
            (
                ReferenceSource::QualifierPredicateQualifier {
                    qualifier: source_qualifier,
                    ..
                },
                ReferenceTarget::Qualifier(nested),
            ) if source_qualifier == qualifier && edge.is_resolved() => {
                collect_qualifier_dependencies(snapshot, nested, qualifiers, context_paths, seen);
            }
            _ => {}
        }
    }
}

fn qualifier_predicates(qualifier: &QualifierNode) -> Vec<PredicateInspectReport> {
    let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
        return Vec::new();
    };
    predicates
        .iter()
        .map(|predicate| PredicateInspectReport {
            index: predicate.index,
            attribute: present_string_value(&predicate.attribute),
            op: present_predicate_op_value(&predicate.op),
            value: predicate.value.as_ref().map(|value| value.value.clone()),
            salt: predicate.salt.as_ref().and_then(present_string_value),
            range: predicate.range.as_ref().and_then(|range| {
                let (Some(start), Some(end)) = (range.start, range.end) else {
                    return None;
                };
                Some(vec![start, end])
            }),
            location: predicate.location.clone(),
        })
        .collect()
}

fn present_predicate_op_value(field: &ProjectField<PredicateOp>) -> Option<String> {
    match field {
        ProjectField::Present(value) => Some(value.value.as_str().to_owned()),
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

fn resource_consumers(
    snapshot: &WorkspaceLintSnapshot,
    resource: &str,
) -> Vec<ReferenceInspectReport> {
    snapshot
        .references
        .edges()
        .iter()
        .filter_map(|edge| {
            let ReferenceTarget::Resource(target) = &edge.target else {
                return None;
            };
            if target != resource {
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
        ReferenceSource::QualifierPredicateQualifier { .. }
        | ReferenceSource::QualifierPredicateContextAttribute { .. } => "qualifier",
        ReferenceSource::ResourceSchema { .. } => "resource",
        ReferenceSource::VariableRuleQualifier { .. }
        | ReferenceSource::VariableRuleValue { .. }
        | ReferenceSource::VariableResolveDefault { .. }
        | ReferenceSource::VariableResource { .. } => "variable",
    }
}

fn reference_source_label(source: &ReferenceSource) -> String {
    match source {
        ReferenceSource::QualifierPredicateQualifier {
            qualifier,
            predicate,
        }
        | ReferenceSource::QualifierPredicateContextAttribute {
            qualifier,
            predicate,
        } => format!("qualifier {qualifier} predicate[{predicate}]"),
        ReferenceSource::VariableRuleQualifier { variable, rule }
        | ReferenceSource::VariableRuleValue { variable, rule } => {
            format!("variable {variable} resolve.rule[{rule}]")
        }
        ReferenceSource::VariableResolveDefault { variable } => {
            format!("variable {variable} resolve.default")
        }
        ReferenceSource::VariableResource { variable } => format!("variable {variable}"),
        ReferenceSource::ResourceSchema { resource } => format!("resource {resource} schema"),
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
        || selection_matches_resource(&request.resources, diagnostic)
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

fn selection_matches_resource(selection: &InspectSelection, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_resource_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_resource(diagnostic, id)),
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
        diagnostic.entity,
        EntityId::Variable { .. } | EntityId::Value { .. } | EntityId::Rule { .. }
    ) || diagnostic.primary.path.starts_with("variables/")
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let variable_path = format!("variables/{id}.toml");
    matches!(&diagnostic.entity, EntityId::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.entity, EntityId::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == variable_path
}

fn diagnostic_is_resource_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.entity,
        EntityId::Resource { .. } | EntityId::ResourceObject { .. }
    ) || diagnostic.primary.path.starts_with("resources/")
}

fn diagnostic_belongs_to_resource(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let resource_path = format!("resources/{id}.toml");
    let resource_objects_prefix = format!("resources/{id}-objects/");
    matches!(&diagnostic.entity, EntityId::Resource { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::ResourceObject { resource, .. } if resource == id)
        || diagnostic.primary.path == resource_path
        || diagnostic
            .primary
            .path
            .starts_with(&resource_objects_prefix)
}

fn diagnostic_is_qualifier_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.entity,
        EntityId::Qualifier { .. } | EntityId::Predicate { .. }
    ) || diagnostic.primary.path.starts_with("qualifiers/")
}

fn diagnostic_belongs_to_qualifier(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let qualifier_path = format!("qualifiers/{id}.toml");
    matches!(&diagnostic.entity, EntityId::Qualifier { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Predicate { qualifier, .. } if qualifier == id)
        || diagnostic.primary.path == qualifier_path
}

fn diagnostic_is_linter_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(diagnostic.entity, EntityId::CustomLint { .. })
        || diagnostic.primary.path.starts_with("lint/")
        || authority_of(&diagnostic.rule.as_string()).is_some_and(|authority| authority != "rototo")
}

fn diagnostic_belongs_to_linter(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let path = format!("lint/{id}.lua");
    matches!(&diagnostic.entity, EntityId::CustomLint { path: diagnostic_path } if diagnostic_path == &path)
        || diagnostic.primary.path == path
}

fn diagnostic_belongs_to_schema(diagnostic: &LintDiagnostic, path: &str) -> bool {
    matches!(&diagnostic.entity, EntityId::Schema { path: diagnostic_path } if diagnostic_path == path)
        || diagnostic.primary.path == path
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

fn schema_consumers(
    snapshot: &WorkspaceLintSnapshot,
    schema_path: &str,
) -> Vec<ReferenceInspectReport> {
    snapshot
        .references
        .edges()
        .iter()
        .filter_map(|edge| {
            let ReferenceTarget::Schema(target) = &edge.target else {
                return None;
            };
            if target != schema_path || !edge.is_resolved() {
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
                    entity: registered_entity_label(&registration.selector.entity).to_owned(),
                    field: registration
                        .selector
                        .field
                        .as_ref()
                        .map(registered_field_label),
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

fn registered_entity_label(entity: &RegisteredLintEntity) -> &'static str {
    match entity {
        RegisteredLintEntity::Workspace => "workspace",
        RegisteredLintEntity::Qualifier => "qualifier",
        RegisteredLintEntity::Variable => "variable",
        RegisteredLintEntity::Value => "value",
        RegisteredLintEntity::Schema => "schema",
    }
}

fn registered_field_label(field: &RegisteredLintField) -> String {
    match field {
        RegisteredLintField::Workspace(WorkspaceLintField::Extends) => "extends".to_owned(),
        RegisteredLintField::Qualifier(QualifierLintField::Id) => "id".to_owned(),
        RegisteredLintField::Qualifier(QualifierLintField::Description) => "description".to_owned(),
        RegisteredLintField::Qualifier(QualifierLintField::Predicates) => "predicates".to_owned(),
        RegisteredLintField::Variable(VariableLintField::Id) => "id".to_owned(),
        RegisteredLintField::Variable(VariableLintField::Description) => "description".to_owned(),
        RegisteredLintField::Variable(VariableLintField::Type) => "type".to_owned(),
        RegisteredLintField::Variable(VariableLintField::Schema) => "schema".to_owned(),
        RegisteredLintField::Variable(VariableLintField::Values) => "values".to_owned(),
        RegisteredLintField::Variable(VariableLintField::Resolve) => "resolve".to_owned(),
        RegisteredLintField::Value(ValueLintField::JsonPath(path)) => {
            format!("value.{}", path.join("."))
        }
        RegisteredLintField::Value(ValueLintField::Key) => "key".to_owned(),
        RegisteredLintField::Value(ValueLintField::Value) => "value".to_owned(),
        RegisteredLintField::Schema(SchemaLintField::JsonPath(path)) => {
            format!("json.{}", path.join("."))
        }
        RegisteredLintField::Schema(SchemaLintField::Json) => "json".to_owned(),
    }
}
