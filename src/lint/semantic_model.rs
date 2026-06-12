//! A serializable projection of the semantic and reference indexes.
//!
//! Tools (the admin app, editors) consume this model instead of parsing
//! workspace files themselves, so rototo's parse stays the single semantic
//! authority. Locations carry source ranges so writers can splice edits at
//! positions reported by the same parse that produced the rendering.

use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, SourceRange};

use super::WorkspaceLintSnapshot;
use super::index::{
    PredicateCollection, ProjectField, ResolveNode, RuleCollection, TypeSourceNode,
};
use super::references::{ReferenceSource, ReferenceTarget};

pub const SEMANTIC_MODEL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSemanticModel {
    pub version: u32,
    pub qualifiers: Vec<QualifierModel>,
    pub variables: Vec<VariableModel>,
    pub resources: Vec<ResourceModel>,
    pub resource_objects: Vec<ResourceObjectModel>,
    pub schemas: Vec<SchemaModel>,
    pub linters: Vec<LinterModel>,
    pub references: Vec<ReferenceModel>,
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
pub struct QualifierModel {
    pub id: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub predicates: Vec<PredicateModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateModel {
    pub index: usize,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<JsonValue>,
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
    /// "primitive", "resource", "schema", "missing", "conflict", or "invalid".
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
    pub default: Option<ModelField>,
    pub rules: Vec<RuleModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleModel {
    pub index: usize,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<ModelField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<ModelField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceModel {
    pub id: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<ModelField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceObjectModel {
    pub resource: String,
    pub key: String,
    pub location: ModelLocation,
    pub value: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaModel {
    pub path: String,
    pub location: ModelLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<JsonValue>,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ModelEntityRef {
    Qualifier { id: String },
    Variable { id: String },
    Resource { id: String },
    ResourceObject { resource: String, key: String },
    Schema { path: String },
    Value { variable: String, key: String },
    ContextAttribute { name: String },
}

impl WorkspaceLintSnapshot {
    pub(crate) fn semantic_model(&self) -> WorkspaceSemanticModel {
        let index = &self.index;
        let qualifiers = index
            .qualifiers
            .values()
            .map(|node| QualifierModel {
                id: node.id.clone(),
                location: model_location(&node.location),
                description: present_string(&node.description),
                predicates: match &node.predicates {
                    PredicateCollection::Predicates(predicates) => predicates
                        .iter()
                        .map(|predicate| PredicateModel {
                            index: predicate.index,
                            location: model_location(&predicate.location),
                            attribute: Some(model_field(&predicate.attribute)),
                            op: Some(ModelField {
                                value: match &predicate.op {
                                    ProjectField::Present(op) => Some(op.value.as_str().to_owned()),
                                    _ => None,
                                },
                                location: model_location(&predicate.op.location()),
                            }),
                            value: predicate.value.as_ref().map(|value| value.value.clone()),
                        })
                        .collect(),
                    PredicateCollection::Missing { .. } | PredicateCollection::Invalid { .. } => {
                        Vec::new()
                    }
                },
            })
            .collect();

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
                    TypeSourceNode::Resource(value) => DeclarationModel {
                        kind: "resource".to_owned(),
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
                        default,
                        rules,
                    } => Some(ResolveModel {
                        location: model_location(location),
                        default: Some(model_field(default)),
                        rules: match rules {
                            RuleCollection::Rules(rules) => rules
                                .iter()
                                .map(|rule| RuleModel {
                                    index: rule.index,
                                    location: model_location(&rule.location),
                                    qualifier: Some(model_field(&rule.qualifier)),
                                    value: Some(model_field(&rule.value)),
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

        let resources = index
            .resources
            .values()
            .map(|node| ResourceModel {
                id: node.id.clone(),
                location: model_location(&node.location),
                description: present_string(&node.description),
                schema: Some(model_field(&node.schema)),
            })
            .collect();

        let resource_objects = index
            .resource_objects
            .values()
            .flat_map(|objects| objects.values())
            .map(|node| ResourceObjectModel {
                resource: node.resource_id.clone(),
                key: node.key.clone(),
                location: model_location(&node.location),
                value: node.value.clone(),
            })
            .collect();

        let schemas = index
            .schemas
            .values()
            .map(|node| SchemaModel {
                path: node.path.clone(),
                location: model_location(&node.location),
                json: node.json.clone(),
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
            })
            .collect();

        WorkspaceSemanticModel {
            version: SEMANTIC_MODEL_VERSION,
            qualifiers,
            variables,
            resources,
            resource_objects,
            schemas,
            linters,
            references,
        }
    }
}

fn model_location(location: &DiagnosticLocation) -> ModelLocation {
    ModelLocation {
        path: location.path.clone(),
        range: location.range,
    }
}

fn model_field(field: &ProjectField<String>) -> ModelField {
    ModelField {
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

fn reference_source_ref(source: &ReferenceSource) -> ModelEntityRef {
    match source {
        ReferenceSource::QualifierPredicateQualifier { qualifier, .. }
        | ReferenceSource::QualifierPredicateContextAttribute { qualifier, .. } => {
            ModelEntityRef::Qualifier {
                id: qualifier.clone(),
            }
        }
        ReferenceSource::VariableResource { variable }
        | ReferenceSource::VariableResolveDefault { variable }
        | ReferenceSource::VariableRuleQualifier { variable, .. }
        | ReferenceSource::VariableRuleValue { variable, .. } => ModelEntityRef::Variable {
            id: variable.clone(),
        },
        ReferenceSource::ResourceSchema { resource } => ModelEntityRef::Resource {
            id: resource.clone(),
        },
    }
}

fn reference_target_ref(target: &ReferenceTarget) -> ModelEntityRef {
    match target {
        ReferenceTarget::ContextAttribute(name) => {
            ModelEntityRef::ContextAttribute { name: name.clone() }
        }
        ReferenceTarget::Qualifier(id) => ModelEntityRef::Qualifier { id: id.clone() },
        ReferenceTarget::Resource(id) => ModelEntityRef::Resource { id: id.clone() },
        ReferenceTarget::ResourceObject { resource, value } => ModelEntityRef::ResourceObject {
            resource: resource.clone(),
            key: value.clone(),
        },
        ReferenceTarget::Schema(path) => ModelEntityRef::Schema { path: path.clone() },
        ReferenceTarget::VariableValue { variable, value } => ModelEntityRef::Value {
            variable: variable.clone(),
            key: value.clone(),
        },
    }
}
