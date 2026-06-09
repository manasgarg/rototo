use std::collections::BTreeMap;

use crate::diagnostics::{LintDiagnostic, RelatedLocation, RototoRuleId, SemanticField};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::stages::{
    push_graph_diagnostic, push_project_diagnostic, push_reference_diagnostic,
};
use super::{field_is_integer, field_is_not_present, predicate_op_label};

pub(super) fn lint_qualifier_shapes(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for qualifier in ctx.index.qualifiers.values() {
        if !field_is_integer(&qualifier.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierSchemaVersion,
                qualifier.field_target(SemanticField::SchemaVersion),
                qualifier.schema_version.location(),
                "qualifier must declare schema_version = 1",
            );
        }

        match &qualifier.predicates {
            PredicateCollection::Missing { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateMissing,
                qualifier.field_target(SemanticField::QualifierPredicates),
                location.clone(),
                "qualifier must contain at least one [[predicate]]",
            ),
            PredicateCollection::Invalid { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateShape,
                qualifier.field_target(SemanticField::QualifierPredicates),
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
    if field_is_not_present(&predicate.attribute) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateShape,
            predicate.field_target(&qualifier.id, SemanticField::PredicateAttribute),
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
                predicate.field_target(&qualifier.id, SemanticField::PredicateOp),
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
            predicate.field_target(&qualifier.id, SemanticField::PredicateOp),
            predicate.op.location(),
            format!("predicate has unknown op: {op}"),
        );
    }

    if matches!(op, PredicateOp::Bucket) {
        lint_bucket_predicate(diagnostics, qualifier, predicate);
    } else {
        lint_comparison_predicate(diagnostics, qualifier, predicate, op);
    }
}

fn lint_bucket_predicate(
    diagnostics: &mut Vec<LintDiagnostic>,
    qualifier: &QualifierNode,
    predicate: &PredicateNode,
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
            predicate.field_target(&qualifier.id, SemanticField::PredicateSalt),
            location,
            "bucket predicate must contain salt",
        );
    }

    let Some(range) = &predicate.range else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            predicate.field_target(&qualifier.id, SemanticField::PredicateRange),
            predicate.location.clone(),
            "bucket predicate must contain range",
        );
        return;
    };

    if !range.is_array {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            predicate.field_target(&qualifier.id, SemanticField::PredicateRange),
            range.location.clone(),
            "bucket range must be a list",
        );
    } else if range.len != 2 || range.start.is_none() || range.end.is_none() {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            predicate.field_target(&qualifier.id, SemanticField::PredicateRange),
            range.location.clone(),
            "bucket range must contain two integers",
        );
    } else {
        match (range.start, range.end) {
            (Some(start), Some(end)) if 0 <= start && start < end && end <= 10_000 => {}
            _ => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateBucket,
                predicate.field_target(&qualifier.id, SemanticField::PredicateRange),
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
            predicate.field_target(&qualifier.id, SemanticField::PredicateValue),
            location,
            "bucket predicate must not contain value",
        );
    }
}

fn lint_comparison_predicate(
    diagnostics: &mut Vec<LintDiagnostic>,
    qualifier: &QualifierNode,
    predicate: &PredicateNode,
    op: &PredicateOp,
) {
    let Some(value) = &predicate.value else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateValue,
            predicate.field_target(&qualifier.id, SemanticField::PredicateValue),
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
                predicate.field_target(&qualifier.id, SemanticField::PredicateValue),
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
                predicate.field_target(&qualifier.id, SemanticField::PredicateValue),
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

pub(super) fn lint_qualifier_references(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;

    for edge in ctx.references.edges() {
        let ReferenceSource::QualifierPredicateQualifier { .. } = &edge.source else {
            continue;
        };
        if edge.is_resolved() {
            continue;
        }
        let ReferenceTarget::Qualifier(reference) = &edge.target else {
            continue;
        };

        push_reference_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateUnknownQualifier,
            edge.semantic_target.clone(),
            edge.location.clone(),
            format!(
                "predicate references unknown qualifier: {}",
                reference_label(reference)
            ),
        );
    }
}

pub(super) fn lint_duplicate_predicates(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };
        let mut seen: BTreeMap<String, &PredicateNode> = BTreeMap::new();

        for predicate in predicates {
            let Some(key) = predicate_key(predicate) else {
                continue;
            };
            if let Some(first) = seen.get(&key) {
                push_graph_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::QualifierPredicateDuplicate,
                    predicate.target(&qualifier.id),
                    predicate.location.clone(),
                    format!(
                        "predicate duplicates an earlier predicate: {}",
                        predicate.index + 1
                    ),
                );
                if let Some(diagnostic) = diagnostics.last_mut() {
                    diagnostic.related.push(RelatedLocation {
                        location: first.location.clone(),
                        message: format!("first matching predicate: {}", first.index + 1),
                    });
                }
            } else {
                seen.insert(key, predicate);
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn predicate_key(predicate: &PredicateNode) -> Option<String> {
    let ProjectField::Present(attribute) = &predicate.attribute else {
        return None;
    };
    let ProjectField::Present(op) = &predicate.op else {
        return None;
    };

    let mut key = format!("{}|{}", attribute.value, op.value.as_str());
    if matches!(op.value, PredicateOp::Bucket) {
        let salt = predicate.salt.as_ref().and_then(|salt| match salt {
            ProjectField::Present(salt) => Some(salt.value.as_str()),
            ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
        })?;
        let range = predicate.range.as_ref()?;
        key.push_str(&format!(
            "|salt={salt}|range={:?}:{:?}",
            range.start, range.end
        ));
    } else {
        let value = predicate.value.as_ref()?;
        let value = serde_json::to_string(&value.value).ok()?;
        key.push('|');
        key.push_str(&value);
    }
    Some(key)
}

fn reference_label(reference: &str) -> &str {
    if reference.is_empty() {
        "<empty>"
    } else {
        reference
    }
}
