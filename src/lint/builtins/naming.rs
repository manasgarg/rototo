use crate::diagnostics::{RototoRuleId, SemanticTarget};

use super::super::engine::LintContext;
use super::super::index::ProjectField;
use super::super::stages::push_project_diagnostic;

/// rototo-recognized ids appear in TOML table headers and CEL expressions
/// (`variables.premium_users`), where a hyphen parses as subtraction. Every id
/// is lowercase snake_case, with `/` separating namespace segments.
pub(super) fn lint_id_naming(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    let mut check = |id: &str,
                     target: SemanticTarget,
                     location: crate::diagnostics::DiagnosticLocation,
                     kind: &str| {
        if !id_is_snake_case(id) {
            // Deleted and update markers compose through extends and are
            // consumed when layers flatten; one surviving into lint has no
            // base package.
            let is_marker_id = (kind == "catalog entry"
                && (id.ends_with(".deleted") || id.ends_with(".update")))
                || (kind == "variable" && id.ends_with(".update"));
            let message = if is_marker_id {
                format!(
                    "{kind} deleted and update markers apply to a base package through extends; \
                     this package has no base entry for them to compose with: {id}"
                )
            } else {
                format!("{kind} id must be snake_case: {id}")
            };
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::IdNotSnakeCase,
                target,
                location,
                message,
            );
        }
    };

    for variable in ctx.index.variables.values() {
        check(
            &variable.id,
            variable.target(),
            variable.location.clone(),
            "variable",
        );
    }
    for declaration in ctx.index.enums.values() {
        check(
            &declaration.id,
            declaration.target(),
            declaration.location.clone(),
            "enum",
        );
    }
    for layer in ctx.index.layers.values() {
        check(&layer.id, layer.target(), layer.location.clone(), "layer");
        for allocation in &layer.allocations {
            if let ProjectField::Present(id) = &allocation.id {
                check(&id.value, layer.target(), id.location.clone(), "allocation");
            }
            for arm in &allocation.arms {
                if let ProjectField::Present(name) = &arm.name {
                    check(&name.value, layer.target(), name.location.clone(), "arm");
                }
            }
        }
    }
    for catalog in ctx.index.catalogs.values() {
        check(
            &catalog.id,
            catalog.target(),
            catalog.location.clone(),
            "catalog",
        );
    }
    for entries in ctx.index.catalog_entries.values() {
        for entry in entries.values() {
            check(
                &entry.key,
                entry.target(),
                entry.location.clone(),
                "catalog entry",
            );
        }
    }
    for evaluation_context in ctx.index.evaluation_contexts.values() {
        check(
            &evaluation_context.id,
            evaluation_context.target(),
            evaluation_context.location.clone(),
            "evaluation context",
        );
    }
    for samples in ctx.index.evaluation_context_samples.values() {
        for sample in samples.values() {
            check(
                &sample.key,
                sample.target(),
                sample.location.clone(),
                "evaluation context sample",
            );
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn id_is_snake_case(id: &str) -> bool {
    !id.is_empty()
        && id.split('/').all(|segment| {
            !segment.is_empty()
                && segment.split('_').all(|part| {
                    !part.is_empty()
                        && part
                            .chars()
                            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
                })
        })
}
