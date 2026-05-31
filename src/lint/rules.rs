use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, EntityId, LintDiagnostic, LintStage,
    RelatedLocation, RototoRuleId, Severity,
};
use crate::workspace::workspace_environments;

use super::engine::{
    LintContext, push_graph_diagnostic, push_project_diagnostic, push_reference_diagnostic,
    push_value_diagnostic, resolve_workspace_relative_path, resolve_workspace_root_path,
    variable_values,
};
use super::nodes::*;
use super::source::DocumentKind;

pub(super) fn run_project(ctx: &mut LintContext) {
    lint_manifest_shape(ctx);
    lint_manifest_custom_rule_shapes(ctx);
    lint_qualifier_shapes(ctx);
    lint_variable_shapes(ctx);
    lint_custom_rule_conflicts(ctx);
}

pub(super) fn run_reference(ctx: &mut LintContext) {
    lint_context_schema_reference(ctx);
    lint_qualifier_context_schema_attributes(ctx);
    lint_qualifier_references(ctx);
    lint_variable_references(ctx);
}

pub(super) fn run_value(ctx: &mut LintContext) {
    lint_schema_documents(ctx);
    lint_variable_values(ctx);
}

pub(super) fn run_graph(ctx: &mut LintContext) {
    lint_qualifier_cycles(ctx);
    lint_unreferenced_qualifiers(ctx);
    lint_shadowed_variable_rules(ctx);
    lint_unused_variable_values(ctx);
}

fn lint_manifest_shape(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };
    let Some(parsed) = ctx.syntax.toml.get(&manifest.doc) else {
        return;
    };

    if let Err(err) = workspace_environments(&parsed.plain) {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::WorkspaceManifestSchemaFailed,
            LintStage::Project,
            EntityId::Manifest,
            manifest.location.clone(),
            err.to_string(),
        ));
    }
}

fn lint_qualifier_shapes(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for qualifier in ctx.index.qualifiers.values() {
        if !field_is_integer(&qualifier.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierSchemaVersion,
                EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                qualifier.schema_version.location(),
                "qualifier must declare schema_version = 1",
            );
        }

        match &qualifier.predicates {
            PredicateCollection::Missing { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateMissing,
                EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                location.clone(),
                "qualifier must contain at least one [[predicate]]",
            ),
            PredicateCollection::Invalid { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateShape,
                EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                location.clone(),
                "predicate must use [[predicate]] tables",
            ),
            PredicateCollection::Predicates(predicates) => {
                for predicate in predicates {
                    lint_predicate_shape(diagnostics, qualifier, predicate);
                }
            }
        }
    }
}

fn lint_predicate_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    qualifier: &QualifierNode,
    predicate: &PredicateNode,
) {
    let entity = EntityId::Predicate {
        qualifier: qualifier.id.clone(),
        index: predicate.index,
    };
    if field_is_not_present(&predicate.attribute) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateShape,
            entity.clone(),
            predicate.attribute.location(),
            "predicate must contain attribute",
        );
        return;
    }

    let op = match &predicate.op {
        ProjectField::Present(op) => &op.value,
        ProjectField::Invalid { location } | ProjectField::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateShape,
                entity,
                location.clone(),
                "predicate must contain op",
            );
            return;
        }
    };

    if let PredicateOp::Unknown(op) = op {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateUnknownOp,
            entity.clone(),
            predicate.op.location(),
            format!("predicate has unknown op: {op}"),
        );
    }

    if matches!(op, PredicateOp::Bucket) {
        lint_bucket_predicate(diagnostics, predicate, entity);
    } else {
        lint_comparison_predicate(diagnostics, predicate, op, entity);
    }
}

