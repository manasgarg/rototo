use std::collections::BTreeSet;

use crate::diagnostics::{RototoRuleId, SemanticEntity, SemanticField, Severity};

use super::super::engine::LintContext;
use super::super::evaluation_context::{
    ContextPathTypeFit, compatibility_for as evaluation_context_compatibility_for,
    context_path_type_fit, expected_type_label, path_declared_in_any_context,
    qualifier_uses_context_attribute, variable_resolve_rules,
    variable_rule_condition_reference_count,
};
use super::super::index::{ProjectField, SemanticIndex};
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::stages::{push_graph_diagnostic, push_project_diagnostic, push_value_diagnostic};
use crate::expression::ContextScalarType;

pub(super) fn lint_evaluation_context_schemas(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for evaluation_context in ctx.index.evaluation_contexts.values() {
        if let Some(message) = &evaluation_context.invalid_message {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::EvaluationContextSchemaInvalid,
                evaluation_context.field_target(SemanticField::SchemaJson),
                evaluation_context.location.clone(),
                format!("evaluation context schema is invalid: {message}"),
            );
        }
    }
}

pub(super) fn lint_evaluation_context_reserved_fields(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for evaluation_context in ctx.index.evaluation_contexts.values() {
        let Some(json) = evaluation_context.json.as_ref() else {
            continue;
        };
        if !json
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|properties| properties.contains_key("qualifier"))
        {
            continue;
        }
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::EvaluationContextReservedField,
            evaluation_context.field_target(SemanticField::SchemaJson),
            evaluation_context.location.clone(),
            "evaluation context schema declares reserved top-level field: qualifier",
        );
    }
}

