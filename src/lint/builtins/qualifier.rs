use crate::diagnostics::{RototoRuleId, SemanticField};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::stages::{push_project_diagnostic, push_reference_diagnostic};
use super::field_is_integer;

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

        match &qualifier.when {
            ProjectField::Present(_) => {}
            ProjectField::Invalid { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierWhenShape,
                qualifier.field_target(SemanticField::QualifierWhen),
                location.clone(),
                "qualifier when expression is invalid",
            ),
            ProjectField::Missing { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierWhenMissing,
                qualifier.field_target(SemanticField::QualifierWhen),
                location.clone(),
                "qualifier must declare when",
            ),
        }

        if let PredicateCollection::Invalid { location } = &qualifier.predicates {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierWhenShape,
                qualifier.field_target(SemanticField::QualifierWhen),
                location.clone(),
                "[[predicate]] is no longer supported; use when = \"...\"",
            );
        }
    }
}

pub(super) fn lint_qualifier_references(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;

    for edge in ctx.references.edges() {
        let ReferenceSource::QualifierWhenQualifier { .. } = &edge.source else {
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
            RototoRuleId::QualifierWhenUnknownQualifier,
            edge.semantic_target.clone(),
            edge.location.clone(),
            format!(
                "when expression references unknown qualifier: {}",
                reference
            ),
        );
    }
}
