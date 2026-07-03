use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, SemanticEntity, SemanticField, SemanticTarget};
use crate::error::Result;
use crate::expression::Expression;
use crate::model::{PackageDiff, ResolutionImpact, SemanticChange, VariableResolution};

use super::index::*;
use super::{LintInput, PackageLintSnapshot, compile_runtime_package_from_snapshot};

pub(crate) async fn diff_packages(
    before_root: &Path,
    after_root: &Path,
    context: Option<&JsonValue>,
) -> Result<PackageDiff> {
    let before = super::lint_package_snapshot(LintInput::new(before_root.to_path_buf())).await?;
    let after = super::lint_package_snapshot(LintInput::new(after_root.to_path_buf())).await?;
    let before_model = PackageSemanticModel::from_snapshot(&before);
    let after_model = PackageSemanticModel::from_snapshot(&after);

    let mut changes = Vec::new();
    diff_variables(&before_model, &after_model, &mut changes);
    diff_catalogs(&before_model, &after_model, &mut changes);
    diff_layers(&before_model, &after_model, &mut changes);

    let resolution_impacts = match context {
        Some(context) => resolution_impacts(&before, &after, context).await?,
        None => Vec::new(),
    };

    Ok(PackageDiff {
        before: before_root.display().to_string(),
        after: after_root.display().to_string(),
        changes,
        resolution_impacts,
    })
}

#[derive(Default)]
struct PackageSemanticModel {
    variables: BTreeMap<String, VariableSemantic>,
    catalogs: BTreeMap<String, CatalogSemantic>,
    catalog_entries: BTreeMap<(String, String), CatalogEntrySemantic>,
    layers: BTreeMap<String, LayerSemantic>,
    allocations: BTreeMap<(String, String), AllocationSemantic>,
}

impl PackageSemanticModel {
    fn from_snapshot(snapshot: &PackageLintSnapshot) -> Self {
        Self {
            variables: snapshot
                .index
                .variables
                .values()
                .map(|variable| (variable.id.clone(), VariableSemantic::from_node(variable)))
                .collect(),
            catalogs: snapshot
                .index
                .catalogs
                .values()
                .map(|catalog| (catalog.id.clone(), CatalogSemantic::from_node(catalog)))
                .collect(),
            catalog_entries: snapshot
                .index
                .catalog_entries
                .values()
                .flat_map(|entries| entries.values())
                .map(|entry| {
                    (
                        (entry.catalog_id.clone(), entry.key.clone()),
                        CatalogEntrySemantic::from_node(entry),
                    )
                })
                .collect(),
            layers: snapshot
                .index
                .layers
                .values()
                .map(|layer| (layer.id.clone(), LayerSemantic::from_node(layer)))
                .collect(),
            allocations: snapshot
                .index
                .layers
                .values()
                .flat_map(|layer| {
                    layer.allocations.iter().filter_map(|allocation| {
                        let ProjectField::Present(id) = &allocation.id else {
                            return None;
                        };
                        Some((
                            (layer.id.clone(), id.value.clone()),
                            AllocationSemantic::from_node(layer, allocation),
                        ))
                    })
                })
                .collect(),
        }
    }
}

struct LayerSemantic {
    target: SemanticTarget,
    location: DiagnosticLocation,
    /// The diversion as one comparable value: the unit expression and the
    /// bucket count. Changing either moves every unit's position.
    diversion: JsonValue,
}

impl LayerSemantic {
    fn from_node(layer: &LayerNode) -> Self {
        Self {
            target: layer.target(),
            location: layer.location.clone(),
            diversion: serde_json::json!({
                "unit": present_expression_source_field(&layer.unit),
                "buckets": match &layer.buckets {
                    ProjectField::Present(buckets) => JsonValue::from(buckets.value),
                    _ => JsonValue::Null,
                },
            }),
        }
    }
}

struct AllocationSemantic {
    target: SemanticTarget,
    location: DiagnosticLocation,
    status: JsonValue,
    eligibility: JsonValue,
    /// Arms as name -> bucket claim, the shape whose change moves traffic.
    arms: JsonValue,
}