pub(super) fn lint_evaluation_context_samples(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for (evaluation_context_id, entries) in &ctx.index.evaluation_context_samples {
        let context = ctx.index.evaluation_contexts.get(evaluation_context_id);
        for entry in entries.values() {
            let Some(value) = entry.value.as_ref() else {
                continue;
            };
            if !value.is_object() {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EvaluationContextSampleShape,
                    entry.field_target(SemanticField::EvaluationContextSample),
                    entry.location.clone(),
                    format!(
                        "evaluation context sample {} must be a JSON object",
                        entry.key
                    ),
                );
                continue;
            }

            let Some(context) = context else {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EvaluationContextSampleShape,
                    entry.field_target(SemanticField::EvaluationContextSample),
                    entry.location.clone(),
                    format!(
                        "evaluation context sample {} has no owning evaluation context: {}",
                        entry.key, entry.evaluation_context_id
                    ),
                );
                continue;
            };
            let Some(validator) = context.validator.as_ref() else {
                continue;
            };
            if let Err(err) = validator.validate(value) {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EvaluationContextSampleSchemaMismatch,
                    entry.field_target(SemanticField::EvaluationContextSample),
                    entry.location.clone(),
                    format!(
                        "evaluation context sample {} does not match {} schema: {err}",
                        entry.key, entry.evaluation_context_id
                    ),
                );
            }
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_undeclared_context_paths(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    let qualifiers_with_errors = qualifiers_with_existing_errors(ctx);
    let variables_with_errors = variables_with_existing_errors(ctx);

    for qualifier in ctx.index.qualifiers.values() {
        if qualifiers_with_errors.contains(&qualifier.id) {
            continue;
        }
        let ProjectField::Present(when) = &qualifier.when else {
            continue;
        };
        for path in &when.value.references().context_paths {
            if path.is_empty() || path_declared_in_any_context(&ctx.index, path) {
                continue;
            }
            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::QualifierWhenUndeclaredContextPath,
                qualifier.target(),
                qualifier.location.clone(),
                format!("when expression references undeclared context path: context.{path}"),
            );
        }
    }

    for (variable_id, variable) in &ctx.index.variables {
        if variables_with_errors.contains(variable_id) {
            continue;
        }
        let Some(rules) = variable_resolve_rules(variable) else {
            continue;
        };
        for rule in rules {
            for expression in [&rule.when, &rule.query].into_iter().flatten() {
                let ProjectField::Present(expression) = expression else {
                    continue;
                };
                for path in &expression.value.references().context_paths {
                    if path.is_empty() || path_declared_in_any_context(&ctx.index, path) {
                        continue;
                    }
                    push_graph_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::VariableRuleUndeclaredContextPath,
                        rule.target(variable_id),
                        rule.location.clone(),
                        format!("rule references undeclared context path: context.{path}"),
                    );
                }
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_context_path_types(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    let qualifiers_with_errors = qualifiers_with_existing_errors(ctx);
    let variables_with_errors = variables_with_existing_errors(ctx);

    for qualifier in ctx.index.qualifiers.values() {
        if qualifiers_with_errors.contains(&qualifier.id) {
            continue;
        }
        let ProjectField::Present(when) = &qualifier.when else {
            continue;
        };
        for (path, constraints) in &when.value.references().context_path_types {
            let Some(expected) = type_mismatch_label(&ctx.index, path, constraints) else {
                continue;
            };
            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::QualifierWhenContextPathTypeMismatch,
                qualifier.target(),
                qualifier.location.clone(),
                format!(
                    "when expression uses context path context.{path} as {expected}, \
                     which no evaluation context declares with a matching type"
                ),
            );
        }
    }

    for (variable_id, variable) in &ctx.index.variables {
        if variables_with_errors.contains(variable_id) {
            continue;
        }
        let Some(rules) = variable_resolve_rules(variable) else {
            continue;
        };
        for rule in rules {
            for expression in [&rule.when, &rule.query].into_iter().flatten() {
                let ProjectField::Present(expression) = expression else {
                    continue;
                };
                for (path, constraints) in &expression.value.references().context_path_types {
                    let Some(expected) = type_mismatch_label(&ctx.index, path, constraints) else {
                        continue;
                    };
                    push_graph_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::VariableRuleContextPathTypeMismatch,
                        rule.target(variable_id),
                        rule.location.clone(),
                        format!(
                            "rule uses context path context.{path} as {expected}, \
                             which no evaluation context declares with a matching type"
                        ),
                    );
                }
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

/// If a declared context path is used with scalar constraints that no
/// evaluation context can satisfy, return the expected-type label for the
/// diagnostic. Returns `None` when the path is undeclared (owned by the
/// undeclared-path rule) or when at least one context declares a matching type.
fn type_mismatch_label(
    index: &SemanticIndex,
    path: &str,
    constraints: &BTreeSet<ContextScalarType>,
) -> Option<String> {
    if constraints.is_empty() {
        return None;
    }
    let mut any_declared = false;
    let mut any_ok = false;
    for context in index.evaluation_contexts.values() {
        let Some(schema) = context.json.as_ref() else {
            continue;
        };
        match context_path_type_fit(schema, path, constraints) {
            ContextPathTypeFit::Missing => {}
            ContextPathTypeFit::Ok => {
                any_declared = true;
                any_ok = true;
            }
            ContextPathTypeFit::Untyped | ContextPathTypeFit::Mismatch => {
                any_declared = true;
            }
        }
    }
    (any_declared && !any_ok).then(|| expected_type_label(constraints))
}

pub(super) fn lint_evaluation_context_compatibility(ctx: &mut LintContext) {
    let compatibility = evaluation_context_compatibility_for(&ctx.index, &ctx.references);
    let mut diagnostics = Vec::new();
    let qualifiers_with_errors = qualifiers_with_existing_errors(ctx);
    let variables_with_errors = variables_with_existing_errors(ctx);
    let mut qualifiers_without_context = BTreeSet::new();

    for qualifier in ctx.index.qualifiers.values() {
        if qualifiers_with_errors.contains(&qualifier.id) {
            continue;
        }
        let contexts = compatibility
            .qualifiers
            .get(&qualifier.id)
            .cloned()
            .unwrap_or_default();
        if contexts.is_empty() && qualifier_uses_context_attribute(&ctx.references, &qualifier.id) {
            qualifiers_without_context.insert(qualifier.id.clone());
            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::QualifierNoCompatibleEvaluationContext,
                qualifier.target(),
                qualifier.location.clone(),
                format!(
                    "qualifier {} has no compatible evaluation context",
                    qualifier.id
                ),
            );
        }
    }

    for variable in ctx.index.variables.values() {
        if variables_with_errors.contains(&variable.id)
            || variable_references_error_qualifier(ctx, &variable.id, &qualifiers_with_errors)
            || variable_references_error_qualifier(ctx, &variable.id, &qualifiers_without_context)
        {
            continue;
        }
        if variable_rule_condition_reference_count(&ctx.index, &variable.id) == 0 {
            continue;
        }
        let contexts = compatibility
            .variables
            .get(&variable.id)
            .cloned()
            .unwrap_or_default();
        if contexts.is_empty() {
            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableEvaluationContextConflict,
                variable.target(),
                variable.location.clone(),
                format!(
                    "variable {} has no evaluation context shared by all rule conditions",
                    variable.id
                ),
            );
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifiers_with_existing_errors(ctx: &LintContext) -> BTreeSet<String> {
    ctx.diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .filter_map(|diagnostic| match &diagnostic.target.entity {
            SemanticEntity::Qualifier { id } => Some(id.clone()),
            SemanticEntity::Predicate { qualifier, .. } => Some(qualifier.clone()),
            _ => None,
        })
        .collect()
}

fn variables_with_existing_errors(ctx: &LintContext) -> BTreeSet<String> {
    ctx.diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .filter_map(|diagnostic| match &diagnostic.target.entity {
            SemanticEntity::Variable { id } => Some(id.clone()),
            SemanticEntity::Value { variable, .. } => Some(variable.clone()),
            SemanticEntity::Rule { variable, .. } => Some(variable.clone()),
            _ => None,
        })
        .collect()
}

fn variable_references_error_qualifier(
    ctx: &LintContext,
    variable_id: &str,
    qualifiers_with_errors: &BTreeSet<String>,
) -> bool {
    ctx.references.edges().iter().any(|edge| {
        matches!(
            &edge.source,
            ReferenceSource::VariableRuleConditionQualifier { variable, .. }
                if variable == variable_id
        ) && matches!(
            &edge.target,
            ReferenceTarget::Qualifier(qualifier) if qualifiers_with_errors.contains(qualifier)
        )
    })
}
