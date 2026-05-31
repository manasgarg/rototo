use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;

use crate::diagnostics::{EntityId, LintDiagnostic, RototoRuleId};

use super::super::engine::{LintContext, variable_values};
use super::super::nodes::*;
use super::super::source::{DocumentKind, resolve_workspace_relative_path};
use super::super::stages::{
    push_project_diagnostic, push_reference_diagnostic, push_value_diagnostic,
};
use super::{declared_workspace_environments, field_is_integer, field_is_not_present};

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
        lint_values_shape(
            diagnostics,
            variable,
            ctx.index.external_values.get(&variable.id),
        );
        lint_environment_shapes(diagnostics, variable);
    }
}

fn lint_type_source(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => {
            if !matches!(
                type_name.value.as_str(),
                "bool" | "int" | "number" | "string" | "list"
            ) {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableUnknownType,
                    EntityId::Variable {
                        id: variable.id.clone(),
                    },
                    type_name.location.clone(),
                    format!("variable declares unknown type: {}", type_name.value),
                );
            }
        }
        TypeSourceNode::Schema(schema) => {
            let _ = &schema.value;
        }
        TypeSourceNode::Missing { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeOrSchema,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "variable must declare exactly one of type or schema",
        ),
        TypeSourceNode::Conflict { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeOrSchema,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "variable must declare exactly one of type or schema",
        ),
        TypeSourceNode::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeOrSchema,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "variable type source must be a string",
        ),
    }
}

fn lint_values_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    external_values: Option<&BTreeMap<String, ValueNode>>,
) {
    if variable.values.invalid_shape {
        if !variable.values.external_keys.is_empty() {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableExternalValuesLoadFailed,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                variable.values.location.clone(),
                "external values cannot be merged because variable values must be a table",
            );
            return;
        }

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

    if variable.values.inline_keys.is_empty() && variable.values.external_keys.is_empty() {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableValuesMissing,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            variable.values.location.clone(),
            "variable must contain [values] or external values",
        );
    }

    lint_external_value_duplicates(diagnostics, variable, external_values);
}

fn lint_external_value_duplicates(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    external_values: Option<&BTreeMap<String, ValueNode>>,
) {
    let Some(external_values) = external_values else {
        return;
    };

    for (key, value) in external_values {
        if !variable.values.inline_keys.contains(key) {
            continue;
        }

        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableExternalValueDuplicate,
            EntityId::Value {
                variable: variable.id.clone(),
                key: key.clone(),
            },
            value.location.clone(),
            format!("external value duplicates inline value: {key}"),
        );
    }
}

fn lint_environment_shapes(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    let environments = match &variable.environments {
        EnvironmentCollection::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableEnvMissingDefault,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                location.clone(),
                "variable must contain [env._]",
            );
            return;
        }
        EnvironmentCollection::Invalid { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableEnvShape,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                location.clone(),
                "env must be a table",
            );
            return;
        }
        EnvironmentCollection::Environments(environments) => environments,
    };

    if !environments.contains_key("_") {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableEnvMissingDefault,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            variable.location.clone(),
            "variable must contain [env._]",
        );
    }

    for block in environments.values() {
        lint_environment_block_shape(diagnostics, variable, block);
    }
}

fn lint_environment_block_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
) {
    let entity = EntityId::EnvironmentBlock {
        variable: variable.id.clone(),
        environment: block.environment.clone(),
    };
    if field_is_not_present(&block.value) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableEnvShape,
            entity,
            block.value.location(),
            "environment block must reference a value",
        );
    }

    match &block.rules {
        RuleCollection::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            EntityId::EnvironmentBlock {
                variable: variable.id.clone(),
                environment: block.environment.clone(),
            },
            location.clone(),
            "rule must use [[env.<id>.rule]] tables or inline rule tables",
        ),
        RuleCollection::Rules(rules) => {
            for rule in rules {
                lint_variable_rule_shape(diagnostics, variable, block, rule);
            }
        }
    }
}

fn lint_variable_rule_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    rule: &VariableRuleNode,
) {
    let entity = EntityId::Rule {
        variable: variable.id.clone(),
        environment: block.environment.clone(),
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
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let declared_environments = declared_workspace_environments(ctx);
    let diagnostics = &mut ctx.diagnostics;

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };

        for block in environments.values() {
            lint_environment_reference(
                diagnostics,
                variable,
                block,
                declared_environments.as_ref(),
            );
            lint_environment_value_reference(diagnostics, variable, block);
            lint_rule_references(diagnostics, variable, block, &known_qualifiers);
        }
    }
}

