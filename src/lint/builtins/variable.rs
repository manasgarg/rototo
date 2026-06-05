use serde_json::Value as JsonValue;

use crate::diagnostics::{EntityId, LintDiagnostic, RototoRuleId};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::stages::{
    push_project_diagnostic, push_reference_diagnostic, push_value_diagnostic,
};
use super::{field_is_integer, field_is_not_present};

pub(super) fn lint_variable_shapes(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for variable in ctx.index.variables.values() {
        if !field_is_integer(&variable.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableSchemaVersion,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                variable.schema_version.location(),
                "variable must declare schema_version = 1",
            );
        }

        lint_type_source(diagnostics, variable);
        lint_values_shape(diagnostics, variable);
        lint_resolve_shape(diagnostics, variable);
    }
}

fn lint_type_source(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    match &variable.type_source {
        TypeSourceNode::Primitive(_) | TypeSourceNode::Resource(_) => {}
        TypeSourceNode::Schema(schema) => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeSource,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            schema.location.clone(),
            "variable schemas are no longer supported; declare type instead",
        ),
        TypeSourceNode::Missing { location } | TypeSourceNode::Conflict { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableTypeSource,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                location.clone(),
                "variable must declare type",
            );
        }
        TypeSourceNode::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeSource,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "variable type must be a string",
        ),
    }
}

fn lint_values_shape(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    if is_resource_backed(variable) {
        if variable.values.invalid_shape || !variable.values.inline_values.is_empty() {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableValuesDisallowed,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                variable.values.location.clone(),
                "resource-backed variables must not contain [values]",
            );
        }
        return;
    }

    if variable.values.invalid_shape {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableValuesMissing,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            variable.values.location.clone(),
            "variable values must be a table",
        );
        return;
    }

    if variable.values.inline_values.is_empty() {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableValuesMissing,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            variable.values.location.clone(),
            "primitive variable must contain [values]",
        );
    }
}

fn lint_resolve_shape(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    let (default, rules) = match &variable.resolve {
        ResolveNode::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableResolveMissingDefault,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                location.clone(),
                "variable must contain [resolve]",
            );
            return;
        }
        ResolveNode::Invalid { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableResolveShape,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                location.clone(),
                "resolve must be a table",
            );
            return;
        }
        ResolveNode::Resolve { default, rules, .. } => (default, rules),
    };

    if field_is_not_present(default) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableResolveMissingDefault,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            default.location(),
            "resolve must reference a default value",
        );
    }

    match rules {
        RuleCollection::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "rule must use [[resolve.rule]] tables",
        ),
        RuleCollection::Rules(rules) => {
            for rule in rules {
                lint_variable_rule_shape(diagnostics, variable, rule);
            }
        }
    }
}

fn lint_variable_rule_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    rule: &VariableRuleNode,
) {
    let entity = EntityId::Rule {
        variable: variable.id.clone(),
        index: rule.index,
    };

    if rule.invalid_shape {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            entity,
            rule.location.clone(),
            "rule must be a table",
        );
        return;
    }

    if field_is_not_present(&rule.qualifier) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            entity.clone(),
            rule.qualifier.location(),
            "rule must reference a qualifier",
        );
    }
    if field_is_not_present(&rule.value) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            entity,
            rule.value.location(),
            "rule must reference a value",
        );
    }
}

