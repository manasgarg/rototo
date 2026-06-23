use std::collections::BTreeMap;

use serde_json::Value as JsonValue;

use crate::diagnostics::{
    DiagnosticLocation, DocId, SemanticEntity, SemanticField, SemanticTarget,
};

use super::super::engine::{LintContext, variable_values};
use super::super::index::*;
use super::super::syntax::item_location;
use super::marshal::{expanded_variable_toml_json, parsed_toml_json};
use super::{RegisteredLintAddress, RegisteredLintSelector};

pub(super) struct RegisteredLintTargetInstance {
    pub(super) target: SemanticTarget,
    pub(super) location: DiagnosticLocation,
    pub(super) data: JsonValue,
}

pub(super) struct RegisteredLintOutputAnchor {
    pub(super) target: SemanticTarget,
    pub(super) location: DiagnosticLocation,
}

pub(super) fn registered_lint_workspace(ctx: &LintContext) -> JsonValue {
    serde_json::json!({
        "version": 1,
        "root": ctx.source.root.display().to_string(),
        "manifest": workspace_manifest_data(ctx),
        "qualifiers": ctx.index.qualifiers.iter()
            .map(|(id, qualifier)| (id.clone(), qualifier_data(ctx, qualifier)))
            .collect::<BTreeMap<_, _>>(),
        "variables": ctx.index.variables.iter()
            .map(|(id, variable)| (id.clone(), variable_data(ctx, variable)))
            .collect::<BTreeMap<_, _>>(),
        "catalogs": ctx.index.catalogs.iter()
            .map(|(id, catalog)| (id.clone(), catalog_data(ctx, catalog)))
            .collect::<BTreeMap<_, _>>(),
        "request_contexts": ctx.index.request_contexts.iter()
            .map(|(id, request_context)| (id.clone(), request_context_data(ctx, request_context)))
            .collect::<BTreeMap<_, _>>(),
    })
}

pub(super) fn registered_lint_targets(
    ctx: &LintContext,
    selector: &RegisteredLintSelector,
) -> Vec<RegisteredLintTargetInstance> {
    match &selector.address {
        RegisteredLintAddress::Workspace => registered_workspace_target(ctx).into_iter().collect(),
        RegisteredLintAddress::Qualifiers => ctx
            .index
            .qualifiers
            .values()
            .filter_map(|qualifier| registered_qualifier_target(ctx, qualifier))
            .collect(),
        RegisteredLintAddress::Qualifier { id } => ctx
            .index
            .qualifiers
            .get(id)
            .and_then(|qualifier| registered_qualifier_target(ctx, qualifier))
            .into_iter()
            .collect(),
        RegisteredLintAddress::Variables => ctx
            .index
            .variables
            .values()
            .filter_map(|variable| registered_variable_target(ctx, variable))
            .collect(),
        RegisteredLintAddress::Variable { id } => ctx
            .index
            .variables
            .get(id)
            .and_then(|variable| registered_variable_target(ctx, variable))
            .into_iter()
            .collect(),
        RegisteredLintAddress::VariableValues { variable } => ctx
            .index
            .variables
            .get(variable)
            .into_iter()
            .flat_map(registered_value_targets)
            .collect(),
        RegisteredLintAddress::VariableValue { variable, key } => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| registered_value_target(variable, key))
            .into_iter()
            .collect(),
        RegisteredLintAddress::VariableRules { variable } => ctx
            .index
            .variables
            .get(variable)
            .into_iter()
            .flat_map(registered_rule_targets)
            .collect(),
        RegisteredLintAddress::VariableRule { variable, index } => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| registered_rule_target(variable, *index))
            .into_iter()
            .collect(),
        RegisteredLintAddress::Catalogs => ctx
            .index
            .catalogs
            .values()
            .map(|catalog| registered_catalog_target(ctx, catalog))
            .collect(),
        RegisteredLintAddress::Catalog { id } => ctx
            .index
            .catalogs
            .get(id)
            .map(|catalog| registered_catalog_target(ctx, catalog))
            .into_iter()
            .collect(),
        RegisteredLintAddress::CatalogEntries { catalog } => ctx
            .index
            .catalog_entries
            .get(catalog)
            .into_iter()
            .flat_map(|entries| entries.values().map(registered_catalog_entry_target))
            .collect(),
        RegisteredLintAddress::CatalogEntry { catalog, key } => ctx
            .index
            .catalog_entries
            .get(catalog)
            .and_then(|entries| entries.get(key))
            .map(registered_catalog_entry_target)
            .into_iter()
            .collect(),
        RegisteredLintAddress::RequestContexts => ctx
            .index
            .request_contexts
            .values()
            .map(|request_context| registered_request_context_target(ctx, request_context))
            .collect(),
        RegisteredLintAddress::RequestContext { id } => ctx
            .index
            .request_contexts
            .get(id)
            .map(|request_context| registered_request_context_target(ctx, request_context))
            .into_iter()
            .collect(),
        RegisteredLintAddress::RequestContextEntries { request_context } => ctx
            .index
            .request_context_entries
            .get(request_context)
            .into_iter()
            .flat_map(|entries| {
                entries
                    .values()
                    .map(registered_request_context_entry_target)
            })
            .collect(),
        RegisteredLintAddress::RequestContextEntry {
            request_context,
            key,
        } => ctx
            .index
            .request_context_entries
            .get(request_context)
            .and_then(|entries| entries.get(key))
            .map(registered_request_context_entry_target)
            .into_iter()
            .collect(),
    }
}

