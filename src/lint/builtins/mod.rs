mod graph;
mod qualifier;
mod schema;
mod variable;
mod workspace;

use super::engine::LintContext;
use super::nodes::{PredicateOp, ProjectField};

pub(super) use qualifier::qualifier_reference;
pub(super) use workspace::{
    custom_rule_definitions_from_collection, declared_workspace_environments,
    workspace_custom_rule_definitions,
};

pub(super) fn run_project(ctx: &mut LintContext) {
    workspace::lint_manifest_shape(ctx);
    workspace::lint_manifest_custom_rule_shapes(ctx);
    qualifier::lint_qualifier_shapes(ctx);
    variable::lint_variable_shapes(ctx);
    workspace::lint_custom_rule_conflicts(ctx);
}

pub(super) fn run_reference(ctx: &mut LintContext) {
    schema::lint_context_schema_reference(ctx);
    schema::lint_qualifier_context_schema_attributes(ctx);
    qualifier::lint_qualifier_references(ctx);
    variable::lint_variable_references(ctx);
}

pub(super) fn run_value(ctx: &mut LintContext) {
    schema::lint_schema_documents(ctx);
    variable::lint_variable_values(ctx);
}

pub(super) fn run_graph(ctx: &mut LintContext) {
    graph::lint_qualifier_cycles(ctx);
    graph::lint_unreferenced_qualifiers(ctx);
    graph::lint_shadowed_variable_rules(ctx);
    graph::lint_unused_variable_values(ctx);
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