impl AllocationSemantic {
    fn from_node(layer: &LayerNode, allocation: &AllocationNode) -> Self {
        let arms: serde_json::Map<String, JsonValue> = allocation
            .arms
            .iter()
            .filter_map(|arm| match (&arm.name, &arm.buckets) {
                (ProjectField::Present(name), ProjectField::Present(buckets)) => {
                    Some((name.value.clone(), JsonValue::from(buckets.value.clone())))
                }
                _ => None,
            })
            .collect();
        Self {
            target: layer.target(),
            location: allocation.location.clone(),
            status: match &allocation.status {
                Some(ProjectField::Present(status)) => JsonValue::from(status.value.clone()),
                _ => JsonValue::from("running"),
            },
            eligibility: match &allocation.eligibility {
                Some(ProjectField::Present(eligibility)) => {
                    JsonValue::from(eligibility.value.source().to_owned())
                }
                _ => JsonValue::Null,
            },
            arms: JsonValue::Object(arms),
        }
    }
}

fn present_expression_source_field(field: &ProjectField<Expression>) -> JsonValue {
    match field {
        ProjectField::Present(expression) => JsonValue::from(expression.value.source().to_owned()),
        _ => JsonValue::Null,
    }
}

struct VariableSemantic {
    target: SemanticTarget,
    location: DiagnosticLocation,
    type_source: Option<FieldSemantic<JsonValue>>,
    values: BTreeMap<String, ValueSemantic>,
    resolve_default: Option<FieldSemantic<JsonValue>>,
    rules: Vec<RuleSemantic>,
    /// The resolution method plus its query or allocation parameters as one
    /// comparable value; rules diff separately with per-rule kinds.
    resolve_shape: Option<FieldSemantic<JsonValue>>,
}

impl VariableSemantic {
    fn from_node(variable: &VariableNode) -> Self {
        let (type_source, type_location, type_field) = variable_type_source(variable);
        let resolve_default = match &variable.resolve {
            ResolveNode::Resolve { default, .. } => match default.as_ref() {
                ProjectField::Present(value) => Some(FieldSemantic {
                    target: variable.field_target(SemanticField::VariableResolveDefault),
                    location: value.location.clone(),
                    value: value.value.clone(),
                }),
                ProjectField::Invalid { location } | ProjectField::Missing { location } => {
                    Some(FieldSemantic {
                        target: variable.field_target(SemanticField::VariableResolveDefault),
                        location: location.clone(),
                        value: JsonValue::Null,
                    })
                }
            },
            ResolveNode::Missing { .. } | ResolveNode::Invalid { .. } => None,
        };
        let rules = match &variable.resolve {
            ResolveNode::Resolve {
                rules: RuleCollection::Rules(rules),
                ..
            } => rules
                .iter()
                .map(|rule| RuleSemantic::from_node(&variable.id, rule))
                .collect(),
            ResolveNode::Resolve { .. }
            | ResolveNode::Missing { .. }
            | ResolveNode::Invalid { .. } => Vec::new(),
        };

        let resolve_shape = match &variable.resolve {
            ResolveNode::Resolve {
                location,
                method,
                query,
                assignments,
                ..
            } => Some(FieldSemantic {
                target: variable.field_target(SemanticField::VariableResolve),
                location: location.clone(),
                value: serde_json::json!({
                    "method": method
                        .as_ref()
                        .map(|method| method.value.clone())
                        .unwrap_or_else(|| "rules".to_owned()),
                    "query": query.as_ref().map(|query| serde_json::json!({
                        "from": present_string_project_field(&query.from),
                        "filter": query
                            .filter
                            .as_ref()
                            .map(present_expression_source_field),
                        "sort": query.sort.as_ref().map(present_expression_source_field),
                        "order": query
                            .order
                            .as_ref()
                            .and_then(|order| match order {
                                ProjectField::Present(order) => Some(order.value.clone()),
                                _ => None,
                            }),
                        "limit": query.limit.as_ref().and_then(|limit| match limit {
                            ProjectField::Present(limit) => Some(limit.value),
                            _ => None,
                        }),
                    })),
                    "allocation": assignments.as_ref().map(|assignments| serde_json::json!({
                        "allocation": present_string_project_field(&assignments.allocation),
                        "assigns": assignments
                            .assigns
                            .iter()
                            .filter_map(|assign| match (&assign.arm, &assign.value) {
                                (
                                    ProjectField::Present(arm),
                                    ProjectField::Present(value),
                                ) => Some((arm.value.clone(), value.value.clone())),
                                _ => None,
                            })
                            .collect::<serde_json::Map<String, JsonValue>>(),
                    })),
                }),
            }),
            ResolveNode::Missing { .. } | ResolveNode::Invalid { .. } => None,
        };

        Self {
            target: variable.target(),
            location: variable.location.clone(),
            type_source: type_source.map(|value| FieldSemantic {
                target: variable.field_target(type_field),
                location: type_location,
                value,
            }),
            values: variable
                .values
                .inline_values
                .values()
                .map(|value| (value.key.clone(), ValueSemantic::from_node(value)))
                .collect(),
            resolve_default,
            rules,
            resolve_shape,
        }
    }
}