pub(super) fn registered_lint_output_anchor(
    ctx: &LintContext,
    target: &RegisteredLintTargetInstance,
    path: Option<&str>,
) -> RegisteredLintOutputAnchor {
    let current = RegisteredLintOutputAnchor {
        target: target.target.clone(),
        location: target.location.clone(),
    };
    let Some(path) = path.map(str::trim) else {
        return current;
    };
    let Some(tokens) = parse_json_pointer(path) else {
        return current;
    };
    resolve_output_pointer(ctx, &target.target.entity, &tokens).unwrap_or(current)
}

fn resolve_output_pointer(
    ctx: &LintContext,
    entity: &SemanticEntity,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    match entity {
        SemanticEntity::Workspace => workspace_pointer(ctx, tokens),
        SemanticEntity::Qualifier { id } => qualifier_pointer(ctx, id, tokens),
        SemanticEntity::Predicate { .. } => None,
        SemanticEntity::Variable { id } => variable_pointer(ctx, id, tokens),
        SemanticEntity::Value { variable, key } => value_pointer(ctx, variable, key, tokens),
        SemanticEntity::Rule { variable, index } => rule_pointer(ctx, variable, *index, tokens),
        SemanticEntity::Catalog { id } => catalog_pointer(ctx, id, tokens),
        SemanticEntity::CatalogEntry { catalog, key } => {
            catalog_entry_pointer(ctx, catalog, key, tokens)
        }
        SemanticEntity::RequestContext { id } => request_context_pointer(ctx, id, tokens),
        SemanticEntity::RequestContextEntry {
            request_context,
            key,
        } => request_context_entry_pointer(ctx, request_context, key, tokens),
        SemanticEntity::Manifest | SemanticEntity::CustomLint { .. } => None,
    }
}

fn registered_lint_output_location(
    ctx: &LintContext,
    entity: &SemanticEntity,
    field: Option<&SemanticField>,
    fallback: DiagnosticLocation,
) -> DiagnosticLocation {
    let Some(field) = field else {
        return fallback;
    };
    match (entity, field) {
        (SemanticEntity::Workspace, SemanticField::WorkspaceExtends) => ctx
            .index
            .manifest
            .as_ref()
            .map(|manifest| registered_workspace_location(ctx, manifest, Some(field)))
            .unwrap_or(fallback),
        (SemanticEntity::Qualifier { id }, _) => ctx
            .index
            .qualifiers
            .get(id)
            .map(|qualifier| registered_qualifier_location(ctx, qualifier, Some(field)))
            .unwrap_or(fallback),
        (SemanticEntity::Variable { id }, _) => ctx
            .index
            .variables
            .get(id)
            .map(|variable| registered_variable_location(ctx, variable, Some(field)))
            .unwrap_or(fallback),
        (SemanticEntity::Value { variable, key }, _) => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_value(variable, key))
            .map(|value| value.location.clone())
            .unwrap_or(fallback),
        (SemanticEntity::Rule { variable, index }, _) => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_rule(variable, *index))
            .map(|rule| registered_rule_location(rule, Some(field)))
            .unwrap_or(fallback),
        (SemanticEntity::Catalog { id }, _) => ctx
            .index
            .catalogs
            .get(id)
            .map(|catalog| catalog.location.clone())
            .unwrap_or(fallback),
        (SemanticEntity::CatalogEntry { catalog, key }, _) => ctx
            .index
            .catalog_entries
            .get(catalog)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone())
            .unwrap_or(fallback),
        (SemanticEntity::RequestContext { id }, _) => ctx
            .index
            .request_contexts
            .get(id)
            .map(|request_context| request_context.location.clone())
            .unwrap_or(fallback),
        (
            SemanticEntity::RequestContextEntry {
                request_context,
                key,
            },
            _,
        ) => ctx
            .index
            .request_context_entries
            .get(request_context)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone())
            .unwrap_or(fallback),
        _ => fallback,
    }
}

