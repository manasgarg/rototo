use crate::diagnostics::{LintDiagnostic, RototoRuleId, SemanticEntity};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::stages::push_project_diagnostic;

/// The three governed operations; only update and delete carry a scope,
/// expressed through the update_policy and delete_policy tables. The retired
/// names `constrain` and `override` must not be accepted.
const OPERATIONS: &[&str] = &["add", "update", "delete"];

pub(super) fn lint_governance_shape(ctx: &mut LintContext) {
    let Some(governance) = &ctx.index.governance else {
        return;
    };
    let mut diagnostics = Vec::new();
    let target = || SemanticEntity::Governance;

    for kind in &governance.unknown_kinds {
        push_project_diagnostic(
            &mut diagnostics,
            RototoRuleId::GovernanceShape,
            target(),
            kind.location.clone(),
            format!(
                "governance blocks are keyed [<kind>.<id>] with kind one of catalog, enum, \
                 variable, evaluation_context, or layer: {}",
                kind.value
            ),
        );
    }

    for block in &governance.blocks {
        let label = format!("{}.{}", block.kind, block.id);

        for key in &block.unknown_keys {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::GovernanceShape,
                target(),
                key.location.clone(),
                format!(
                    "governance block {label} declares an unknown key: {}",
                    key.value
                ),
            );
        }

        if !governed_target_exists(ctx, &block.kind, &block.id) {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::GovernanceUnknownTarget,
                target(),
                block.location.clone(),
                format!("governance block names an unknown target: {label}"),
            );
        }

        let mut allowed = Vec::new();
        for (key, list) in [
            ("allowed_operations", &block.allowed_operations),
            ("denied_operations", &block.denied_operations),
        ] {
            match list {
                None => {}
                Some(ProjectField::Present(operations)) => {
                    for operation in &operations.value {
                        if !OPERATIONS.contains(&operation.value.as_str()) {
                            push_project_diagnostic(
                                &mut diagnostics,
                                RototoRuleId::GovernanceShape,
                                target(),
                                operation.location.clone(),
                                format!(
                                    "governance operations are add, update, and delete: {}",
                                    operation.value
                                ),
                            );
                        } else if key == "allowed_operations" {
                            allowed.push(operation.value.as_str());
                        }
                    }
                }
                Some(field) => push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::GovernanceShape,
                    target(),
                    field.location(),
                    format!("governance {key} must be an array of operation names"),
                ),
            }
        }

        for (operation, policy) in [
            ("update", &block.update_policy),
            ("delete", &block.delete_policy),
        ] {
            let Some(policy) = policy else {
                // A scoped operation gated on with no policy applies to
                // everything; for update that silently includes future fields.
                if operation == "update" && allowed.contains(&"update") && block.kind == "catalog" {
                    push_project_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::GovernanceUnscopedUpdate,
                        target(),
                        block.location.clone(),
                        format!("governance block {label} grants update without allowed_fields"),
                    );
                }
                continue;
            };
            if !allowed.contains(&operation) {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::GovernanceShape,
                    target(),
                    policy.location.clone(),
                    format!(
                        "governance block {label} declares a {operation}_policy but does not \
                         allow {operation}; the policy is dead"
                    ),
                );
            }
            lint_policy(&mut diagnostics, ctx, block, &label, operation, policy);
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn lint_policy(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    block: &GovernanceBlockNode,
    label: &str,
    operation: &str,
    policy: &GovernancePolicyNode,
) {
    let target = SemanticEntity::Governance;

    for (key, list) in [
        ("allowed_entries", &policy.allowed_entries),
        ("denied_entries", &policy.denied_entries),
        ("allowed_fields", &policy.allowed_fields),
        ("denied_fields", &policy.denied_fields),
    ] {
        match list {
            None => {}
            Some(ProjectField::Present(items)) => {
                if items.value.is_empty() && key.starts_with("allowed") {
                    push_project_diagnostic(
                        diagnostics,
                        RototoRuleId::GovernanceShape,
                        target.clone(),
                        items.location.clone(),
                        format!(
                            "governance {operation}_policy {key} is empty; \"listed nothing\" \
                             reads two ways, so name targets or drop the list"
                        ),
                    );
                }
                if key.ends_with("_fields") && operation == "delete" {
                    push_project_diagnostic(
                        diagnostics,
                        RototoRuleId::GovernanceShape,
                        target.clone(),
                        items.location.clone(),
                        format!(
                            "governance delete_policy has no field scope; {key} does not apply"
                        ),
                    );
                }
                // Field names are a fixed set: a field glob matching nothing
                // in the catalog schema is an error. Entry lists may name
                // entries an overlay adds later, so they are not checked.
                if key.ends_with("_fields") && block.kind == "catalog" {
                    for item in &items.value {
                        if !field_pattern_matches_schema(ctx, &block.id, &item.value) {
                            push_project_diagnostic(
                                diagnostics,
                                RototoRuleId::GovernanceShape,
                                target.clone(),
                                item.location.clone(),
                                format!(
                                    "governance {operation}_policy names a field the {} schema \
                                     does not declare: {}",
                                    label, item.value
                                ),
                            );
                        }
                    }
                }
            }
            Some(field) => push_project_diagnostic(
                diagnostics,
                RototoRuleId::GovernanceShape,
                target.clone(),
                field.location(),
                format!("governance {operation}_policy {key} must be an array of strings"),
            ),
        }
    }
}

fn governed_target_exists(ctx: &LintContext, kind: &str, id: &str) -> bool {
    match kind {
        "catalog" => ctx.index.catalogs.contains_key(id),
        "enum" => ctx.index.enums.contains_key(id),
        "variable" => ctx.index.variables.contains_key(id),
        "evaluation_context" => ctx.index.evaluation_contexts.contains_key(id),
        "layer" => ctx.index.layers.contains_key(id),
        _ => true,
    }
}

/// Whether a field pattern (a literal or `*` glob) matches at least one
/// top-level property of the catalog's schema.
fn field_pattern_matches_schema(ctx: &LintContext, catalog: &str, pattern: &str) -> bool {
    let Some(properties) = ctx
        .index
        .catalogs
        .get(catalog)
        .and_then(|catalog| catalog.json.as_ref())
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
    else {
        // No schema to check against; the schema lint owns that failure.
        return true;
    };
    properties.keys().any(|field| glob_match(pattern, field))
}

/// Minimal glob: `*` matches any run of characters; everything else is
/// literal. This is the whole pattern language governance lists use.
pub(crate) fn glob_match(pattern: &str, value: &str) -> bool {
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else {
        return pattern == value;
    };
    if !value.starts_with(first) {
        return false;
    }
    let mut position = first.len();
    let mut last: Option<&str> = None;
    for part in parts {
        last = Some(part);
        if part.is_empty() {
            continue;
        }
        match value[position..].find(part) {
            Some(found) => position = position + found + part.len(),
            None => return false,
        }
    }
    match last {
        // No `*` at all: the pattern is a literal.
        None => pattern == value,
        Some(last) => last.is_empty() || value.ends_with(last),
    }
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn glob_matches_literals_stars_and_mixed_patterns() {
        assert!(glob_match("welcome_hero", "welcome_hero"));
        assert!(!glob_match("welcome_hero", "welcome"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("acme_*", "acme_enterprise"));
        assert!(!glob_match("acme_*", "base_plan"));
        assert!(glob_match("*_hero", "welcome_hero"));
        assert!(!glob_match("*_hero", "welcome_banner"));
        assert!(glob_match("a*c", "abc"));
        assert!(!glob_match("a*c", "abd"));
    }
}
