//! A serializable projection of the semantic and reference indexes.
//!
//! Tools (the admin app, editors) consume this model instead of parsing
//! package files themselves, so rototo's parse stays the single semantic
//! authority. Locations carry source ranges so writers can splice edits at
//! positions reported by the same parse that produced the rendering.

use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, SourceRange};
use crate::expression::Expression;

use super::PackageLintSnapshot;
use super::index::{ProjectField, ResolveNode, RuleCollection, TypeSourceNode};
use super::references::{ReferenceSource, ReferenceTarget};

pub const SEMANTIC_MODEL_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSemanticModel {
    pub version: u32,
    pub variables: Vec<VariableModel>,
    pub layers: Vec<LayerModel>,
    pub catalogs: Vec<CatalogModel>,
    pub catalog_entries: Vec<CatalogEntryModel>,
    pub evaluation_contexts: Vec<EvaluationContextModel>,
    pub evaluation_context_samples: Vec<EvaluationContextSampleModel>,
    pub linters: Vec<LinterModel>,
    pub references: Vec<ReferenceModel>,
    pub variable_evaluation_contexts: Vec<VariableEvaluationContextModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLocation {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceRange>,
}

/// A scalar field as projected: `value` is present only when the field parsed
/// to the expected shape; the location always points at the field (or where
/// it should be) for diagnostics and edits.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelField {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub location: ModelLocation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelValueField {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<JsonValue>,
    pub location: ModelLocation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableModel {
    pub id: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub declaration: DeclarationModel,
    pub values: Vec<ValueModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values_section: Option<ModelLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolve: Option<ResolveModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationModel {
    /// "primitive", "catalog", "schema", "missing", "conflict", or "invalid".
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub location: ModelLocation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueModel {
    pub key: String,
    pub location: ModelLocation,
    pub value: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveModel {
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<ModelValueField>,
    pub rules: Vec<RuleModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation: Option<ModelField>,
    pub assigns: Vec<AssignModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignModel {
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerModel {
    pub id: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buckets: Option<i64>,
    pub allocations: Vec<AllocationModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationModel {
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eligibility: Option<ModelField>,
    pub arms: Vec<ArmModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmModel {
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buckets: Option<String>,
}

/// The `method = "query"` parameters on `[resolve]`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryModel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<ModelField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleModel {
    pub index: usize,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<ModelValueField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    pub id: String,
    pub path: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntryModel {
    pub catalog: String,
    pub key: String,
    pub location: ModelLocation,
    pub value: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationContextModel {
    pub id: String,
    pub path: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationContextSampleModel {
    pub evaluation_context: String,
    pub key: String,
    pub path: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableEvaluationContextModel {
    pub variable: String,
    pub evaluation_contexts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinterModel {
    pub path: String,
    pub location: ModelLocation,
    pub rules: Vec<LinterRuleModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinterRuleModel {
    pub id: String,
    pub title: String,
    pub help: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceModel {
    pub from: ModelEntityRef,
    pub to: ModelEntityRef,
    pub location: ModelLocation,
    /// Where in the source entity the reference sits, so tools can render
    /// the relation semantically ("rule[1] checks ...").
    pub via: ModelReferenceVia,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ModelReferenceVia {
    VariableCatalog,
    ResolveDefault,
    RuleCondition { index: usize },
    RuleValue { index: usize },
    Query,
    Allocation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ModelEntityRef {
    Variable {
        id: String,
    },
    Allocation {
        id: String,
    },
    Catalog {
        id: String,
    },
    CatalogEntry {
        catalog: String,
        key: String,
    },
    EvaluationContext {
        id: String,
    },
    EvaluationContextSample {
        evaluation_context: String,
        key: String,
    },
    Value {
        variable: String,
        key: String,
    },
    ContextAttribute {
        name: String,
    },
}

impl PackageLintSnapshot {
    pub(crate) fn semantic_model(&self) -> PackageSemanticModel {
        let index = &self.index;
        let variables = index
            .variables
            .values()
            .map(|node| {
                let declaration = match &node.type_source {
                    TypeSourceNode::Primitive(value) => DeclarationModel {
                        kind: "primitive".to_owned(),
                        value: Some(value.value.clone()),
                        location: model_location(&value.location),
                    },
                    TypeSourceNode::Catalog(value) => DeclarationModel {
                        kind: "catalog".to_owned(),
                        value: Some(value.value.clone()),
                        location: model_location(&value.location),
                    },
                    TypeSourceNode::Schema(value) => DeclarationModel {
                        kind: "schema".to_owned(),
                        value: Some(value.value.clone()),
                        location: model_location(&value.location),
                    },
                    TypeSourceNode::Missing { location } => DeclarationModel {
                        kind: "missing".to_owned(),
                        value: None,
                        location: model_location(location),
                    },
                    TypeSourceNode::Conflict { location } => DeclarationModel {
                        kind: "conflict".to_owned(),
                        value: None,
                        location: model_location(location),
                    },
                    TypeSourceNode::Invalid { location } => DeclarationModel {
                        kind: "invalid".to_owned(),
                        value: None,
                        location: model_location(location),
                    },
                };
                let resolve = match &node.resolve {
                    ResolveNode::Resolve {
                        location,
                        method,
                        default,
                        rules,
                        query,
                        assignments,
                    } => Some(ResolveModel {
                        location: model_location(location),
                        allocation: assignments
                            .as_ref()
                            .map(|assignments| model_string_field(&assignments.allocation)),
                        assigns: assignments
                            .as_ref()
                            .map(|assignments| {
                                assignments
                                    .assigns
                                    .iter()
                                    .map(|assign| AssignModel {
                                        location: model_location(&assign.location),
                                        arm: match &assign.arm {
                                            ProjectField::Present(arm) => Some(arm.value.clone()),
                                            _ => None,
                                        },
                                        value: match &assign.value {
                                            ProjectField::Present(value) => {
                                                Some(value.value.clone())
                                            }
                                            _ => None,
                                        },
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        method: method.as_ref().map(|method| ModelField {
                            value: Some(method.value.clone()),
                            location: model_location(&method.location),
                        }),
                        default: Some(model_value_field(default)),
                        query: query.as_ref().map(|query| QueryModel {
                            from: Some(model_string_field(&query.from)),
                            filter: query.filter.as_ref().map(model_expression_field),
                            sort: query.sort.as_ref().map(model_expression_field),
                            order: query.order.as_ref().map(model_string_field),
                            limit: query.limit.as_ref().map(|limit| ModelField {
                                value: match limit {
                                    ProjectField::Present(value) => Some(value.value.to_string()),
                                    _ => None,
                                },
                                location: model_location(&limit.location()),
                            }),
                        }),
                        rules: match rules {
                            RuleCollection::Rules(rules) => rules
                                .iter()
                                .map(|rule| RuleModel {
                                    index: rule.index,
                                    location: model_location(&rule.location),
                                    when: rule.when.as_ref().map(model_expression_field),
                                    value: Some(model_value_field(&rule.value)),
                                })
                                .collect(),
                            RuleCollection::Invalid { .. } => Vec::new(),
                        },
                    }),
                    ResolveNode::Missing { .. } | ResolveNode::Invalid { .. } => None,
                };
                VariableModel {
                    id: node.id.clone(),
                    location: model_location(&node.location),
                    description: present_string(&node.description),
                    declaration,
                    values: node
                        .values
                        .inline_values
                        .values()
                        .map(|value| ValueModel {
                            key: value.key.clone(),
                            location: model_location(&value.location),
                            value: value.value.clone(),
                        })
                        .collect(),
                    values_section: Some(model_location(&node.values.location)),
                    resolve,
                }
            })
            .collect();

        let catalogs = index
            .catalogs
            .values()
            .map(|node| {
                let json = node.json.as_ref();
                CatalogModel {
                    id: node.id.clone(),
                    path: node.path.clone(),
                    location: model_location(&node.location),
                    description: json
                        .and_then(|json| json.get("description"))
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    json: node.json.clone(),
                }
            })
            .collect();

        let catalog_entries = index
            .catalog_entries
            .values()
            .flat_map(|entries| entries.values())
            .map(|node| CatalogEntryModel {
                catalog: node.catalog_id.clone(),
                key: node.key.clone(),
                location: model_location(&node.location),
                value: node.value.clone(),
            })
            .collect();

        let evaluation_contexts = index
            .evaluation_contexts
            .values()
            .map(|node| {
                let json = node.json.as_ref();
                EvaluationContextModel {
                    id: node.id.clone(),
                    path: node.path.clone(),
                    location: model_location(&node.location),
                    title: json
                        .and_then(|json| json.get("title"))
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    description: json
                        .and_then(|json| json.get("description"))
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    json: node.json.clone(),
                }
            })
            .collect();

        let evaluation_context_samples = index
            .evaluation_context_samples
            .values()
            .flat_map(|entries| entries.values())
            .map(|node| EvaluationContextSampleModel {
                evaluation_context: node.evaluation_context_id.clone(),
                key: node.key.clone(),
                path: node.path.clone(),
                location: model_location(&node.location),
                value: node.value.clone(),
            })
            .collect();

        let linters = index
            .custom_lints
            .files
            .values()
            .map(|file| LinterModel {
                path: file.path.clone(),
                location: model_location(&file.location),
                rules: index
                    .custom_lints
                    .registrations
                    .iter()
                    .filter(|registration| registration.file_path == file.path)
                    .filter_map(|registration| {
                        index
                            .custom_lints
                            .rules
                            .get(&registration.rule)
                            .map(|rule| LinterRuleModel {
                                id: rule.definition.rule.as_str().to_owned(),
                                title: rule.definition.title.clone(),
                                help: rule.definition.help.clone(),
                            })
                    })
                    .collect(),
            })
            .collect();

        let references = self
            .references
            .edges()
            .iter()
            .map(|edge| ReferenceModel {
                from: reference_source_ref(&edge.source),
                to: reference_target_ref(&edge.target),
                location: model_location(&edge.location),
                via: reference_via(&edge.source),
            })
            .collect();

        let compatibility = self.evaluation_context_compatibility();
        let variable_evaluation_contexts = compatibility
            .variables
            .into_iter()
            .map(|(variable, contexts)| VariableEvaluationContextModel {
                variable,
                evaluation_contexts: contexts.into_iter().collect(),
            })
            .collect();

        let layers = index
            .layers
            .values()
            .map(|layer| LayerModel {
                id: layer.id.clone(),
                location: model_location(&layer.location),
                description: present_string(&layer.description),
                unit: match &layer.unit {
                    ProjectField::Present(unit) => Some(ModelField {
                        value: Some(unit.value.source().to_owned()),
                        location: model_location(&unit.location),
                    }),
                    _ => None,
                },
                buckets: match &layer.buckets {
                    ProjectField::Present(buckets) => Some(buckets.value),
                    _ => None,
                },
                allocations: layer
                    .allocations
                    .iter()
                    .map(|allocation| AllocationModel {
                        location: model_location(&allocation.location),
                        id: match &allocation.id {
                            ProjectField::Present(id) => Some(id.value.clone()),
                            _ => None,
                        },
                        status: match &allocation.status {
                            Some(ProjectField::Present(status)) => Some(status.value.clone()),
                            _ => None,
                        },
                        eligibility: match &allocation.eligibility {
                            Some(ProjectField::Present(eligibility)) => Some(ModelField {
                                value: Some(eligibility.value.source().to_owned()),
                                location: model_location(&eligibility.location),
                            }),
                            _ => None,
                        },
                        arms: allocation
                            .arms
                            .iter()
                            .map(|arm| ArmModel {
                                location: model_location(&arm.location),
                                name: match &arm.name {
                                    ProjectField::Present(name) => Some(name.value.clone()),
                                    _ => None,
                                },
                                buckets: match &arm.buckets {
                                    ProjectField::Present(buckets) => Some(buckets.value.clone()),
                                    _ => None,
                                },
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();

        PackageSemanticModel {
            version: SEMANTIC_MODEL_VERSION,
            variables,
            layers,
            catalogs,
            catalog_entries,
            evaluation_contexts,
            evaluation_context_samples,
            linters,
            references,
            variable_evaluation_contexts,
        }
    }
}

fn model_location(location: &DiagnosticLocation) -> ModelLocation {
    ModelLocation {
        path: location.path.clone(),
        range: location.range,
    }
}

fn model_expression_field(field: &ProjectField<Expression>) -> ModelField {
    ModelField {
        value: match field {
            ProjectField::Present(value) => Some(value.value.source().to_owned()),
            ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
        },
        location: model_location(&field.location()),
    }
}

fn model_string_field(field: &ProjectField<String>) -> ModelField {
    ModelField {
        value: match field {
            ProjectField::Present(value) => Some(value.value.clone()),
            ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
        },
        location: model_location(&field.location()),
    }
}

fn model_value_field(field: &ProjectField<JsonValue>) -> ModelValueField {
    ModelValueField {
        value: match field {
            ProjectField::Present(value) => Some(value.value.clone()),
            ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
        },
        location: model_location(&field.location()),
    }
}

fn present_string(field: &Option<ProjectField<String>>) -> Option<String> {
    match field {
        Some(ProjectField::Present(value)) => Some(value.value.clone()),
        _ => None,
    }
}

fn reference_via(source: &ReferenceSource) -> ModelReferenceVia {
    match source {
        ReferenceSource::VariableCatalog { .. } => ModelReferenceVia::VariableCatalog,
        ReferenceSource::VariableResolveDefault { .. } => ModelReferenceVia::ResolveDefault,
        ReferenceSource::VariableRuleConditionVariable { rule, .. }
        | ReferenceSource::VariableRuleContextAttribute { rule, .. } => {
            ModelReferenceVia::RuleCondition { index: *rule }
        }
        ReferenceSource::VariableRuleValue { rule, .. } => {
            ModelReferenceVia::RuleValue { index: *rule }
        }
        ReferenceSource::VariableQueryVariable { .. }
        | ReferenceSource::VariableQueryContextAttribute { .. } => ModelReferenceVia::Query,
        ReferenceSource::VariableAllocation { .. } => ModelReferenceVia::Allocation,
    }
}

fn reference_source_ref(source: &ReferenceSource) -> ModelEntityRef {
    match source {
        ReferenceSource::VariableCatalog { variable }
        | ReferenceSource::VariableResolveDefault { variable }
        | ReferenceSource::VariableRuleConditionVariable { variable, .. }
        | ReferenceSource::VariableRuleContextAttribute { variable, .. }
        | ReferenceSource::VariableRuleValue { variable, .. }
        | ReferenceSource::VariableQueryVariable { variable }
        | ReferenceSource::VariableQueryContextAttribute { variable }
        | ReferenceSource::VariableAllocation { variable } => ModelEntityRef::Variable {
            id: variable.clone(),
        },
    }
}

fn reference_target_ref(target: &ReferenceTarget) -> ModelEntityRef {
    match target {
        ReferenceTarget::ContextAttribute(name) => {
            ModelEntityRef::ContextAttribute { name: name.clone() }
        }
        ReferenceTarget::Variable(id) => ModelEntityRef::Variable { id: id.clone() },
        ReferenceTarget::Catalog(id) => ModelEntityRef::Catalog { id: id.clone() },
        ReferenceTarget::CatalogEntry { catalog, value } => ModelEntityRef::CatalogEntry {
            catalog: catalog.clone(),
            key: value.clone(),
        },
        ReferenceTarget::VariableValue { variable, value } => ModelEntityRef::Value {
            variable: variable.clone(),
            key: value.clone(),
        },
        ReferenceTarget::Allocation(id) => ModelEntityRef::Allocation { id: id.clone() },
    }
}