fn registered_lint_output_anchor_for(
    ctx: &LintContext,
    entity: SemanticEntity,
    field: Option<&SemanticField>,
) -> Option<RegisteredLintOutputAnchor> {
    let target = SemanticTarget::entity(entity);
    let location = entity_location(ctx, &target.entity)?;
    Some(anchor_with_location(ctx, target, field, location))
}

fn anchor_with_location(
    ctx: &LintContext,
    target: SemanticTarget,
    field: Option<&SemanticField>,
    location: DiagnosticLocation,
) -> RegisteredLintOutputAnchor {
    let output_target = match field {
        Some(field) => SemanticTarget::field(target.entity.clone(), field.clone()),
        None => target,
    };
    let location =
        registered_lint_output_location(ctx, &output_target.entity, field, location.clone());
    RegisteredLintOutputAnchor {
        target: output_target,
        location,
    }
}

fn field_anchor(
    ctx: &LintContext,
    entity: SemanticEntity,
    field: SemanticField,
    fallback: DiagnosticLocation,
) -> RegisteredLintOutputAnchor {
    anchor_with_location(ctx, SemanticTarget::entity(entity), Some(&field), fallback)
}

fn entity_location(ctx: &LintContext, entity: &SemanticEntity) -> Option<DiagnosticLocation> {
    match entity {
        SemanticEntity::Workspace => ctx
            .index
            .manifest
            .as_ref()
            .map(|manifest| registered_workspace_location(ctx, manifest, None)),
        SemanticEntity::Qualifier { id } => ctx
            .index
            .qualifiers
            .get(id)
            .map(|qualifier| qualifier.location.clone()),
        SemanticEntity::Variable { id } => ctx
            .index
            .variables
            .get(id)
            .map(|variable| variable.location.clone()),
        SemanticEntity::Value { variable, key } => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_value(variable, key))
            .map(|value| value.location.clone()),
        SemanticEntity::Rule { variable, index } => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_rule(variable, *index))
            .map(|rule| rule.location.clone()),
        SemanticEntity::Catalog { id } => ctx
            .index
            .catalogs
            .get(id)
            .map(|catalog| catalog.location.clone()),
        SemanticEntity::CatalogEntry { catalog, key } => ctx
            .index
            .catalog_entries
            .get(catalog)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone()),
        SemanticEntity::RequestContext { id } => ctx
            .index
            .request_contexts
            .get(id)
            .map(|request_context| request_context.location.clone()),
        SemanticEntity::RequestContextEntry {
            request_context,
            key,
        } => ctx
            .index
            .request_context_entries
            .get(request_context)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone()),
        SemanticEntity::Predicate { .. }
        | SemanticEntity::Manifest
        | SemanticEntity::CustomLint { .. } => None,
    }
}

fn workspace_pointer(ctx: &LintContext, tokens: &[String]) -> Option<RegisteredLintOutputAnchor> {
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, SemanticEntity::Workspace, None);
    }
    let manifest = ctx.index.manifest.as_ref()?;
    match tokens {
        [segment, ..] if segment == "extends" => Some(field_anchor(
            ctx,
            SemanticEntity::Workspace,
            SemanticField::WorkspaceExtends,
            registered_workspace_location(ctx, manifest, None),
        )),
        [segment] if segment == "manifest" => {
            registered_lint_output_anchor_for(ctx, SemanticEntity::Workspace, None)
        }
        [segment, field, ..] if segment == "manifest" && field == "extends" => Some(field_anchor(
            ctx,
            SemanticEntity::Workspace,
            SemanticField::WorkspaceExtends,
            registered_workspace_location(ctx, manifest, None),
        )),
        [segment, id, rest @ ..] if segment == "qualifiers" => qualifier_pointer(ctx, id, rest)
            .or_else(|| registered_lint_output_anchor_for(ctx, SemanticEntity::Workspace, None)),
        [segment, id, rest @ ..] if segment == "variables" => variable_pointer(ctx, id, rest)
            .or_else(|| registered_lint_output_anchor_for(ctx, SemanticEntity::Workspace, None)),
        [segment, id, rest @ ..] if segment == "catalogs" => catalog_pointer(ctx, id, rest)
            .or_else(|| registered_lint_output_anchor_for(ctx, SemanticEntity::Workspace, None)),
        [segment, id, rest @ ..] if segment == "request_contexts" => {
            request_context_pointer(ctx, id, rest)
                .or_else(|| registered_lint_output_anchor_for(ctx, SemanticEntity::Workspace, None))
        }
        _ => registered_lint_output_anchor_for(ctx, SemanticEntity::Workspace, None),
    }
}