fn lint_bucket_predicate(
    diagnostics: &mut Vec<LintDiagnostic>,
    predicate: &PredicateNode,
    entity: EntityId,
) {
    if predicate.salt.as_ref().is_none_or(field_is_not_present) {
        let location = predicate
            .salt
            .as_ref()
            .map(ProjectField::location)
            .unwrap_or_else(|| predicate.location.clone());
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            location,
            "bucket predicate must contain salt",
        );
    }

    let Some(range) = &predicate.range else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            predicate.location.clone(),
            "bucket predicate must contain range",
        );
        return;
    };

    if !range.is_array {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            range.location.clone(),
            "bucket range must be a list",
        );
    } else if range.len != 2 {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            range.location.clone(),
            "bucket range must contain two integers",
        );
    } else {
        match (range.start, range.end) {
            (Some(start), Some(end)) if 0 <= start && start < end && end <= 10_000 => {}
            _ => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateBucket,
                entity.clone(),
                range.location.clone(),
                "bucket range must satisfy 0 <= start < end <= 10000",
            ),
        }
    }

    if predicate.has_bucket_value {
        let location = predicate
            .value
            .as_ref()
            .map(|value| value.location.clone())
            .unwrap_or_else(|| predicate.location.clone());
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity,
            location,
            "bucket predicate must not contain value",
        );
    }
}

fn lint_comparison_predicate(
    diagnostics: &mut Vec<LintDiagnostic>,
    predicate: &PredicateNode,
    op: &PredicateOp,
    entity: EntityId,
) {
    let Some(value) = &predicate.value else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateValue,
            entity,
            predicate.location.clone(),
            "predicate must contain value",
        );
        return;
    };

    match op {
        PredicateOp::In | PredicateOp::NotIn if value.shape != ValueShape::Array => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateValue,
                entity,
                value.location.clone(),
                format!("{} predicate value must be a list", predicate_op_label(op)),
            );
        }
        PredicateOp::Gt | PredicateOp::Gte | PredicateOp::Lt | PredicateOp::Lte
            if !matches!(value.shape, ValueShape::Integer | ValueShape::Float) =>
        {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateValue,
                entity,
                value.location.clone(),
                format!(
                    "{} predicate value must be a number",
                    predicate_op_label(op)
                ),
            );
        }
        _ => {}
    }
}

fn lint_manifest_custom_rule_shapes(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };

    match &manifest.custom_rules {
        CustomRuleCollection::Invalid { location } => push_project_diagnostic(
            &mut ctx.diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            location.clone(),
            "workspace lint rule declarations must use [[lint.rule]] tables",
        ),
        CustomRuleCollection::Rules(rules) => {
            for rule in rules {
                lint_workspace_custom_rule_declaration_shape(&mut ctx.diagnostics, rule);
            }
        }
    }
}

fn lint_workspace_custom_rule_declaration_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: &CustomRuleDeclarationNode,
) {
    if field_is_not_present(&rule.id) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.id.location(),
            "custom lint rule must contain id",
        );
    }
    if field_is_not_present(&rule.title) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.title.location(),
            "custom lint rule must contain title",
        );
    }
    if field_is_not_present(&rule.help) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.help.location(),
            "custom lint rule must contain help",
        );
    }
    if let Some(ProjectField::Invalid { location }) = &rule.severity {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            location.clone(),
            "custom lint rule severity must be error or warning",
        );
    }

    if let ProjectField::Present(id) = &rule.id
        && let Err(err) = CustomRuleId::parse(&id.value)
    {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintInvalidRule,
            EntityId::Manifest,
            id.location.clone(),
            format!("custom lint rule id is invalid: {err}"),
        );
    }
}

