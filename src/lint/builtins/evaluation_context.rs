use std::collections::BTreeSet;

use crate::diagnostics::{LintDiagnostic, RototoRuleId, SemanticEntity, SemanticField, Severity};

use super::super::engine::LintContext;
use super::super::evaluation_context::{
    ContextPathTypeFit, compatibility_for as evaluation_context_compatibility_for,
    context_path_type_fit, expected_type_label, path_declared_in_any_context,
    variable_query_expressions, variable_resolve_rules, variable_rule_condition_reference_count,
};
use serde_json::Value as JsonValue;

use super::super::index::{
    EvaluationContextNode, EvaluationContextSampleNode, ProjectField, SemanticIndex,
};
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::stages::{push_graph_diagnostic, push_project_diagnostic, push_value_diagnostic};
use crate::expression::ContextScalarType;

pub(super) fn lint_evaluation_context_schemas(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for evaluation_context in ctx.index.evaluation_contexts.values() {
        if let Some(message) = &evaluation_context.invalid_message {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::EvaluationContextSchemaInvalid,
                evaluation_context.field_target(SemanticField::SchemaJson),
                evaluation_context.location.clone(),
                format!("evaluation context schema is invalid: {message}"),
            );
        }
        if let Some(schema) = evaluation_context.json.as_ref() {
            lint_context_enum_ref_shapes(&mut diagnostics, ctx, evaluation_context, schema, "$");
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

/// Context schema fields may pin their values to an enum with
/// `x-rototo-ref: "enum:<id>"`. Context facts are caller data, so catalog
/// targets are rejected here; enums are the only referenceable set.
fn lint_context_enum_ref_shapes(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    evaluation_context: &EvaluationContextNode,
    schema: &JsonValue,
    pointer: &str,
) {
    if let Some(target) = schema.get("x-rototo-ref") {
        match target.as_str().map(context_enum_ref_target) {
            Some(Ok(id)) if ctx.index.enums.contains_key(&id) => {}
            Some(Ok(id)) => push_project_diagnostic(
                diagnostics,
                RototoRuleId::EvaluationContextSchemaInvalid,
                evaluation_context.field_target(SemanticField::SchemaJson),
                evaluation_context.location.clone(),
                format!("x-rototo-ref references unknown enum {id} at {pointer}"),
            ),
            Some(Err(message)) => push_project_diagnostic(
                diagnostics,
                RototoRuleId::EvaluationContextSchemaInvalid,
                evaluation_context.field_target(SemanticField::SchemaJson),
                evaluation_context.location.clone(),
                format!("{message} at {pointer}"),
            ),
            None => push_project_diagnostic(
                diagnostics,
                RototoRuleId::EvaluationContextSchemaInvalid,
                evaluation_context.field_target(SemanticField::SchemaJson),
                evaluation_context.location.clone(),
                format!(
                    "x-rototo-ref in evaluation context schemas must target enum:<id> at {pointer}"
                ),
            ),
        }
    }

    for keyword in ["properties", "$defs", "definitions"] {
        let Some(children) = schema.get(keyword).and_then(JsonValue::as_object) else {
            continue;
        };
        for (key, child) in children {
            lint_context_enum_ref_shapes(
                diagnostics,
                ctx,
                evaluation_context,
                child,
                &format!("{pointer}/{keyword}/{key}"),
            );
        }
    }
    if let Some(items) = schema.get("items") {
        lint_context_enum_ref_shapes(
            diagnostics,
            ctx,
            evaluation_context,
            items,
            &format!("{pointer}/items"),
        );
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(children) = schema.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for (index, child) in children.iter().enumerate() {
            lint_context_enum_ref_shapes(
                diagnostics,
                ctx,
                evaluation_context,
                child,
                &format!("{pointer}/{keyword}/{index}"),
            );
        }
    }
}

fn context_enum_ref_target(target: &str) -> Result<String, String> {
    if let Some(id) = target.strip_prefix("enum:") {
        if id.is_empty() {
            return Err("x-rototo-ref enum id must not be empty".to_owned());
        }
        return Ok(id.to_owned());
    }
    Err("x-rototo-ref in evaluation context schemas must target enum:<id>".to_owned())
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
            if let Some(schema) = context.json.as_ref() {
                lint_sample_enum_refs(&mut diagnostics, ctx, entry, schema, schema, value, "$");
            }
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

/// Walk the context schema and the sample together, checking every field that
/// pins its values with `x-rototo-ref: "enum:<id>"` against the enum's member
/// set. Local `#/...` `$ref`s resolve against the schema root; anything else is
/// out of scope for context schemas.
#[allow(clippy::too_many_arguments)]
fn lint_sample_enum_refs(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    entry: &EvaluationContextSampleNode,
    root: &JsonValue,
    schema: &JsonValue,
    value: &JsonValue,
    path: &str,
) {
    if let Some(reference) = schema.get("$ref").and_then(JsonValue::as_str)
        && let Some(pointer) = reference.strip_prefix("#")
        && let Some(resolved) = root.pointer(pointer)
    {
        lint_sample_enum_refs(diagnostics, ctx, entry, root, resolved, value, path);
    }

    if let Some(id) = schema
        .get("x-rototo-ref")
        .and_then(JsonValue::as_str)
        .and_then(|target| target.strip_prefix("enum:"))
        && let Some(members) = sample_enum_member_values(ctx, id)
        && !value.is_object()
        && !value.is_array()
        && !members.contains(&value)
    {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::EvaluationContextSampleSchemaMismatch,
            entry.field_target(SemanticField::EvaluationContextSample),
            entry.location.clone(),
            format!(
                "evaluation context sample {} field {path} is not a member of enum {id}: {}",
                entry.key,
                serde_json::to_string(value).unwrap_or_default()
            ),
        );
    }

    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(JsonValue::as_object),
        value.as_object(),
    ) {
        for (key, subschema) in properties {
            let Some(child) = object.get(key) else {
                continue;
            };
            lint_sample_enum_refs(
                diagnostics,
                ctx,
                entry,
                root,
                subschema,
                child,
                &format!("{path}.{key}"),
            );
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, child) in array.iter().enumerate() {
            lint_sample_enum_refs(
                diagnostics,
                ctx,
                entry,
                root,
                items,
                child,
                &format!("{path}[{index}]"),
            );
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(subschemas) = schema.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for subschema in subschemas {
            lint_sample_enum_refs(diagnostics, ctx, entry, root, subschema, value, path);
        }
    }
}

fn sample_enum_member_values<'a>(ctx: &'a LintContext, id: &str) -> Option<Vec<&'a JsonValue>> {
    if !ctx.index.enums.contains_key(id) {
        return None;
    }
    let members = ctx.index.enum_members.get(id)?;
    let ProjectField::Present(members) = &members.members else {
        return None;
    };
    Some(members.value.iter().map(|member| &member.value).collect())
}