fn qualifier_pointer(
    ctx: &LintContext,
    id: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let qualifier = ctx.index.qualifiers.get(id)?;
    let entity = SemanticEntity::Qualifier { id: id.to_owned() };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment, ..] if segment == "description" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::Description,
            qualifier.location.clone(),
        )),
        [segment, ..] if segment == "when" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::QualifierWhen,
            qualifier.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn variable_pointer(
    ctx: &LintContext,
    id: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let variable = ctx.index.variables.get(id)?;
    let entity = SemanticEntity::Variable { id: id.to_owned() };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment, ..] if segment == "description" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::Description,
            variable.location.clone(),
        )),
        [segment, ..] if segment == "declaration" || segment == "type" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableType,
            variable.location.clone(),
        )),
        [segment, ..] if segment == "schema" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableSchema,
            variable.location.clone(),
        )),
        [segment] if segment == "values" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableValues,
            variable.location.clone(),
        )),
        [segment, key, rest @ ..] if segment == "values" => value_pointer(ctx, id, key, rest)
            .or_else(|| {
                Some(field_anchor(
                    ctx,
                    entity,
                    SemanticField::VariableValues,
                    variable.location.clone(),
                ))
            }),
        [segment] if segment == "resolve" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableResolve,
            variable.location.clone(),
        )),
        [segment, field, ..] if segment == "resolve" && field == "default" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableResolveDefault,
            variable.location.clone(),
        )),
        [segment, field] if segment == "resolve" && field == "rules" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableResolve,
            variable.location.clone(),
        )),
        [segment, field, index, rest @ ..] if segment == "resolve" && field == "rules" => {
            let Some(index) = parse_pointer_index(index) else {
                return Some(field_anchor(
                    ctx,
                    entity,
                    SemanticField::VariableResolve,
                    variable.location.clone(),
                ));
            };
            rule_pointer(ctx, id, index, rest).or_else(|| {
                Some(field_anchor(
                    ctx,
                    entity,
                    SemanticField::VariableResolve,
                    variable.location.clone(),
                ))
            })
        }
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn value_pointer(
    ctx: &LintContext,
    variable_id: &str,
    key: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let variable = ctx.index.variables.get(variable_id)?;
    let value = find_value(variable, key)?;
    let entity = SemanticEntity::Value {
        variable: variable_id.to_owned(),
        key: key.to_owned(),
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::Value,
            value.location.clone(),
        )),
        [segment, rest @ ..] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::ValueJsonPath {
                path: rest.to_vec(),
            },
            value.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn rule_pointer(
    ctx: &LintContext,
    variable_id: &str,
    index: usize,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let variable = ctx.index.variables.get(variable_id)?;
    let rule = find_rule(variable, index)?;
    let entity = SemanticEntity::Rule {
        variable: variable_id.to_owned(),
        index,
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens.first().map(String::as_str) {
        Some("when") => Some(field_anchor(
            ctx,
            entity.clone(),
            SemanticField::VariableRuleWhen,
            rule.when
                .as_ref()
                .map(ProjectField::location)
                .unwrap_or_else(|| rule.location.clone()),
        )),
        Some("query") => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableRuleQuery,
            rule.query
                .as_ref()
                .map(ProjectField::location)
                .unwrap_or_else(|| rule.location.clone()),
        )),
        Some("value") => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableRuleValue,
            rule.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn catalog_pointer(
    ctx: &LintContext,
    id: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let catalog = ctx.index.catalogs.get(id)?;
    let entity = SemanticEntity::Catalog { id: id.to_owned() };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJson,
            catalog.location.clone(),
        )),
        [segment, rest @ ..] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJsonPath {
                path: rest.to_vec(),
            },
            catalog.location.clone(),
        )),
        [segment, key, rest @ ..] if segment == "entries" => {
            catalog_entry_pointer(ctx, id, key, rest)
                .or_else(|| registered_lint_output_anchor_for(ctx, entity, None))
        }
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn catalog_entry_pointer(
    ctx: &LintContext,
    catalog_id: &str,
    key: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let entry = ctx.index.catalog_entries.get(catalog_id)?.get(key)?;
    let entity = SemanticEntity::CatalogEntry {
        catalog: catalog_id.to_owned(),
        key: key.to_owned(),
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::CatalogEntry,
            entry.location.clone(),
        )),
        [segment, rest @ ..] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::ValueJsonPath {
                path: rest.to_vec(),
            },
            entry.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn request_context_pointer(
    ctx: &LintContext,
    id: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let request_context = ctx.index.request_contexts.get(id)?;
    let entity = SemanticEntity::RequestContext { id: id.to_owned() };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJson,
            request_context.location.clone(),
        )),
        [segment, rest @ ..] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJsonPath {
                path: rest.to_vec(),
            },
            request_context.location.clone(),
        )),
        [segment, key, rest @ ..] if segment == "entries" => {
            request_context_entry_pointer(ctx, id, key, rest)
                .or_else(|| registered_lint_output_anchor_for(ctx, entity, None))
        }
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn request_context_entry_pointer(
    ctx: &LintContext,
    request_context_id: &str,
    key: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let entry = ctx
        .index
        .request_context_entries
        .get(request_context_id)?
        .get(key)?;
    let entity = SemanticEntity::RequestContextEntry {
        request_context: request_context_id.to_owned(),
        key: key.to_owned(),
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::RequestContextEntry,
            entry.location.clone(),
        )),
        [segment, rest @ ..] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::ValueJsonPath {
                path: rest.to_vec(),
            },
            entry.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn registered_workspace_target(ctx: &LintContext) -> Option<RegisteredLintTargetInstance> {
    let manifest = ctx.index.manifest.as_ref()?;
    Some(RegisteredLintTargetInstance {
        target: SemanticEntity::Workspace.into(),
        location: registered_workspace_location(ctx, manifest, None),
        data: workspace_target_data(ctx),
    })
}