struct CatalogSemantic {
    target: SemanticTarget,
    location: DiagnosticLocation,
    json: Option<JsonValue>,
}

impl CatalogSemantic {
    fn from_node(catalog: &CatalogNode) -> Self {
        Self {
            target: catalog.target(),
            location: catalog.location.clone(),
            json: catalog.json.clone(),
        }
    }
}

struct CatalogEntrySemantic {
    target: SemanticTarget,
    location: DiagnosticLocation,
    value: JsonValue,
}

impl CatalogEntrySemantic {
    fn from_node(entry: &CatalogEntryNode) -> Self {
        Self {
            target: entry.target(),
            location: entry.location.clone(),
            value: entry.value.clone(),
        }
    }
}

struct ValueSemantic {
    target: SemanticTarget,
    location: DiagnosticLocation,
    value: JsonValue,
}

impl ValueSemantic {
    fn from_node(value: &ValueNode) -> Self {
        Self {
            target: value.target(),
            location: value.location.clone(),
            value: value.value.clone(),
        }
    }
}

struct RuleSemantic {
    target: SemanticTarget,
    location: DiagnosticLocation,
    when: Option<FieldSemantic<String>>,
    value: Option<FieldSemantic<JsonValue>>,
}

impl RuleSemantic {
    fn from_node(variable_id: &str, rule: &VariableRuleNode) -> Self {
        Self {
            target: rule.target(variable_id),
            location: rule.location.clone(),
            when: rule.when.as_ref().and_then(|field| {
                present_expression_field(
                    field,
                    rule.field_target(variable_id, SemanticField::VariableRuleWhen),
                )
            }),
            value: present_json_field(
                &rule.value,
                rule.field_target(variable_id, SemanticField::VariableRuleValue),
            ),
        }
    }
}

struct FieldSemantic<T> {
    target: SemanticTarget,
    location: DiagnosticLocation,
    value: T,
}

fn diff_variables(
    before: &PackageSemanticModel,
    after: &PackageSemanticModel,
    changes: &mut Vec<SemanticChange>,
) {
    for id in sorted_keys(before.variables.keys(), after.variables.keys()) {
        match (before.variables.get(&id), after.variables.get(&id)) {
            (None, Some(after)) => {
                push_added(changes, "variable_added", &after.target, &after.location)
            }
            (Some(before), None) => push_removed(
                changes,
                "variable_removed",
                &before.target,
                &before.location,
            ),
            (Some(before), Some(after)) => {
                diff_optional_field(
                    changes,
                    "variable_type_changed",
                    &before.type_source,
                    &after.type_source,
                );
                diff_values(&before.values, &after.values, changes);
                diff_optional_field(
                    changes,
                    "variable_resolve_default_changed",
                    &before.resolve_default,
                    &after.resolve_default,
                );
                diff_rules(&before.rules, &after.rules, changes);
                diff_optional_field(
                    changes,
                    "variable_resolution_changed",
                    &before.resolve_shape,
                    &after.resolve_shape,
                );
            }
            (None, None) => {}
        }
    }
}

