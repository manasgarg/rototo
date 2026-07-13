use std::collections::BTreeMap;

use serde_json::Value as JsonValue;

use crate::address::{EntityClass, StepId};
use crate::diagnostics::{DiagnosticLocation, SemanticEntity, SemanticTarget};
use crate::lint::custom::RegisteredLintSelector;
use crate::lint::custom::marshal::{expanded_variable_toml_json, parsed_toml_json};
use crate::lint::engine::{LintContext, variable_values};
use crate::lint::index::*;

use super::locations::registered_package_location;

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
        "variables": ctx.index.variables.iter()
            .map(|(id, variable)| (id.clone(), variable_data(ctx, variable)))
            .collect::<BTreeMap<_, _>>(),
        "catalogs": ctx.index.catalogs.iter()
            .map(|(id, catalog)| (id.clone(), catalog_data(ctx, catalog)))
            .collect::<BTreeMap<_, _>>(),
        "layers": ctx.index.layers.iter()
            .map(|(id, layer)| (id.clone(), layer_data(layer)))
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
    let steps = selector.address.steps();
    match steps[0].class {
        EntityClass::Package => registered_package_target(ctx).into_iter().collect(),
        EntityClass::Variable => ctx
            .index
            .variables
            .iter()
            .filter(|(id, _)| step_id_matches(&steps[0].id, id))
            .filter_map(|(_, variable)| registered_variable_target(ctx, variable))
            .collect(),
        EntityClass::Catalog if steps.len() == 1 => ctx
            .index
            .catalogs
            .iter()
            .filter(|(id, _)| step_id_matches(&steps[0].id, id))
            .map(|(_, catalog)| registered_catalog_target(ctx, catalog))
            .collect(),
        EntityClass::Catalog => {
            let StepId::Entity(catalog_id) = &steps[0].id else {
                return Vec::new();
            };
            ctx.index
                .catalog_entries
                .get(catalog_id)
                .into_iter()
                .flat_map(|entries| {
                    entries
                        .iter()
                        .filter(|(key, _)| step_id_matches(&steps[1].id, key))
                        .map(|(_, entry)| registered_catalog_entry_target(entry))
                })
                .collect()
        }
        EntityClass::EvaluationContext if steps.len() == 1 => ctx
            .index
            .evaluation_contexts
            .iter()
            .filter(|(id, _)| step_id_matches(&steps[0].id, id))
            .map(|(_, evaluation_context)| {
                registered_evaluation_context_target(ctx, evaluation_context)
            })
            .collect(),
        EntityClass::EvaluationContext => {
            let StepId::Entity(evaluation_context_id) = &steps[0].id else {
                return Vec::new();
            };
            ctx.index
                .evaluation_context_samples
                .get(evaluation_context_id)
                .into_iter()
                .flat_map(|samples| {
                    samples
                        .iter()
                        .filter(|(key, _)| step_id_matches(&steps[1].id, key))
                        .map(|(_, sample)| registered_evaluation_context_sample_target(sample))
                })
                .collect()
        }
        // Registration acceptance already rejected every other class.
        _ => Vec::new(),
    }
}

/// Whether one step's id selector admits an entity id: an empty id is the
/// collective, a subtree matches its namespace prefix at a path boundary,
/// and a concrete id matches exactly.
fn step_id_matches(selector: &StepId, id: &str) -> bool {
    match selector {
        StepId::Empty => true,
        StepId::Subtree(prefix) => id
            .strip_prefix(prefix.as_str())
            .is_some_and(|rest| rest.starts_with('/')),
        StepId::Entity(entity) => entity == id,
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

fn layer_data(layer: &LayerNode) -> JsonValue {
    serde_json::json!({
        "kind": "layer",
        "id": layer.id,
        "path": layer.location.path.clone(),
        "unit": project_expression_field(&layer.unit),
        "buckets": project_json_number(&layer.buckets),
        "allocations": layer.allocations.iter().map(|allocation| serde_json::json!({
            "kind": "allocation",
            "index": allocation.index,
            "id": project_string(&allocation.id),
            "status": optional_project_string(&allocation.status),
            "arms": allocation.arms.iter().map(|arm| serde_json::json!({
                "kind": "arm",
                "index": arm.index,
                "name": project_string(&arm.name),
                "buckets": project_string(&arm.buckets),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn project_expression_field(field: &ProjectField<crate::expression::Expression>) -> JsonValue {
    match field {
        ProjectField::Present(value) => JsonValue::String(value.value.source().to_owned()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => JsonValue::Null,
    }
}

fn project_json_number(field: &ProjectField<i64>) -> JsonValue {
    match field {
        ProjectField::Present(value) => serde_json::json!(value.value),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => JsonValue::Null,
    }
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