fn registered_qualifier_target(
    ctx: &LintContext,
    qualifier: &QualifierNode,
) -> Option<RegisteredLintTargetInstance> {
    ctx.source.documents.get(&qualifier.doc)?;
    Some(RegisteredLintTargetInstance {
        target: qualifier.target(),
        location: qualifier.location.clone(),
        data: qualifier_data(ctx, qualifier),
    })
}

fn registered_variable_target(
    ctx: &LintContext,
    variable: &VariableNode,
) -> Option<RegisteredLintTargetInstance> {
    ctx.source.documents.get(&variable.doc)?;
    Some(RegisteredLintTargetInstance {
        target: variable.target(),
        location: variable.location.clone(),
        data: variable_data(ctx, variable),
    })
}

fn registered_value_targets(variable: &VariableNode) -> Vec<RegisteredLintTargetInstance> {
    variable
        .values
        .inline_values
        .values()
        .map(registered_value_target_from_node)
        .collect()
}

fn registered_value_target(
    variable: &VariableNode,
    key: &str,
) -> Option<RegisteredLintTargetInstance> {
    find_value(variable, key).map(registered_value_target_from_node)
}

fn registered_value_target_from_node(value: &ValueNode) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: value.target(),
        location: value.location.clone(),
        data: value_data(value),
    }
}

fn registered_rule_targets(variable: &VariableNode) -> Vec<RegisteredLintTargetInstance> {
    match &variable.resolve {
        ResolveNode::Resolve {
            rules: RuleCollection::Rules(rules),
            ..
        } => rules
            .iter()
            .map(|rule| registered_rule_target_from_node(&variable.id, rule))
            .collect(),
        _ => Vec::new(),
    }
}

fn registered_rule_target(
    variable: &VariableNode,
    index: usize,
) -> Option<RegisteredLintTargetInstance> {
    find_rule(variable, index).map(|rule| registered_rule_target_from_node(&variable.id, rule))
}