fn diff_values(
    before: &BTreeMap<String, ValueSemantic>,
    after: &BTreeMap<String, ValueSemantic>,
    changes: &mut Vec<SemanticChange>,
) {
    for key in sorted_keys(before.keys(), after.keys()) {
        match (before.get(&key), after.get(&key)) {
            (None, Some(after)) => push_added_value(
                changes,
                "variable_value_added",
                &after.target,
                &after.location,
                &after.value,
            ),
            (Some(before), None) => push_removed_value(
                changes,
                "variable_value_removed",
                &before.target,
                &before.location,
                &before.value,
            ),
            (Some(before), Some(after)) => diff_json_value(
                changes,
                JsonDiffSide {
                    target: &before.target,
                    location: &before.location,
                    value: &before.value,
                },
                JsonDiffSide {
                    target: &after.target,
                    location: &after.location,
                    value: &after.value,
                },
                Vec::new(),
                "variable_value_changed",
            ),
            (None, None) => {}
        }
    }
}

fn diff_rules(before: &[RuleSemantic], after: &[RuleSemantic], changes: &mut Vec<SemanticChange>) {
    let max = before.len().max(after.len());
    for index in 0..max {
        match (before.get(index), after.get(index)) {
            (None, Some(after)) => push_added(
                changes,
                "variable_rule_added",
                &after.target,
                &after.location,
            ),
            (Some(before), None) => push_removed(
                changes,
                "variable_rule_removed",
                &before.target,
                &before.location,
            ),
            (Some(before), Some(after)) => {
                diff_optional_field(
                    changes,
                    "variable_rule_when_changed",
                    &before.when,
                    &after.when,
                );
                diff_optional_field(
                    changes,
                    "variable_rule_value_changed",
                    &before.value,
                    &after.value,
                );
            }
            (None, None) => {}
        }
    }
}

fn diff_layers(
    before: &PackageSemanticModel,
    after: &PackageSemanticModel,
    changes: &mut Vec<SemanticChange>,
) {
    for id in sorted_keys(before.layers.keys(), after.layers.keys()) {
        match (before.layers.get(&id), after.layers.get(&id)) {
            (None, Some(after)) => {
                push_added(changes, "layer_added", &after.target, &after.location)
            }
            (Some(before), None) => {
                push_removed(changes, "layer_removed", &before.target, &before.location)
            }
            (Some(before), Some(after)) => {
                if before.diversion != after.diversion {
                    push_json_change(
                        changes,
                        "layer_diversion_changed",
                        &after.target,
                        &after.location,
                        &before.diversion,
                        &after.diversion,
                    );
                }
            }
            (None, None) => {}
        }
    }
    for key in sorted_keys(before.allocations.keys(), after.allocations.keys()) {
        match (before.allocations.get(&key), after.allocations.get(&key)) {
            (None, Some(after)) => {
                push_added(changes, "allocation_added", &after.target, &after.location)
            }
            (Some(before), None) => push_removed(
                changes,
                "allocation_removed",
                &before.target,
                &before.location,
            ),
            (Some(before), Some(after)) => {
                for (kind, before_value, after_value) in [
                    ("allocation_status_changed", &before.status, &after.status),
                    (
                        "allocation_eligibility_changed",
                        &before.eligibility,
                        &after.eligibility,
                    ),
                    ("allocation_arms_changed", &before.arms, &after.arms),
                ] {
                    if before_value != after_value {
                        push_json_change(
                            changes,
                            kind,
                            &after.target,
                            &after.location,
                            before_value,
                            after_value,
                        );
                    }
                }
            }
            (None, None) => {}
        }
    }
}

