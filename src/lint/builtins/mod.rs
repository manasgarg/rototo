mod catalog;
mod evaluation_context;
mod graph;
mod package;
mod qualifier;
mod schema;
mod variable;

use super::engine::LintContext;
use super::index::ProjectField;

pub(super) fn run_project(ctx: &mut LintContext) {
    package::lint_manifest_shape(ctx);
    evaluation_context::lint_evaluation_context_schemas(ctx);
    evaluation_context::lint_evaluation_context_reserved_fields(ctx);
    qualifier::lint_qualifier_shapes(ctx);
    catalog::lint_catalog_shapes(ctx);
    variable::lint_variable_shapes(ctx);
}

pub(super) fn run_reference(ctx: &mut LintContext) {
    qualifier::lint_qualifier_references(ctx);
    variable::lint_variable_references(ctx);
}

pub(super) fn run_value(ctx: &mut LintContext) {
    evaluation_context::lint_evaluation_context_samples(ctx);
    catalog::lint_catalog_entries(ctx);
    variable::lint_variable_values(ctx);
}

pub(super) fn run_graph(ctx: &mut LintContext) {
    graph::lint_qualifier_cycles(ctx);
    graph::lint_unreferenced_qualifiers(ctx);
    graph::lint_unreachable_qualifiers(ctx);
    graph::lint_shadowed_variable_rules(ctx);
    graph::lint_rules_selecting_default_value(ctx);
    evaluation_context::lint_evaluation_context_compatibility(ctx);
}

fn field_is_not_present<T>(field: &ProjectField<T>) -> bool {
    !matches!(field, ProjectField::Present(_))
}

fn field_is_integer(field: &ProjectField<i64>, expected: i64) -> bool {
    matches!(field, ProjectField::Present(value) if value.value == expected)
}