fn registered_rule_target_from_node(
    variable_id: &str,
    rule: &VariableRuleNode,
) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: rule.target(variable_id),
        location: rule.location.clone(),
        data: rule_data(variable_id, rule),
    }
}

fn registered_catalog_target(
    ctx: &LintContext,
    catalog: &CatalogNode,
) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: catalog.target(),
        location: catalog.location.clone(),
        data: catalog_data(ctx, catalog),
    }
}

fn registered_catalog_entry_target(entry: &CatalogEntryNode) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: entry.target(),
        location: entry.location.clone(),
        data: catalog_entry_data(entry),
    }
}

fn registered_request_context_target(
    ctx: &LintContext,
    request_context: &RequestContextNode,
) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: request_context.target(),
        location: request_context.location.clone(),
        data: request_context_data(ctx, request_context),
    }
}

fn registered_request_context_entry_target(
    entry: &RequestContextEntryNode,
) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: entry.target(),
        location: entry.location.clone(),
        data: request_context_entry_data(entry),
    }
}

fn workspace_target_data(ctx: &LintContext) -> JsonValue {
    let extends = ctx
        .index
        .manifest
        .as_ref()
        .map(workspace_extends_data)
        .unwrap_or_default();
    serde_json::json!({
        "kind": "workspace",
        "root": ctx.source.root.display().to_string(),
        "manifest": workspace_manifest_data(ctx),
        "extends": extends,
    })
}

fn workspace_manifest_data(ctx: &LintContext) -> JsonValue {
    let Some(manifest) = &ctx.index.manifest else {
        return JsonValue::Null;
    };
    let document = ctx.source.documents.get(&manifest.doc);
    serde_json::json!({
        "kind": "workspace",
        "uri": document.map(|document| document.uri.clone()),
        "path": document.map(|document| document.path.clone()).unwrap_or_else(|| "rototo-workspace.toml".to_owned()),
        "toml": parsed_toml_json(ctx, manifest.doc),
        "extends": workspace_extends_data(manifest),
    })
}

fn workspace_extends_data(manifest: &ManifestNode) -> Vec<String> {
    match &manifest.extends {
        WorkspaceExtendsCollection::Sources { values, .. } => {
            values.iter().map(|value| value.source.clone()).collect()
        }
        WorkspaceExtendsCollection::Missing | WorkspaceExtendsCollection::Invalid { .. } => {
            Vec::new()
        }
    }
}

fn qualifier_data(ctx: &LintContext, qualifier: &QualifierNode) -> JsonValue {
    let document = ctx.source.documents.get(&qualifier.doc);
    serde_json::json!({
        "kind": "qualifier",
        "id": qualifier.id,
        "uri": document.map(|document| document.uri.clone()),
        "path": document.map(|document| document.path.clone()),
        "description": optional_project_string(&qualifier.description),
        "when": project_expression(&qualifier.when),
        "toml": parsed_toml_json(ctx, qualifier.doc),
    })
}

fn variable_data(ctx: &LintContext, variable: &VariableNode) -> JsonValue {
    let document = ctx.source.documents.get(&variable.doc);
    serde_json::json!({
        "kind": "variable",
        "id": variable.id,
        "uri": document.map(|document| document.uri.clone()),
        "path": document.map(|document| document.path.clone()),
        "description": optional_project_string(&variable.description),
        "declaration": type_source_data(&variable.type_source),
        "values": variable_values(ctx, variable)
            .map(|value| (value.key.clone(), value_data(value)))
            .collect::<BTreeMap<_, _>>(),
        "resolve": resolve_data(&variable.id, &variable.resolve),
        "toml": expanded_variable_toml_json(ctx, variable),
    })
}

fn value_data(value: &ValueNode) -> JsonValue {
    serde_json::json!({
        "kind": "value",
        "variable": value.variable_id,
        "key": value.key,
        "value": value.value,
        "origin": value_origin_json(value),
    })
}

fn rule_data(variable_id: &str, rule: &VariableRuleNode) -> JsonValue {
    serde_json::json!({
    "kind": "rule",
    "variable": variable_id,
    "index": rule.index,
        "when": optional_project_expression(&rule.when),
        "query": optional_project_expression(&rule.query),
    "value": project_json(&rule.value),
    })
}