pub(super) fn lint_variable_references(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for edge in ctx.references.edges() {
        if edge.is_resolved() {
            continue;
        }
        match (&edge.source, &edge.target) {
            (
                ReferenceSource::VariableResource { variable: _ },
                ReferenceTarget::Resource(resource),
            ) => push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableUnknownResource,
                edge.entity.clone(),
                edge.location.clone(),
                format!("variable references unknown resource: {resource}"),
            ),
            (
                ReferenceSource::VariableResolveDefault { variable },
                ReferenceTarget::VariableValue { variable: _, value },
            ) => {
                let Some(variable_node) = ctx.index.variables.get(variable) else {
                    continue;
                };
                if !variable_has_values(variable_node) {
                    continue;
                }
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.entity.clone(),
                    edge.location.clone(),
                    format!("resolve default references unknown value: {value}"),
                );
            }
            (
                ReferenceSource::VariableResolveDefault { variable },
                ReferenceTarget::ResourceObject { resource, value },
            ) => {
                if !ctx.index.resources.contains_key(resource)
                    || variable_resource_id(ctx, variable).is_none_or(|id| id != resource)
                {
                    continue;
                }
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.entity.clone(),
                    edge.location.clone(),
                    format!("resolve default references unknown resource object: {value}"),
                );
            }
            (
                ReferenceSource::VariableRuleQualifier {
                    variable: _,
                    rule: _,
                },
                ReferenceTarget::Qualifier(qualifier),
            ) => push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableRuleUnknownQualifier,
                edge.entity.clone(),
                edge.location.clone(),
                format!("rule references unknown qualifier: {qualifier}"),
            ),
            (
                ReferenceSource::VariableRuleValue { variable, rule: _ },
                ReferenceTarget::VariableValue { variable: _, value },
            ) => {
                let Some(variable_node) = ctx.index.variables.get(variable) else {
                    continue;
                };
                if !variable_has_values(variable_node) {
                    continue;
                }
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.entity.clone(),
                    edge.location.clone(),
                    format!("rule references unknown value: {value}"),
                );
            }
            (
                ReferenceSource::VariableRuleValue { variable, rule: _ },
                ReferenceTarget::ResourceObject { resource, value },
            ) => {
                if !ctx.index.resources.contains_key(resource)
                    || variable_resource_id(ctx, variable).is_none_or(|id| id != resource)
                {
                    continue;
                }
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.entity.clone(),
                    edge.location.clone(),
                    format!("rule references unknown resource object: {value}"),
                );
            }
            _ => {}
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn variable_has_values(variable: &VariableNode) -> bool {
    !variable.values.inline_values.is_empty()
}

pub(super) fn lint_variable_values(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for variable in ctx.index.variables.values() {
        match &variable.type_source {
            TypeSourceNode::Primitive(type_name) => {
                let Some(primitive) = lint_primitive_type(&mut diagnostics, variable, type_name)
                else {
                    continue;
                };
                lint_primitive_variable_values(&mut diagnostics, variable, primitive);
            }
            TypeSourceNode::Resource(_)
            | TypeSourceNode::Schema(_)
            | TypeSourceNode::Missing { .. }
            | TypeSourceNode::Conflict { .. }
            | TypeSourceNode::Invalid { .. } => {}
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_primitive_type(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    type_name: &Spanned<String>,
) -> Option<PrimitiveType> {
    let primitive = PrimitiveType::from_str(&type_name.value);
    if primitive.is_none() {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableUnknownType,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            type_name.location.clone(),
            format!("variable declares unknown type: {}", type_name.value),
        );
    }
    primitive
}

fn lint_primitive_variable_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    primitive: PrimitiveType,
) {
    for value in variable.values.inline_values.values() {
        if primitive.matches(&value.value) {
            continue;
        }

        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableValueTypeMismatch,
            EntityId::Value {
                variable: variable.id.clone(),
                key: value.key.clone(),
            },
            value.location.clone(),
            format!(
                "value {} does not match type {}",
                value.key,
                primitive.as_str()
            ),
        );
    }
}

fn is_resource_backed(variable: &VariableNode) -> bool {
    matches!(variable.type_source, TypeSourceNode::Resource(_))
}

fn variable_resource_id<'a>(ctx: &'a LintContext, variable: &str) -> Option<&'a str> {
    let variable = ctx.index.variables.get(variable)?;
    match &variable.type_source {
        TypeSourceNode::Resource(resource) => Some(resource.value.as_str()),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum PrimitiveType {
    Bool,
    Int,
    Number,
    String,
    List,
}

impl PrimitiveType {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "number" => Some(Self::Number),
            "string" => Some(Self::String),
            "list" => Some(Self::List),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Number => "number",
            Self::String => "string",
            Self::List => "list",
        }
    }

    fn matches(self, value: &JsonValue) -> bool {
        match self {
            Self::Bool => value.is_boolean(),
            Self::Int => value.as_i64().is_some() || value.as_u64().is_some(),
            Self::Number => value.is_number(),
            Self::String => value.is_string(),
            Self::List => value.is_array(),
        }
    }
}
