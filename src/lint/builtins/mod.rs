mod graph;
mod qualifier;
mod schema;
mod variable;
mod workspace;

use super::engine::LintContext;
use super::index::{PredicateOp, ProjectField};

pub(super) use workspace::declared_workspace_environments;

pub(super) fn run_project(ctx: &mut LintContext) {
    workspace::lint_manifest_shape(ctx);
    schema::lint_context_schema_reference(ctx);
    schema::lint_schema_documents(ctx);
    workspace::lint_manifest_custom_rule_shapes(ctx);
    qualifier::lint_qualifier_shapes(ctx);
    variable::lint_variable_shapes(ctx);
    workspace::lint_custom_rule_conflicts(ctx);
}

pub(super) fn run_reference(ctx: &mut LintContext) {
    schema::lint_qualifier_context_schema_attributes(ctx);
    schema::lint_unreferenced_schemas(ctx);
    qualifier::lint_qualifier_references(ctx);
    variable::lint_variable_references(ctx);
    variable::lint_variable_schema_references(ctx);
    schema::lint_missing_context_schema_for_qualifier_attributes(ctx);
}

pub(super) fn run_value(ctx: &mut LintContext) {
    variable::lint_variable_values(ctx);
}

pub(super) fn run_graph(ctx: &mut LintContext) {
    graph::lint_qualifier_cycles(ctx);
    graph::lint_unreferenced_qualifiers(ctx);
    graph::lint_unreachable_qualifiers(ctx);
    graph::lint_shadowed_variable_rules(ctx);
    graph::lint_rules_selecting_default_value(ctx);
    graph::lint_unused_variable_values(ctx);
    qualifier::lint_duplicate_predicates(ctx);
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