fn lint_variable_shapes(ctx: &mut LintContext) {
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

fn lint_custom_rule_conflicts(ctx: &mut LintContext) {
    let mut declared: BTreeMap<CustomRuleId, CustomRuleDefinition> = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for (definition, location, entity) in custom_rule_definition_entries(ctx) {
        match declared.get(&definition.rule) {
            Some(existing) if !existing.same_metadata(&definition) => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::CustomLintRuleConflict,
                    entity,
                    location,
                    format!("custom lint rule metadata conflicts: {}", definition.rule),
                );
            }
            Some(_) => {}
            None => {
                declared.insert(definition.rule.clone(), definition);
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn custom_rule_definition_entries(
    ctx: &LintContext,
) -> Vec<(CustomRuleDefinition, DiagnosticLocation, EntityId)> {
    let mut definitions = Vec::new();

    if let Some(manifest) = &ctx.index.manifest {
        definitions.extend(
            custom_rule_definitions_from_collection(&manifest.custom_rules)
                .into_iter()
                .map(|(definition, location)| (definition, location, EntityId::Manifest)),
        );
    }

    definitions
}

pub(super) fn workspace_custom_rule_definitions(
    ctx: &LintContext,
) -> BTreeMap<CustomRuleId, CustomRuleDefinition> {
    let Some(manifest) = &ctx.index.manifest else {
        return BTreeMap::new();
    };
    custom_rule_definitions_from_collection(&manifest.custom_rules)
        .into_iter()
        .map(|(definition, _)| (definition.rule.clone(), definition))
        .collect()
}

pub(super) fn custom_rule_definitions_from_collection(
    rules: &CustomRuleCollection,
) -> Vec<(CustomRuleDefinition, DiagnosticLocation)> {
    let CustomRuleCollection::Rules(rules) = rules else {
        return Vec::new();
    };
    custom_rule_definitions_from_rules(rules)
}

fn custom_rule_definitions_from_rules(
    rules: &[CustomRuleDeclarationNode],
) -> Vec<(CustomRuleDefinition, DiagnosticLocation)> {
    rules
        .iter()
        .filter_map(|rule| {
            let (
                ProjectField::Present(id),
                ProjectField::Present(title),
                ProjectField::Present(help),
            ) = (&rule.id, &rule.title, &rule.help)
            else {
                return None;
            };
            let Ok(rule_id) = CustomRuleId::parse(&id.value) else {
                return None;
            };
            let severity = match &rule.severity {
                Some(ProjectField::Present(severity)) => severity.value,
                Some(ProjectField::Invalid { .. }) => return None,
                Some(ProjectField::Missing { .. }) | None => Severity::Error,
            };
            Some((
                CustomRuleDefinition::with_severity(
                    rule_id,
                    severity,
                    title.value.clone(),
                    help.value.clone(),
                ),
                rule.location.clone(),
            ))
        })
        .collect()
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

struct ContextSchemaError {
    location: DiagnosticLocation,
    message: String,
}

fn lint_context_schema_reference(ctx: &mut LintContext) {
    let Err(err) = valid_context_schema(ctx) else {
        return;
    };

    push_reference_diagnostic(
        &mut ctx.diagnostics,
        RototoRuleId::WorkspaceContextSchemaRef,
        EntityId::Manifest,
        err.location,
        err.message,
    );
}

fn lint_qualifier_context_schema_attributes(ctx: &mut LintContext) {
    let Ok(Some(schema)) = valid_context_schema(ctx) else {
        return;
    };

    let mut diagnostics = Vec::new();
    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if qualifier_reference(&attribute.value).is_some()
                || context_schema_declares_path(schema, &attribute.value)
            {
                continue;
            }

            push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::WorkspaceContextSchemaAttribute,
                EntityId::Predicate {
                    qualifier: qualifier.id.clone(),
                    index: predicate.index,
                },
                attribute.location.clone(),
                format!(
                    "context attribute is not declared by the context schema: {}",
                    attribute.value
                ),
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn valid_context_schema(
    ctx: &LintContext,
) -> std::result::Result<Option<&JsonValue>, ContextSchemaError> {
    let Some(manifest) = &ctx.index.manifest else {
        return Ok(None);
    };
    let Some(context) = &manifest.context_schema else {
        return Ok(None);
    };

    if context.invalid_shape {
        return Err(ContextSchemaError {
            location: context.location.clone(),
            message: "[context] must be a table".to_owned(),
        });
    }

    let ProjectField::Present(schema_ref) = &context.schema else {
        return Err(ContextSchemaError {
            location: context.schema.location(),
            message: "[context] must declare schema".to_owned(),
        });
    };

    let schema_path =
        resolve_workspace_root_path(&schema_ref.value).ok_or_else(|| ContextSchemaError {
            location: schema_ref.location.clone(),
            message: "context schema path must be a relative path inside the workspace".to_owned(),
        })?;
    let schema_document =
        ctx.source
            .document_by_path(&schema_path)
            .ok_or_else(|| ContextSchemaError {
                location: schema_ref.location.clone(),
                message: format!("context schema file not found: {schema_path}"),
            })?;
    if !matches!(&schema_document.kind, DocumentKind::Schema) {
        return Err(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema path is not a schema document: {schema_path}"),
        });
    }

    let schema = ctx
        .syntax
        .json
        .get(&schema_document.id)
        .ok_or_else(|| ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema file could not be parsed: {schema_path}"),
        })?;
    jsonschema::validator_for(schema).map_err(|err| ContextSchemaError {
        location: schema_ref.location.clone(),
        message: format!("context schema is invalid: {err}"),
    })?;

    Ok(Some(schema))
}

fn context_schema_declares_path(schema: &JsonValue, attribute: &str) -> bool {
    if attribute.is_empty() {
        return false;
    }

    let mut current = schema;
    for segment in attribute.split('.') {
        let Some(properties) = current.get("properties").and_then(JsonValue::as_object) else {
            return false;
        };
        let Some(next) = properties.get(segment) else {
            return false;
        };
        current = next;
    }
    true
}

fn lint_qualifier_references(ctx: &mut LintContext) {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let diagnostics = &mut ctx.diagnostics;

    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            let Some(referenced_qualifier) = qualifier_reference(&attribute.value) else {
                continue;
            };

            if known_qualifiers.contains(referenced_qualifier) {
                continue;
            }

            push_reference_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateUnknownQualifier,
                EntityId::Predicate {
                    qualifier: qualifier.id.clone(),
                    index: predicate.index,
                },
                attribute.location.clone(),
                format!(
                    "predicate references unknown qualifier: {}",
                    reference_label(referenced_qualifier)
                ),
            );
        }
    }
}