pub(super) fn lint_undeclared_context_paths(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    let variables_with_errors = variables_with_existing_errors(ctx);

    for (variable_id, variable) in &ctx.index.variables {
        if variables_with_errors.contains(variable_id) {
            continue;
        }
        if let Some(rules) = variable_resolve_rules(variable) {
            for rule in rules {
                for expression in [&rule.when].into_iter().flatten() {
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
        for expression in variable_query_expressions(variable) {
            for path in &expression.value.references().context_paths {
                if path.is_empty() || path_declared_in_any_context(&ctx.index, path) {
                    continue;
                }
                push_graph_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableRuleUndeclaredContextPath,
                    variable.target(),
                    expression.location.clone(),
                    format!("query references undeclared context path: context.{path}"),
                );
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_context_path_types(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    let variables_with_errors = variables_with_existing_errors(ctx);

    for (variable_id, variable) in &ctx.index.variables {
        if variables_with_errors.contains(variable_id) {
            continue;
        }
        if let Some(rules) = variable_resolve_rules(variable) {
            for rule in rules {
                for expression in [&rule.when].into_iter().flatten() {
                    let ProjectField::Present(expression) = expression else {
                        continue;
                    };
                    for (path, constraints) in &expression.value.references().context_path_types {
                        let Some(expected) = type_mismatch_label(&ctx.index, path, constraints)
                        else {
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
        for expression in variable_query_expressions(variable) {
            for (path, constraints) in &expression.value.references().context_path_types {
                let Some(expected) = type_mismatch_label(&ctx.index, path, constraints) else {
                    continue;
                };
                push_graph_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableRuleContextPathTypeMismatch,
                    variable.target(),
                    expression.location.clone(),
                    format!(
                        "query uses context path context.{path} as {expected}, \
                         which no evaluation context declares with a matching type"
                    ),
                );
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
    let variables_with_errors = variables_with_existing_errors(ctx);

    for variable in ctx.index.variables.values() {
        if variables_with_errors.contains(&variable.id)
            || variable_references_error_variable(ctx, &variable.id, &variables_with_errors)
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

fn variable_references_error_variable(
    ctx: &LintContext,
    variable_id: &str,
    variables_with_errors: &BTreeSet<String>,
) -> bool {
    ctx.references.edges().iter().any(|edge| {
        matches!(
            &edge.source,
            ReferenceSource::VariableRuleConditionVariable { variable, .. }
                if variable == variable_id
        ) && matches!(
            &edge.target,
            ReferenceTarget::Variable(referenced) if variables_with_errors.contains(referenced)
        )
    })
}