fn catalog_data(ctx: &LintContext, catalog: &CatalogNode) -> JsonValue {
    serde_json::json!({
        "kind": "catalog",
        "id": catalog.id,
        "path": catalog.path,
        "json": catalog.json.clone().unwrap_or(JsonValue::Null),
        "entries": ctx.index.catalog_entries.get(&catalog.id)
            .map(|entries| entries.iter()
                .map(|(key, entry)| (key.clone(), catalog_entry_data(entry)))
                .collect::<BTreeMap<_, _>>())
            .unwrap_or_default(),
    })
}

fn catalog_entry_data(entry: &CatalogEntryNode) -> JsonValue {
    serde_json::json!({
        "kind": "catalog_entry",
        "catalog": entry.catalog_id,
        "key": entry.key,
        "path": entry.location.path.clone(),
        "value": entry.value,
    })
}

fn request_context_data(ctx: &LintContext, request_context: &RequestContextNode) -> JsonValue {
    serde_json::json!({
        "kind": "request_context",
        "id": request_context.id,
        "path": request_context.path,
        "json": request_context.json.clone().unwrap_or(JsonValue::Null),
        "entries": ctx.index.request_context_entries.get(&request_context.id)
            .map(|entries| entries.iter()
                .map(|(key, entry)| (key.clone(), request_context_entry_data(entry)))
                .collect::<BTreeMap<_, _>>())
            .unwrap_or_default(),
    })
}

fn request_context_entry_data(entry: &RequestContextEntryNode) -> JsonValue {
    serde_json::json!({
        "kind": "request_context_entry",
        "request_context": entry.request_context_id,
        "key": entry.key,
        "path": entry.path,
        "value": entry.value.clone().unwrap_or(JsonValue::Null),
    })
}

fn type_source_data(type_source: &TypeSourceNode) -> JsonValue {
    match type_source {
        TypeSourceNode::Primitive(value) => serde_json::json!({
            "kind": "primitive",
            "value": value.value,
        }),
        TypeSourceNode::Catalog(value) => serde_json::json!({
            "kind": "catalog",
            "value": value.value,
        }),
        TypeSourceNode::Schema(value) => serde_json::json!({
            "kind": "schema",
            "value": value.value,
        }),
        TypeSourceNode::Missing { .. } => serde_json::json!({
            "kind": "missing",
        }),
        TypeSourceNode::Conflict { .. } => serde_json::json!({
            "kind": "conflict",
        }),
        TypeSourceNode::Invalid { .. } => serde_json::json!({
            "kind": "invalid",
        }),
    }
}

fn resolve_data(variable_id: &str, resolve: &ResolveNode) -> JsonValue {
    match resolve {
        ResolveNode::Resolve { default, rules, .. } => serde_json::json!({
            "kind": "resolve",
            "default": project_json(default),
            "rules": match rules {
                RuleCollection::Rules(rules) => rules
                    .iter()
                    .map(|rule| rule_data(variable_id, rule))
                    .collect::<Vec<_>>(),
                RuleCollection::Invalid { .. } => Vec::new(),
            },
        }),
        ResolveNode::Missing { .. } => serde_json::json!({
            "kind": "missing",
        }),
        ResolveNode::Invalid { .. } => serde_json::json!({
            "kind": "invalid",
        }),
    }
}

fn value_origin_json(value: &ValueNode) -> JsonValue {
    match &value.origin {
        ValueOrigin::Inline { variable_doc } => serde_json::json!({
            "kind": "inline",
            "doc": variable_doc,
        }),
    }
}

fn optional_project_string(field: &Option<ProjectField<String>>) -> JsonValue {
    field
        .as_ref()
        .map(project_string)
        .unwrap_or(JsonValue::Null)
}

fn optional_project_expression(
    field: &Option<ProjectField<crate::expression::Expression>>,
) -> JsonValue {
    field
        .as_ref()
        .map(project_expression)
        .unwrap_or(JsonValue::Null)
}

fn project_string(field: &ProjectField<String>) -> JsonValue {
    match field {
        ProjectField::Present(value) => JsonValue::String(value.value.clone()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => JsonValue::Null,
    }
}

fn project_expression(field: &ProjectField<crate::expression::Expression>) -> JsonValue {
    match field {
        ProjectField::Present(value) => JsonValue::String(value.value.source().to_owned()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => JsonValue::Null,
    }
}

fn project_json(field: &ProjectField<JsonValue>) -> JsonValue {
    match field {
        ProjectField::Present(value) => value.value.clone(),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => JsonValue::Null,
    }
}

fn find_value<'a>(variable: &'a VariableNode, key: &str) -> Option<&'a ValueNode> {
    variable
        .values
        .inline_values
        .values()
        .find(|value| value.key.as_str() == key)
}