fn lint_variable_references(ctx: &mut LintContext) {
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

pub(super) fn declared_workspace_environments(ctx: &LintContext) -> Option<BTreeSet<String>> {
    let manifest = ctx.index.manifest.as_ref()?;
    let parsed = ctx.syntax.toml.get(&manifest.doc)?;
    workspace_environments(&parsed.plain)
        .ok()
        .map(|environments| environments.into_iter().collect())
}

pub(super) fn qualifier_reference(attribute: &str) -> Option<&str> {
    attribute.strip_prefix("qualifier.")
}

fn reference_label(reference: &str) -> &str {
    if reference.is_empty() {
        "<empty>"
    } else {
        reference
    }
}

fn variable_has_values(variable: &VariableNode) -> bool {
    !variable.values.inline_keys.is_empty() || !variable.values.external_keys.is_empty()
}

fn variable_has_value(variable: &VariableNode, value: &str) -> bool {
    variable.values.inline_keys.contains(value) || variable.values.external_keys.contains(value)
}

#[derive(Clone)]
struct QualifierReferenceEdge {
    from: String,
    to: String,
    location: DiagnosticLocation,
}

fn lint_qualifier_cycles(ctx: &mut LintContext) {
    let graph = qualifier_reference_graph(ctx);
    let components = strongly_connected_qualifiers(&graph);
    let mut diagnostics = Vec::new();

    for component in components {
        let component_set: BTreeSet<_> = component.iter().cloned().collect();
        let cycle_edges = component
            .iter()
            .flat_map(|qualifier_id| graph.get(qualifier_id).into_iter().flatten())
            .filter(|edge| component_set.contains(&edge.to))
            .cloned()
            .collect::<Vec<_>>();
        let is_cycle = component.len() > 1
            || cycle_edges
                .iter()
                .any(|edge| edge.from == edge.to && component_set.contains(&edge.from));
        if !is_cycle {
            continue;
        }

        for qualifier_id in &component {
            let Some(qualifier) = ctx.index.qualifiers.get(qualifier_id) else {
                continue;
            };
            let primary_edge = cycle_edges.iter().find(|edge| edge.from == *qualifier_id);
            let primary = primary_edge
                .map(|edge| edge.location.clone())
                .unwrap_or_else(|| qualifier.location.clone());
            let mut diagnostic = LintDiagnostic::rototo(
                RototoRuleId::QualifierCycle,
                LintStage::Graph,
                EntityId::Qualifier {
                    id: qualifier_id.clone(),
                },
                primary.clone(),
                qualifier_cycle_message(qualifier_id, &component),
            );
            diagnostic.related = cycle_edges
                .iter()
                .filter(|edge| edge.from != *qualifier_id || edge.location != primary)
                .map(|edge| RelatedLocation {
                    location: edge.location.clone(),
                    message: format!("cycle reference: {} -> {}", edge.from, edge.to),
                })
                .collect();
            diagnostics.push(diagnostic);
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_cycle_message(qualifier_id: &str, component: &[String]) -> String {
    if component.len() == 1 {
        format!("qualifier references itself: {qualifier_id}")
    } else {
        format!(
            "qualifier participates in a reference cycle: {}",
            component.join(" -> ")
        )
    }
}

fn lint_unreferenced_qualifiers(ctx: &mut LintContext) {
    let referenced = referenced_qualifier_ids(ctx);
    let mut diagnostics = Vec::new();

    for qualifier in ctx.index.qualifiers.values() {
        if referenced.contains(&qualifier.id) {
            continue;
        }

        push_graph_diagnostic(
            &mut diagnostics,
            RototoRuleId::QualifierUnreferenced,
            EntityId::Qualifier {
                id: qualifier.id.clone(),
            },
            qualifier.location.clone(),
            format!("qualifier is not referenced: {}", qualifier.id),
        );
    }

    ctx.diagnostics.extend(diagnostics);
}

fn lint_shadowed_variable_rules(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };

        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            let mut seen_qualifiers: BTreeMap<String, DiagnosticLocation> = BTreeMap::new();

            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                let ProjectField::Present(qualifier) = &rule.qualifier else {
                    continue;
                };

                if let Some(first_location) = seen_qualifiers.get(&qualifier.value) {
                    let mut diagnostic = LintDiagnostic::rototo(
                        RototoRuleId::VariableRuleShadowed,
                        LintStage::Graph,
                        EntityId::Rule {
                            variable: variable.id.clone(),
                            environment: block.environment.clone(),
                            index: rule.index,
                        },
                        qualifier.location.clone(),
                        format!(
                            "rule is shadowed by an earlier rule with qualifier: {}",
                            qualifier.value
                        ),
                    );
                    diagnostic.related.push(RelatedLocation {
                        location: first_location.clone(),
                        message: format!("first rule using qualifier: {}", qualifier.value),
                    });
                    diagnostics.push(diagnostic);
                } else {
                    seen_qualifiers.insert(qualifier.value.clone(), qualifier.location.clone());
                }
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn lint_unused_variable_values(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let referenced = referenced_variable_value_keys(variable);
        for value in variable_values(ctx, variable) {
            if referenced.contains(&value.key) {
                continue;
            }

            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableValueUnused,
                EntityId::Value {
                    variable: variable.id.clone(),
                    key: value.key.clone(),
                },
                value.location.clone(),
                format!("variable value is not referenced: {}", value.key),
            );
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_reference_graph(ctx: &LintContext) -> BTreeMap<String, Vec<QualifierReferenceEdge>> {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let mut graph = known_qualifiers
        .iter()
        .map(|qualifier_id| (qualifier_id.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();

    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            let Some(referenced_qualifier) = qualifier_reference(&attribute.value) else {
                continue;
            };
            if !known_qualifiers.contains(referenced_qualifier) {
                continue;
            }

            graph
                .entry(qualifier.id.clone())
                .or_default()
                .push(QualifierReferenceEdge {
                    from: qualifier.id.clone(),
                    to: referenced_qualifier.to_owned(),
                    location: attribute.location.clone(),
                });
        }
    }

    graph
}

#[derive(Default)]
struct TarjanState {
    next_index: usize,
    stack: Vec<String>,
    indices: BTreeMap<String, usize>,
    lowlinks: BTreeMap<String, usize>,
    on_stack: BTreeSet<String>,
    components: Vec<Vec<String>>,
}

fn strongly_connected_qualifiers(
    graph: &BTreeMap<String, Vec<QualifierReferenceEdge>>,
) -> Vec<Vec<String>> {
    let mut state = TarjanState::default();

    for qualifier_id in graph.keys() {
        if !state.indices.contains_key(qualifier_id) {
            strong_connect_qualifier(qualifier_id, graph, &mut state);
        }
    }

    state.components
}

fn strong_connect_qualifier(
    qualifier_id: &str,
    graph: &BTreeMap<String, Vec<QualifierReferenceEdge>>,
    state: &mut TarjanState,
) {
    state
        .indices
        .insert(qualifier_id.to_owned(), state.next_index);
    state
        .lowlinks
        .insert(qualifier_id.to_owned(), state.next_index);
    state.next_index += 1;
    state.stack.push(qualifier_id.to_owned());
    state.on_stack.insert(qualifier_id.to_owned());

    if let Some(edges) = graph.get(qualifier_id) {
        for edge in edges {
            if !state.indices.contains_key(&edge.to) {
                strong_connect_qualifier(&edge.to, graph, state);
                let target_lowlink = state.lowlinks[&edge.to];
                let lowlink = state.lowlinks.get_mut(qualifier_id).unwrap();
                *lowlink = (*lowlink).min(target_lowlink);
            } else if state.on_stack.contains(&edge.to) {
                let target_index = state.indices[&edge.to];
                let lowlink = state.lowlinks.get_mut(qualifier_id).unwrap();
                *lowlink = (*lowlink).min(target_index);
            }
        }
    }

    if state.lowlinks[qualifier_id] != state.indices[qualifier_id] {
        return;
    }

    let mut component = Vec::new();
    while let Some(member) = state.stack.pop() {
        state.on_stack.remove(&member);
        let is_root = member == qualifier_id;
        component.push(member);
        if is_root {
            break;
        }
    }
    component.sort();
    state.components.push(component);
}

fn referenced_qualifier_ids(ctx: &LintContext) -> BTreeSet<String> {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let mut referenced = BTreeSet::new();

    for edges in qualifier_reference_graph(ctx).values() {
        for edge in edges {
            if edge.from != edge.to {
                referenced.insert(edge.to.clone());
            }
        }
    }

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };
        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                let ProjectField::Present(qualifier) = &rule.qualifier else {
                    continue;
                };
                if known_qualifiers.contains(&qualifier.value) {
                    referenced.insert(qualifier.value.clone());
                }
            }
        }
    }

    referenced
}

fn referenced_variable_value_keys(variable: &VariableNode) -> BTreeSet<String> {
    let mut referenced = BTreeSet::new();
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return referenced;
    };

    for block in environments.values() {
        if let ProjectField::Present(value) = &block.value {
            referenced.insert(value.value.clone());
        }
        let RuleCollection::Rules(rules) = &block.rules else {
            continue;
        };
        for rule in rules {
            if rule.invalid_shape {
                continue;
            }
            if let ProjectField::Present(value) = &rule.value {
                referenced.insert(value.value.clone());
            }
        }
    }

    referenced
}