fn lint_environment_reference(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    declared_environments: Option<&BTreeSet<String>>,
) {
    let Some(declared_environments) = declared_environments else {
        return;
    };

    if block.environment == "_" || declared_environments.contains(&block.environment) {
        return;
    }

    push_reference_diagnostic(
        diagnostics,
        RototoRuleId::VariableUnknownEnvironment,
        EntityId::EnvironmentBlock {
            variable: variable.id.clone(),
            environment: block.environment.clone(),
        },
        block.value.location(),
        format!(
            "variable references undeclared environment: {}",
            block.environment
        ),
    );
}

fn lint_environment_value_reference(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
) {
    let ProjectField::Present(value) = &block.value else {
        return;
    };

    if !variable_has_values(variable) || variable_has_value(variable, &value.value) {
        return;
    }

    push_reference_diagnostic(
        diagnostics,
        RototoRuleId::VariableUnknownValue,
        EntityId::EnvironmentBlock {
            variable: variable.id.clone(),
            environment: block.environment.clone(),
        },
        value.location.clone(),
        format!("environment references unknown value: {}", value.value),
    );
}

fn lint_rule_references(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    known_qualifiers: &BTreeSet<String>,
) {
    let RuleCollection::Rules(rules) = &block.rules else {
        return;
    };

    for rule in rules {
        if rule.invalid_shape {
            continue;
        }

        let entity = EntityId::Rule {
            variable: variable.id.clone(),
            environment: block.environment.clone(),
            index: rule.index,
        };

        if let ProjectField::Present(qualifier) = &rule.qualifier
            && !known_qualifiers.contains(&qualifier.value)
        {
            push_reference_diagnostic(
                diagnostics,
                RototoRuleId::VariableRuleUnknownQualifier,
                entity.clone(),
                qualifier.location.clone(),
                format!("rule references unknown qualifier: {}", qualifier.value),
            );
        }

        if let ProjectField::Present(value) = &rule.value
            && variable_has_values(variable)
            && !variable_has_value(variable, &value.value)
        {
            push_reference_diagnostic(
                diagnostics,
                RototoRuleId::VariableUnknownValue,
                entity,
                value.location.clone(),
                format!("rule references unknown value: {}", value.value),
            );
        }
    }
}

fn variable_has_values(variable: &VariableNode) -> bool {
    !variable.values.inline_keys.is_empty() || !variable.values.external_keys.is_empty()
}

fn variable_has_value(variable: &VariableNode, value: &str) -> bool {
    variable.values.inline_keys.contains(value) || variable.values.external_keys.contains(value)
}

pub(super) fn lint_variable_values(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for variable in ctx.index.variables.values() {
        match &variable.type_source {
            TypeSourceNode::Primitive(type_name) => {
                let Some(primitive) = PrimitiveType::from_str(&type_name.value) else {
                    continue;
                };
                lint_primitive_variable_values(&mut diagnostics, ctx, variable, primitive);
            }
            TypeSourceNode::Schema(schema_ref) => {
                lint_schema_backed_variable_values(&mut diagnostics, ctx, variable, schema_ref);
            }
            TypeSourceNode::Missing { .. }
            | TypeSourceNode::Conflict { .. }
            | TypeSourceNode::Invalid { .. } => {}
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_primitive_variable_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    variable: &VariableNode,
    primitive: PrimitiveType,
) {
    for value in variable_values(ctx, variable) {
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

fn lint_schema_backed_variable_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    variable: &VariableNode,
    schema_ref: &Spanned<String>,
) {
    let Some(schema_path) =
        resolve_workspace_relative_path(&variable.location.path, &schema_ref.value)
    else {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableSchemaRef,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            schema_ref.location.clone(),
            format!(
                "variable schema reference is invalid: {} is not a relative path inside the workspace",
                schema_ref.value
            ),
        );
        return;
    };

    let Some(document) = ctx.source.document_by_path(&schema_path) else {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableSchemaRef,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            schema_ref.location.clone(),
            format!("variable schema reference is invalid: schema file not found: {schema_path}"),
        );
        return;
    };

    if !matches!(&document.kind, DocumentKind::Schema) {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableSchemaRef,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            schema_ref.location.clone(),
            format!(
                "variable schema reference is invalid: path is not a schema document: {schema_path}"
            ),
        );
        return;
    }

    let Some(schema) = ctx.syntax.json.get(&document.id) else {
        return;
    };

    let validator = match jsonschema::validator_for(schema) {
        Ok(validator) => validator,
        Err(_) => {
            return;
        }
    };

    for value in variable_values(ctx, variable) {
        if let Err(err) = validator.validate(&value.value) {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::VariableValueSchemaMismatch,
                EntityId::Value {
                    variable: variable.id.clone(),
                    key: value.key.clone(),
                },
                value.location.clone(),
                format!("value {} does not match schema: {err}", value.key),
            );
        }
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