fn find_rule(variable: &VariableNode, index: usize) -> Option<&VariableRuleNode> {
    match &variable.resolve {
        ResolveNode::Resolve {
            rules: RuleCollection::Rules(rules),
            ..
        } => rules.iter().find(|rule| rule.index == index),
        _ => None,
    }
}

fn registered_workspace_location(
    ctx: &LintContext,
    manifest: &ManifestNode,
    field: Option<&SemanticField>,
) -> DiagnosticLocation {
    match field {
        Some(SemanticField::WorkspaceExtends) => {
            toml_root_item_location(ctx, manifest.doc, "extends")
                .unwrap_or_else(|| manifest.location.clone())
        }
        _ => manifest.location.clone(),
    }
}

fn registered_qualifier_location(
    ctx: &LintContext,
    qualifier: &QualifierNode,
    field: Option<&SemanticField>,
) -> DiagnosticLocation {
    match field {
        Some(SemanticField::Description) => {
            toml_root_item_location(ctx, qualifier.doc, "description")
                .unwrap_or_else(|| qualifier.location.clone())
        }
        Some(SemanticField::QualifierWhen) => toml_root_item_location(ctx, qualifier.doc, "when")
            .unwrap_or_else(|| qualifier.location.clone()),
        _ => qualifier.location.clone(),
    }
}

fn registered_variable_location(
    ctx: &LintContext,
    variable: &VariableNode,
    field: Option<&SemanticField>,
) -> DiagnosticLocation {
    match field {
        Some(SemanticField::Description) => {
            toml_root_item_location(ctx, variable.doc, "description")
                .unwrap_or_else(|| variable.location.clone())
        }
        Some(SemanticField::VariableType)
            if matches!(
                &variable.type_source,
                TypeSourceNode::Primitive(_) | TypeSourceNode::Catalog(_)
            ) =>
        {
            variable.type_source.location()
        }
        Some(SemanticField::VariableSchema)
            if matches!(&variable.type_source, TypeSourceNode::Schema(_)) =>
        {
            variable.type_source.location()
        }
        Some(SemanticField::VariableValues) => variable.values.location.clone(),
        Some(SemanticField::VariableResolve) => {
            toml_root_item_location(ctx, variable.doc, "resolve")
                .unwrap_or_else(|| variable.resolve.location())
        }
        Some(SemanticField::VariableResolveDefault) => match &variable.resolve {
            ResolveNode::Resolve { default, .. } => default.location(),
            _ => variable.resolve.location(),
        },
        _ => variable.location.clone(),
    }
}

fn registered_rule_location(
    rule: &VariableRuleNode,
    field: Option<&SemanticField>,
) -> DiagnosticLocation {
    match field {
        Some(SemanticField::VariableRuleWhen) => rule
            .when
            .as_ref()
            .map(ProjectField::location)
            .unwrap_or_else(|| rule.location.clone()),
        Some(SemanticField::VariableRuleQuery) => rule
            .query
            .as_ref()
            .map(ProjectField::location)
            .unwrap_or_else(|| rule.location.clone()),
        Some(SemanticField::VariableRuleValue) => rule.value.location(),
        _ => rule.location.clone(),
    }
}

fn toml_root_item_location(ctx: &LintContext, doc: DocId, key: &str) -> Option<DiagnosticLocation> {
    let document = ctx.source.documents.get(&doc)?;
    let parsed = ctx.syntax.toml.get(&doc)?;
    parsed
        .root_table()?
        .get(key)
        .map(|item| item_location(document, item))
}

fn parse_json_pointer(pointer: &str) -> Option<Vec<String>> {
    if pointer.is_empty() {
        return Some(Vec::new());
    }
    let rest = pointer.strip_prefix('/')?;
    rest.split('/').map(decode_json_pointer_token).collect()
}

fn decode_json_pointer_token(token: &str) -> Option<String> {
    let mut decoded = String::new();
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '~' {
            decoded.push(ch);
            continue;
        }
        match chars.next()? {
            '0' => decoded.push('~'),
            '1' => decoded.push('/'),
            _ => return None,
        }
    }
    Some(decoded)
}

fn parse_pointer_index(token: &str) -> Option<usize> {
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}