fn diff_catalogs(
    before: &PackageSemanticModel,
    after: &PackageSemanticModel,
    changes: &mut Vec<SemanticChange>,
) {
    for id in sorted_keys(before.catalogs.keys(), after.catalogs.keys()) {
        match (before.catalogs.get(&id), after.catalogs.get(&id)) {
            (None, Some(after)) => {
                push_added(changes, "catalog_added", &after.target, &after.location)
            }
            (Some(before), None) => {
                push_removed(changes, "catalog_removed", &before.target, &before.location)
            }
            (Some(before), Some(after)) => {
                if let (Some(before_json), Some(after_json)) = (&before.json, &after.json) {
                    diff_json_value(
                        changes,
                        JsonDiffSide {
                            target: &before.target,
                            location: &before.location,
                            value: before_json,
                        },
                        JsonDiffSide {
                            target: &after.target,
                            location: &after.location,
                            value: after_json,
                        },
                        Vec::new(),
                        "catalog_schema_changed",
                    );
                }
            }
            (None, None) => {}
        }
    }
    for key in sorted_keys(before.catalog_entries.keys(), after.catalog_entries.keys()) {
        match (
            before.catalog_entries.get(&key),
            after.catalog_entries.get(&key),
        ) {
            (None, Some(after)) => push_added_value(
                changes,
                "catalog_entry_added",
                &after.target,
                &after.location,
                &after.value,
            ),
            (Some(before), None) => push_removed_value(
                changes,
                "catalog_entry_removed",
                &before.target,
                &before.location,
                &before.value,
            ),
            (Some(before), Some(after)) => diff_json_value(
                changes,
                JsonDiffSide {
                    target: &before.target,
                    location: &before.location,
                    value: &before.value,
                },
                JsonDiffSide {
                    target: &after.target,
                    location: &after.location,
                    value: &after.value,
                },
                Vec::new(),
                "catalog_entry_changed",
            ),
            (None, None) => {}
        }
    }
}

fn diff_optional_field<T: serde::Serialize + PartialEq>(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    before: &Option<FieldSemantic<T>>,
    after: &Option<FieldSemantic<T>>,
) {
    match (before, after) {
        (None, Some(after)) => push_added_json(
            changes,
            kind,
            &after.target,
            &after.location,
            json(&after.value),
        ),
        (Some(before), None) => push_removed_json(
            changes,
            kind,
            &before.target,
            &before.location,
            json(&before.value),
        ),
        (Some(before), Some(after)) if before.value != after.value => {
            changes.push(SemanticChange {
                kind: kind.to_owned(),
                target: after.target.clone(),
                before: Some(json(&before.value)),
                after: Some(json(&after.value)),
                before_location: Some(before.location.clone()),
                after_location: Some(after.location.clone()),
            });
        }
        (Some(_), Some(_)) | (None, None) => {}
    }
}

struct JsonDiffSide<'a> {
    target: &'a SemanticTarget,
    location: &'a DiagnosticLocation,
    value: &'a JsonValue,
}

fn diff_json_value(
    changes: &mut Vec<SemanticChange>,
    before: JsonDiffSide<'_>,
    after: JsonDiffSide<'_>,
    path: Vec<String>,
    kind: &'static str,
) {
    let before_value = before.value;
    let after_value = after.value;
    if before_value == after_value {
        return;
    }

    match (before_value, after_value) {
        (JsonValue::Object(before_obj), JsonValue::Object(after_obj)) => {
            for key in sorted_keys(before_obj.keys(), after_obj.keys()) {
                let mut child_path = path.clone();
                child_path.push(key.clone());
                match (before_obj.get(&key), after_obj.get(&key)) {
                    (Some(before_child), Some(after_child)) => diff_json_value(
                        changes,
                        JsonDiffSide {
                            target: before.target,
                            location: before.location,
                            value: before_child,
                        },
                        JsonDiffSide {
                            target: after.target,
                            location: after.location,
                            value: after_child,
                        },
                        child_path,
                        kind,
                    ),
                    (Some(before_child), None) => push_json_path_change(
                        changes,
                        kind,
                        JsonPathChange {
                            target: before.target,
                            before_location: Some(before.location),
                            after_location: None,
                            before: Some(before_child.clone()),
                            after: None,
                            path: child_path,
                        },
                    ),
                    (None, Some(after_child)) => push_json_path_change(
                        changes,
                        kind,
                        JsonPathChange {
                            target: after.target,
                            before_location: None,
                            after_location: Some(after.location),
                            before: None,
                            after: Some(after_child.clone()),
                            path: child_path,
                        },
                    ),
                    (None, None) => {}
                }
            }
        }
        _ => push_json_path_change(
            changes,
            kind,
            JsonPathChange {
                target: after.target,
                before_location: Some(before.location),
                after_location: Some(after.location),
                before: Some(before_value.clone()),
                after: Some(after_value.clone()),
                path,
            },
        ),
    }
}

