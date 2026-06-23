use std::collections::BTreeSet;

use crate::diagnostics::{RototoRuleId, SemanticEntity, SemanticField, Severity};

use super::super::engine::LintContext;
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::request_context::{
    compatibility_for as request_context_compatibility_for, qualifier_uses_context_attribute,
    variable_rule_condition_reference_count,
};
use super::super::stages::{push_graph_diagnostic, push_project_diagnostic, push_value_diagnostic};

pub(super) fn lint_request_context_schemas(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for request_context in ctx.index.request_contexts.values() {
        if let Some(message) = &request_context.invalid_message {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::RequestContextSchemaInvalid,
                request_context.field_target(SemanticField::SchemaJson),
                request_context.location.clone(),
                format!("request context schema is invalid: {message}"),
            );
        }
    }
}

pub(super) fn lint_request_context_reserved_fields(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for request_context in ctx.index.request_contexts.values() {
        let Some(json) = request_context.json.as_ref() else {
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
            RototoRuleId::RequestContextReservedField,
            request_context.field_target(SemanticField::SchemaJson),
            request_context.location.clone(),
            "request context schema declares reserved top-level field: qualifier",
        );
    }
}

pub(super) fn lint_request_context_entries(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for (request_context_id, entries) in &ctx.index.request_context_entries {
        let context = ctx.index.request_contexts.get(request_context_id);
        for entry in entries.values() {
            let Some(value) = entry.value.as_ref() else {
                continue;
            };
            if !value.is_object() {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::RequestContextEntryShape,
                    entry.field_target(SemanticField::RequestContextEntry),
                    entry.location.clone(),
                    format!("request context sample {} must be a JSON object", entry.key),
                );
                continue;
            }

            let Some(context) = context else {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::RequestContextEntryShape,
                    entry.field_target(SemanticField::RequestContextEntry),
                    entry.location.clone(),
                    format!(
                        "request context sample {} has no owning request context: {}",
                        entry.key, entry.request_context_id
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
                    RototoRuleId::RequestContextEntrySchemaMismatch,
                    entry.field_target(SemanticField::RequestContextEntry),
                    entry.location.clone(),
                    format!(
                        "request context sample {} does not match {} schema: {err}",
                        entry.key, entry.request_context_id
                    ),
                );
            }
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_request_context_compatibility(ctx: &mut LintContext) {
    let compatibility = request_context_compatibility_for(&ctx.index, &ctx.references);
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
                RototoRuleId::QualifierNoCompatibleRequestContext,
                qualifier.target(),
                qualifier.location.clone(),
                format!(
                    "qualifier {} has no compatible request context",
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
                RototoRuleId::VariableRequestContextConflict,
                variable.target(),
                variable.location.clone(),
                format!(
                    "variable {} has no request context shared by all rule conditions",
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
