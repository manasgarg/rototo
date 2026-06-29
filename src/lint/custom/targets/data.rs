use std::collections::BTreeMap;

use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, SemanticEntity, SemanticTarget};
use crate::lint::custom::marshal::{expanded_variable_toml_json, parsed_toml_json};
use crate::lint::custom::{RegisteredLintAddress, RegisteredLintSelector};
use crate::lint::engine::{LintContext, variable_values};
use crate::lint::index::*;

use super::locations::registered_package_location;
use super::{find_rule, find_value};

pub(crate) struct RegisteredLintTargetInstance {
    pub(crate) target: SemanticTarget,
    pub(crate) location: DiagnosticLocation,
    pub(crate) data: JsonValue,
}

pub(crate) fn registered_lint_package(ctx: &LintContext) -> JsonValue {
    serde_json::json!({
        "version": 1,
        "root": ctx.source.root.display().to_string(),
        "manifest": package_manifest_data(ctx),
        "qualifiers": ctx.index.qualifiers.iter()
            .map(|(id, qualifier)| (id.clone(), qualifier_data(ctx, qualifier)))
            .collect::<BTreeMap<_, _>>(),
        "variables": ctx.index.variables.iter()
            .map(|(id, variable)| (id.clone(), variable_data(ctx, variable)))
            .collect::<BTreeMap<_, _>>(),
        "catalogs": ctx.index.catalogs.iter()
            .map(|(id, catalog)| (id.clone(), catalog_data(ctx, catalog)))
            .collect::<BTreeMap<_, _>>(),
        "evaluation_contexts": ctx.index.evaluation_contexts.iter()
            .map(|(id, evaluation_context)| (id.clone(), evaluation_context_data(ctx, evaluation_context)))
            .collect::<BTreeMap<_, _>>(),
    })
}

pub(crate) fn registered_lint_targets(
    ctx: &LintContext,
    selector: &RegisteredLintSelector,
) -> Vec<RegisteredLintTargetInstance> {
    match &selector.address {
        RegisteredLintAddress::Package => registered_package_target(ctx).into_iter().collect(),
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
        RegisteredLintAddress::EvaluationContexts => ctx
            .index
            .evaluation_contexts
            .values()
            .map(|evaluation_context| registered_evaluation_context_target(ctx, evaluation_context))
            .collect(),
        RegisteredLintAddress::EvaluationContext { id } => ctx
            .index
            .evaluation_contexts
            .get(id)
            .map(|evaluation_context| registered_evaluation_context_target(ctx, evaluation_context))
            .into_iter()
            .collect(),
        RegisteredLintAddress::EvaluationContextSamples { evaluation_context } => ctx
            .index
            .evaluation_context_samples
            .get(evaluation_context)
            .into_iter()
            .flat_map(|entries| {
                entries
                    .values()
                    .map(registered_evaluation_context_sample_target)
            })
            .collect(),
        RegisteredLintAddress::EvaluationContextSample {
            evaluation_context,
            key,
        } => ctx
            .index
            .evaluation_context_samples
            .get(evaluation_context)
            .and_then(|entries| entries.get(key))
            .map(registered_evaluation_context_sample_target)
            .into_iter()
            .collect(),
    }
}

fn registered_package_target(ctx: &LintContext) -> Option<RegisteredLintTargetInstance> {
    let manifest = ctx.index.manifest.as_ref()?;
    Some(RegisteredLintTargetInstance {
        target: SemanticEntity::Package.into(),
        location: registered_package_location(ctx, manifest, None),
        data: package_target_data(ctx),
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

fn registered_evaluation_context_target(
    ctx: &LintContext,
    evaluation_context: &EvaluationContextNode,
) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: evaluation_context.target(),
        location: evaluation_context.location.clone(),
        data: evaluation_context_data(ctx, evaluation_context),
    }
}

fn registered_evaluation_context_sample_target(
    entry: &EvaluationContextSampleNode,
) -> RegisteredLintTargetInstance {
    RegisteredLintTargetInstance {
        target: entry.target(),
        location: entry.location.clone(),
        data: evaluation_context_sample_data(entry),
    }
}

fn package_target_data(ctx: &LintContext) -> JsonValue {
    let extends = ctx
        .index
        .manifest
        .as_ref()
        .map(package_extends_data)
        .unwrap_or_default();
    serde_json::json!({
        "kind": "package",
        "root": ctx.source.root.display().to_string(),
        "manifest": package_manifest_data(ctx),
        "extends": extends,
    })
}

fn package_manifest_data(ctx: &LintContext) -> JsonValue {
    let Some(manifest) = &ctx.index.manifest else {
        return JsonValue::Null;
    };
    let document = ctx.source.documents.get(&manifest.doc);
    serde_json::json!({
        "kind": "package",
        "uri": document.map(|document| document.uri.clone()),
        "path": document.map(|document| document.path.clone()).unwrap_or_else(|| "rototo-package.toml".to_owned()),
        "toml": parsed_toml_json(ctx, manifest.doc),
        "extends": package_extends_data(manifest),
    })
}

fn package_extends_data(manifest: &ManifestNode) -> Vec<String> {
    match &manifest.extends {
        PackageExtendsCollection::Sources { values, .. } => {
            values.iter().map(|value| value.source.clone()).collect()
        }
        PackageExtendsCollection::Missing | PackageExtendsCollection::Invalid { .. } => Vec::new(),
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

fn evaluation_context_data(
    ctx: &LintContext,
    evaluation_context: &EvaluationContextNode,
) -> JsonValue {
    serde_json::json!({
        "kind": "evaluation_context",
        "id": evaluation_context.id,
        "path": evaluation_context.path,
        "json": evaluation_context.json.clone().unwrap_or(JsonValue::Null),
        "samples": ctx.index.evaluation_context_samples.get(&evaluation_context.id)
            .map(|samples| samples.iter()
                .map(|(key, entry)| (key.clone(), evaluation_context_sample_data(entry)))
                .collect::<BTreeMap<_, _>>())
            .unwrap_or_default(),
    })
}

fn evaluation_context_sample_data(entry: &EvaluationContextSampleNode) -> JsonValue {
    serde_json::json!({
        "kind": "evaluation_context_sample",
        "evaluation_context": entry.evaluation_context_id,
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