struct JsonPathChange<'a> {
    target: &'a SemanticTarget,
    before_location: Option<&'a DiagnosticLocation>,
    after_location: Option<&'a DiagnosticLocation>,
    before: Option<JsonValue>,
    after: Option<JsonValue>,
    path: Vec<String>,
}

fn push_json_path_change(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    change: JsonPathChange<'_>,
) {
    changes.push(SemanticChange {
        kind: kind.to_owned(),
        target: target_with_json_path(change.target, change.path),
        before: change.before,
        after: change.after,
        before_location: change.before_location.cloned(),
        after_location: change.after_location.cloned(),
    });
}

fn target_with_json_path(target: &SemanticTarget, path: Vec<String>) -> SemanticTarget {
    if path.is_empty() {
        return target.clone();
    }

    match &target.entity {
        SemanticEntity::Value { .. } | SemanticEntity::CatalogEntry { .. } => {
            SemanticTarget::field(target.entity.clone(), SemanticField::ValueJsonPath { path })
        }
        _ => target.clone(),
    }
}

async fn resolution_impacts(
    before: &PackageLintSnapshot,
    after: &PackageLintSnapshot,
    context: &JsonValue,
) -> Result<Vec<ResolutionImpact>> {
    let before_runtime = compile_runtime_package_from_snapshot(before)?;
    let after_runtime = compile_runtime_package_from_snapshot(after)?;
    before_runtime.validate_context(context)?;
    after_runtime.validate_context(context)?;
    let before_resolutions = crate::resolve::resolve_variables_unchecked(&before_runtime, context)?
        .into_iter()
        .map(|resolution| (resolution.id.clone(), resolution))
        .collect::<BTreeMap<_, _>>();
    let after_resolutions = crate::resolve::resolve_variables_unchecked(&after_runtime, context)?
        .into_iter()
        .map(|resolution| (resolution.id.clone(), resolution))
        .collect::<BTreeMap<_, _>>();

    let mut impacts = Vec::new();
    for variable in sorted_keys(before_resolutions.keys(), after_resolutions.keys()) {
        let (Some(before), Some(after)) = (
            before_resolutions.get(&variable),
            after_resolutions.get(&variable),
        ) else {
            continue;
        };
        if variable_resolution_eq(before, after) {
            continue;
        }
        impacts.push(ResolutionImpact {
            variable,
            before: before.clone(),
            after: after.clone(),
        });
    }
    Ok(impacts)
}

fn variable_resolution_eq(left: &VariableResolution, right: &VariableResolution) -> bool {
    left.value == right.value
        && serde_json::to_value(&left.source).ok() == serde_json::to_value(&right.source).ok()
}