fn lint_schema_documents(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for document in ctx.source.documents.values() {
        if !matches!(&document.kind, DocumentKind::Schema) {
            continue;
        }
        let Some(schema) = ctx.syntax.json.get(&document.id) else {
            continue;
        };

        if let Err(err) = jsonschema::validator_for(schema) {
            push_value_diagnostic(
                &mut diagnostics,
                RototoRuleId::SchemaInvalid,
                EntityId::Schema {
                    path: document.path.clone(),
                },
                document.document_location(),
                format!("schema is invalid: {err}"),
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_variable_values(ctx: &mut LintContext) {
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

fn field_is_not_present<T>(field: &ProjectField<T>) -> bool {
    !matches!(field, ProjectField::Present(_))
}

fn field_is_integer(field: &ProjectField<i64>, expected: i64) -> bool {
    matches!(field, ProjectField::Present(value) if value.value == expected)
}

fn predicate_op_label(op: &PredicateOp) -> &'static str {
    match op {
        PredicateOp::Eq => "eq",
        PredicateOp::Neq => "neq",
        PredicateOp::In => "in",
        PredicateOp::NotIn => "not_in",
        PredicateOp::Gt => "gt",
        PredicateOp::Gte => "gte",
        PredicateOp::Lt => "lt",
        PredicateOp::Lte => "lte",
        PredicateOp::Bucket => "bucket",
        PredicateOp::Unknown(_) => "unknown",
    }
}