fn variable_type_source(
    variable: &VariableNode,
) -> (Option<JsonValue>, DiagnosticLocation, SemanticField) {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => (
            Some(serde_json::json!({
                "kind": "primitive",
                "type": type_name.value,
            })),
            type_name.location.clone(),
            SemanticField::VariableType,
        ),
        TypeSourceNode::Catalog(catalog) => (
            Some(serde_json::json!({
                "kind": "catalog",
                "catalog": catalog.value,
            })),
            catalog.location.clone(),
            SemanticField::VariableType,
        ),
        TypeSourceNode::Schema(schema) => (
            Some(serde_json::json!({
                "kind": "schema",
                "schema": schema.value,
            })),
            schema.location.clone(),
            SemanticField::VariableSchema,
        ),
        TypeSourceNode::Missing { location }
        | TypeSourceNode::Conflict { location }
        | TypeSourceNode::Invalid { location } => {
            (None, location.clone(), SemanticField::VariableType)
        }
    }
}

fn present_expression_field(
    field: &ProjectField<crate::expression::Expression>,
    target: SemanticTarget,
) -> Option<FieldSemantic<String>> {
    match field {
        ProjectField::Present(value) => Some(FieldSemantic {
            target,
            location: value.location.clone(),
            value: value.value.source().to_owned(),
        }),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn present_json_field(
    field: &ProjectField<JsonValue>,
    target: SemanticTarget,
) -> Option<FieldSemantic<JsonValue>> {
    match field {
        ProjectField::Present(value) => Some(FieldSemantic {
            target,
            location: value.location.clone(),
            value: value.value.clone(),
        }),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn present_string_project_field(field: &ProjectField<String>) -> JsonValue {
    match field {
        ProjectField::Present(value) => JsonValue::from(value.value.clone()),
        _ => JsonValue::Null,
    }
}

fn push_json_change(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    target: &SemanticTarget,
    location: &DiagnosticLocation,
    before: &JsonValue,
    after: &JsonValue,
) {
    changes.push(SemanticChange {
        kind: kind.to_owned(),
        target: target.clone(),
        before: Some(before.clone()),
        after: Some(after.clone()),
        before_location: None,
        after_location: Some(location.clone()),
    });
}

fn push_added(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    target: &SemanticTarget,
    location: &DiagnosticLocation,
) {
    changes.push(SemanticChange {
        kind: kind.to_owned(),
        target: target.clone(),
        before: None,
        after: None,
        before_location: None,
        after_location: Some(location.clone()),
    });
}

fn push_removed(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    target: &SemanticTarget,
    location: &DiagnosticLocation,
) {
    changes.push(SemanticChange {
        kind: kind.to_owned(),
        target: target.clone(),
        before: None,
        after: None,
        before_location: Some(location.clone()),
        after_location: None,
    });
}

fn push_added_value(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    target: &SemanticTarget,
    location: &DiagnosticLocation,
    value: &JsonValue,
) {
    push_added_json(changes, kind, target, location, value.clone());
}

fn push_removed_value(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    target: &SemanticTarget,
    location: &DiagnosticLocation,
    value: &JsonValue,
) {
    push_removed_json(changes, kind, target, location, value.clone());
}

fn push_added_json(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    target: &SemanticTarget,
    location: &DiagnosticLocation,
    value: JsonValue,
) {
    changes.push(SemanticChange {
        kind: kind.to_owned(),
        target: target.clone(),
        before: None,
        after: Some(value),
        before_location: None,
        after_location: Some(location.clone()),
    });
}

fn push_removed_json(
    changes: &mut Vec<SemanticChange>,
    kind: &'static str,
    target: &SemanticTarget,
    location: &DiagnosticLocation,
    value: JsonValue,
) {
    changes.push(SemanticChange {
        kind: kind.to_owned(),
        target: target.clone(),
        before: Some(value),
        after: None,
        before_location: Some(location.clone()),
        after_location: None,
    });
}

fn sorted_keys<'a, T, I, J>(left: I, right: J) -> Vec<T>
where
    T: Ord + Clone + 'a,
    I: IntoIterator<Item = &'a T>,
    J: IntoIterator<Item = &'a T>,
{
    left.into_iter()
        .chain(right)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn json<T: serde::Serialize>(value: &T) -> JsonValue {
    serde_json::to_value(value).unwrap_or(JsonValue::Null)
}
