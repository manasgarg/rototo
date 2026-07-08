use serde_json::Value as JsonValue;

use crate::diagnostics::{LintDiagnostic, RototoRuleId, SemanticField};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::stages::{
    push_project_diagnostic, push_reference_diagnostic, push_value_diagnostic,
};
use super::{field_is_integer, field_is_not_present};

pub(super) fn lint_variable_shapes(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for variable in ctx.index.variables.values() {
        if !field_is_integer(&variable.schema_version, 1) {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableSchemaVersion,
                variable.field_target(SemanticField::SchemaVersion),
                variable.schema_version.location(),
                "variable must declare schema_version = 1",
            );
        }

        lint_type_source(&mut diagnostics, variable);
        lint_values_shape(&mut diagnostics, variable);
        lint_resolve_shape(&mut diagnostics, &ctx.index, variable);
    }
    ctx.diagnostics.extend(diagnostics);
}

/// Flag root identifiers a variable rule `when`/`query` expression uses that
/// rototo does not provide: the retired qualifier roots, unknown `env`
/// members, and any other unknown identifier.
pub(super) fn lint_variable_expression_roots(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for variable in ctx.index.variables.values() {
        let ResolveNode::Resolve { rules, query, .. } = &variable.resolve else {
            continue;
        };
        if let RuleCollection::Rules(rules) = rules {
            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                for (field, expression) in [(SemanticField::VariableRuleWhen, &rule.when)]
                    .into_iter()
                    .filter_map(|(field, expression)| expression.as_ref().map(|expr| (field, expr)))
                {
                    let ProjectField::Present(expression) = expression else {
                        continue;
                    };
                    for issue in &expression.value.references().invalid_roots {
                        push_project_diagnostic(
                            diagnostics,
                            RototoRuleId::VariableRuleInvalidReference,
                            rule.field_target(&variable.id, field.clone()),
                            expression.location.clone(),
                            issue.describe(),
                        );
                    }
                    for list_id in &expression.value.references().lists {
                        if !ctx.index.lists.contains_key(list_id) {
                            push_project_diagnostic(
                                diagnostics,
                                RototoRuleId::ExpressionUnknownList,
                                rule.field_target(&variable.id, field.clone()),
                                expression.location.clone(),
                                format!("expression references unknown list: {list_id}"),
                            );
                        }
                    }
                    if expression.value.references().uses_resolving {
                        push_project_diagnostic(
                            diagnostics,
                            RototoRuleId::VariableRuleInvalidReference,
                            rule.field_target(&variable.id, field.clone()),
                            expression.location.clone(),
                            "env.resolving is only available in [[trace]] policies",
                        );
                    }
                }
            }
        }
        if let Some(query) = query {
            for (field, expression) in [
                (SemanticField::VariableQueryFilter, &query.filter),
                (SemanticField::VariableQuerySort, &query.sort),
            ]
            .into_iter()
            .filter_map(|(field, expression)| expression.as_ref().map(|expr| (field, expr)))
            {
                let ProjectField::Present(expression) = expression else {
                    continue;
                };
                for issue in &expression.value.references().invalid_roots {
                    push_project_diagnostic(
                        diagnostics,
                        RototoRuleId::VariableRuleInvalidReference,
                        variable.field_target(field.clone()),
                        expression.location.clone(),
                        issue.describe(),
                    );
                }
                for list_id in &expression.value.references().lists {
                    if !ctx.index.lists.contains_key(list_id) {
                        push_project_diagnostic(
                            diagnostics,
                            RototoRuleId::ExpressionUnknownList,
                            variable.field_target(field.clone()),
                            expression.location.clone(),
                            format!("expression references unknown list: {list_id}"),
                        );
                    }
                }
                if expression.value.references().uses_resolving {
                    push_project_diagnostic(
                        diagnostics,
                        RototoRuleId::VariableRuleInvalidReference,
                        variable.field_target(field.clone()),
                        expression.location.clone(),
                        "env.resolving is only available in [[trace]] policies",
                    );
                }
            }
        }
    }
}

fn lint_type_source(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    match &variable.type_source {
        TypeSourceNode::Primitive(_) | TypeSourceNode::Catalog(_) => {}
        TypeSourceNode::Schema(schema) => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeSource,
            variable.field_target(SemanticField::VariableSchema),
            schema.location.clone(),
            "variable schemas are no longer supported; declare type instead",
        ),
        TypeSourceNode::Missing { location } | TypeSourceNode::Conflict { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableTypeSource,
                variable.field_target(SemanticField::VariableType),
                location.clone(),
                "variable must declare type",
            );
        }
        TypeSourceNode::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeSource,
            variable.field_target(SemanticField::VariableType),
            location.clone(),
            "variable type must be a string",
        ),
    }
}

fn lint_values_shape(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    if variable.values.invalid_shape || !variable.values.inline_values.is_empty() {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableValuesDisallowed,
            variable.field_target(SemanticField::VariableValues),
            variable.values.location.clone(),
            "variables must not contain [values]; put literal values directly under [resolve]",
        );
    }
}

fn lint_resolve_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    index: &SemanticIndex,
    variable: &VariableNode,
) {
    let (method, default, rules, query, assignments) = match &variable.resolve {
        ResolveNode::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableResolveMissingDefault,
                variable.field_target(SemanticField::VariableResolve),
                location.clone(),
                "variable must contain [resolve]",
            );
            return;
        }
        ResolveNode::Invalid { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableResolveShape,
                variable.field_target(SemanticField::VariableResolve),
                location.clone(),
                "resolve must be a table",
            );
            return;
        }
        ResolveNode::Resolve {
            method,
            default,
            rules,
            query,
            assignments,
            ..
        } => (method, default, rules, query, assignments),
    };

    let method_name = match method {
        None => "rules",
        Some(method) if matches!(method.value.as_str(), "rules" | "query" | "allocation") => {
            method.value.as_str()
        }
        Some(method) => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableResolveShape,
                variable.field_target(SemanticField::VariableResolve),
                method.location.clone(),
                "resolve method must be \"rules\", \"query\", or \"allocation\"",
            );
            return;
        }
    };

    if method_name != "query"
        && let Some(query) = query
    {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableQueryShape,
            variable.field_target(SemanticField::VariableResolve),
            query.location.clone(),
            "query parameters (from, filter, sort, order, limit) are only valid with \
             method = \"query\"",
        );
    }
    if method_name != "allocation"
        && let Some(assignments) = assignments
    {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableAllocationShape,
            variable.field_target(SemanticField::VariableResolve),
            assignments.location.clone(),
            "allocation parameters (allocation, [[resolve.assign]]) are only valid with \
             method = \"allocation\"",
        );
    }

    if method_name == "query" {
        lint_query_shape(diagnostics, index, variable, rules, query);
        return;
    }
    if method_name == "allocation" {
        lint_allocation_shape(diagnostics, index, variable, default, rules, assignments);
        return;
    }

    if field_is_not_present(default) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableResolveMissingDefault,
            variable.field_target(SemanticField::VariableResolveDefault),
            default.location(),
            "resolve must reference a default value",
        );
    }

    match rules {
        RuleCollection::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            variable.field_target(SemanticField::VariableResolve),
            location.clone(),
            "rule must use [[resolve.rule]] tables",
        ),
        RuleCollection::Rules(rules) => {
            for rule in rules {
                lint_variable_rule_shape(diagnostics, variable, rule);
            }
        }
    }
}

/// Coherence checks for `method = "allocation"`: the variable consumes one
/// allocation, covers exactly that allocation's arms, and keeps a default for
/// units in no arm (ineligible, unclaimed buckets, or a non-running
/// allocation).
fn lint_allocation_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    index: &SemanticIndex,
    variable: &VariableNode,
    default: &ProjectField<JsonValue>,
    rules: &RuleCollection,
    assignments: &Option<Box<AssignmentsNode>>,
) {
    match rules {
        RuleCollection::Rules(rules) if rules.is_empty() => {}
        RuleCollection::Rules(rules) => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableAllocationShape,
            variable.field_target(SemanticField::VariableResolve),
            rules[0].location.clone(),
            "method = \"allocation\" must not declare [[resolve.rule]] tables",
        ),
        RuleCollection::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableAllocationShape,
            variable.field_target(SemanticField::VariableResolve),
            location.clone(),
            "method = \"allocation\" must not declare [[resolve.rule]] tables",
        ),
    }

    if field_is_not_present(default) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableResolveMissingDefault,
            variable.field_target(SemanticField::VariableResolveDefault),
            default.location(),
            "method = \"allocation\" must declare a default for units in no arm",
        );
    }

    let Some(assignments) = assignments else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableAllocationShape,
            variable.field_target(SemanticField::VariableResolve),
            variable.resolve.location(),
            "method = \"allocation\" must declare allocation = \"<allocation-id>\"",
        );
        return;
    };

    let allocation = match &assignments.allocation {
        ProjectField::Present(allocation) => {
            let declared = index.layers.values().find_map(|layer| {
                layer.allocations.iter().find(|candidate| {
                    matches!(&candidate.id, ProjectField::Present(id) if id.value == allocation.value)
                })
            });
            if declared.is_none() {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableUnknownAllocation,
                    variable.field_target(SemanticField::VariableAllocation),
                    allocation.location.clone(),
                    format!(
                        "variable references unknown allocation: {}",
                        allocation.value
                    ),
                );
            }
            declared
        }
        ProjectField::Invalid { location } | ProjectField::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableAllocationShape,
                variable.field_target(SemanticField::VariableAllocation),
                location.clone(),
                "method = \"allocation\" must declare allocation = \"<allocation-id>\"",
            );
            None
        }
    };

    if assignments.assigns_invalid {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableAllocationShape,
            variable.field_target(SemanticField::VariableResolve),
            assignments.location.clone(),
            "assign must use [[resolve.assign]] tables",
        );
    }

    let mut assigned: Vec<&str> = Vec::new();
    for assign in &assignments.assigns {
        if assign.invalid_shape {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableAllocationShape,
                variable.field_target(SemanticField::VariableResolve),
                assign.location.clone(),
                "assign must be a table",
            );
            continue;
        }
        match &assign.arm {
            ProjectField::Present(arm) => {
                if assigned.contains(&arm.value.as_str()) {
                    push_project_diagnostic(
                        diagnostics,
                        RototoRuleId::VariableAllocationShape,
                        variable.field_target(SemanticField::VariableResolve),
                        arm.location.clone(),
                        format!("arm is assigned more than once: {}", arm.value),
                    );
                }
                assigned.push(arm.value.as_str());
                if let Some(allocation) = allocation {
                    let known = allocation.arms.iter().any(|candidate| {
                        matches!(&candidate.name, ProjectField::Present(name) if name.value == arm.value)
                    });
                    if !known {
                        push_project_diagnostic(
                            diagnostics,
                            RototoRuleId::VariableAllocationShape,
                            variable.field_target(SemanticField::VariableResolve),
                            arm.location.clone(),
                            format!(
                                "assign names an arm the allocation does not declare: {}",
                                arm.value
                            ),
                        );
                    }
                }
            }
            field => push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableAllocationShape,
                variable.field_target(SemanticField::VariableResolve),
                field.location(),
                "assign must declare arm",
            ),
        }
        if field_is_not_present(&assign.value) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableAllocationShape,
                variable.field_target(SemanticField::VariableAssignValue),
                assign.value.location(),
                "assign must declare a value",
            );
        }
    }

    if let Some(allocation) = allocation {
        for arm in &allocation.arms {
            if let ProjectField::Present(name) = &arm.name
                && !assigned.contains(&name.value.as_str())
            {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableAllocationShape,
                    variable.field_target(SemanticField::VariableResolve),
                    assignments.location.clone(),
                    format!("assign is missing for arm: {}", name.value),
                );
            }
        }
    }
}

/// Coherence checks for `method = "query"`: the pipeline reads one catalog's
/// entries, so the type must be catalog-backed, `from` must name that catalog,
/// and rule tables have no meaning.
fn lint_query_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    index: &SemanticIndex,
    variable: &VariableNode,
    rules: &RuleCollection,
    query: &Option<Box<QueryNode>>,
) {
    match rules {
        RuleCollection::Rules(rules) if rules.is_empty() => {}
        RuleCollection::Rules(rules) => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableQueryShape,
            variable.field_target(SemanticField::VariableResolve),
            rules[0].location.clone(),
            "method = \"query\" must not declare [[resolve.rule]] tables",
        ),
        RuleCollection::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableQueryShape,
            variable.field_target(SemanticField::VariableResolve),
            location.clone(),
            "method = \"query\" must not declare [[resolve.rule]] tables",
        ),
    }

    let catalog_type =
        variable_type_kind(&variable.type_source).and_then(|kind| match &kind.value {
            VariableTypeKind::Catalog(catalog) => Some(catalog.clone()),
            VariableTypeKind::Array(item) => match item.as_ref() {
                VariableTypeKind::Catalog(catalog) => Some(catalog.clone()),
                _ => None,
            },
            _ => None,
        });
    if catalog_type.is_none() {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableQueryShape,
            variable.field_target(SemanticField::VariableType),
            variable.type_source.location(),
            "method = \"query\" requires a catalog=<id> or array<catalog=<id>> type",
        );
    }

    let Some(query) = query else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableQueryShape,
            variable.field_target(SemanticField::VariableResolve),
            variable.resolve.location(),
            "method = \"query\" must declare from = \"<catalog-id>\"",
        );
        return;
    };

    match &query.from {
        ProjectField::Present(from) => {
            if !index.catalogs.contains_key(&from.value) {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableUnknownCatalog,
                    variable.field_target(SemanticField::VariableResolve),
                    from.location.clone(),
                    format!("query from references unknown catalog: {}", from.value),
                );
            } else if let Some(catalog) = &catalog_type
                && from.value != *catalog
            {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableQueryShape,
                    variable.field_target(SemanticField::VariableResolve),
                    from.location.clone(),
                    format!(
                        "query from ({}) must match the variable's catalog type ({catalog})",
                        from.value
                    ),
                );
            }
        }
        ProjectField::Invalid { location } | ProjectField::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableQueryShape,
                variable.field_target(SemanticField::VariableResolve),
                location.clone(),
                "method = \"query\" must declare from = \"<catalog-id>\"",
            );
        }
    }

    for (label, field, expression) in [
        ("filter", SemanticField::VariableQueryFilter, &query.filter),
        ("sort", SemanticField::VariableQuerySort, &query.sort),
    ] {
        if let Some(ProjectField::Invalid { location } | ProjectField::Missing { location }) =
            expression
        {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableQueryShape,
                variable.field_target(field),
                location.clone(),
                format!("query {label} must be a CEL expression string"),
            );
        }
    }

    match &query.order {
        None => {}
        Some(ProjectField::Present(order)) if matches!(order.value.as_str(), "asc" | "desc") => {
            if !matches!(query.sort, Some(ProjectField::Present(_))) {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableQueryShape,
                    variable.field_target(SemanticField::VariableResolve),
                    order.location.clone(),
                    "query order requires a sort expression",
                );
            }
        }
        Some(ProjectField::Present(order)) => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableQueryShape,
            variable.field_target(SemanticField::VariableResolve),
            order.location.clone(),
            format!("query order must be asc or desc, not {}", order.value),
        ),
        Some(ProjectField::Invalid { location } | ProjectField::Missing { location }) => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableQueryShape,
                variable.field_target(SemanticField::VariableResolve),
                location.clone(),
                "query order must be asc or desc",
            );
        }
    }

    match &query.limit {
        None => {}
        Some(ProjectField::Present(limit)) if limit.value >= 1 => {}
        Some(field) => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableQueryShape,
            variable.field_target(SemanticField::VariableResolve),
            field.location(),
            "query limit must be a positive integer",
        ),
    }
}

fn lint_variable_rule_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    rule: &VariableRuleNode,
) {
    if rule.invalid_shape {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            rule.target(&variable.id),
            rule.location.clone(),
            "rule must be a table",
        );
        return;
    }

    if let Some(ProjectField::Invalid { location } | ProjectField::Missing { location }) =
        &rule.when
    {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            rule.field_target(&variable.id, SemanticField::VariableRuleWhen),
            location.clone(),
            "rule when expression is invalid",
        );
    }
    if !matches!(rule.when, Some(ProjectField::Present(_))) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            rule.field_target(&variable.id, SemanticField::VariableRuleWhen),
            rule.location.clone(),
            "rule must declare when",
        );
    }
    if field_is_not_present(&rule.value) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            rule.field_target(&variable.id, SemanticField::VariableRuleValue),
            rule.value.location(),
            "rule must reference a value",
        );
    }
}

pub(super) fn lint_variable_references(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for edge in ctx.references.edges() {
        if edge.is_resolved() {
            continue;
        }
        match (&edge.source, &edge.target) {
            (
                ReferenceSource::VariableCatalog { variable: _ },
                ReferenceTarget::Catalog(catalog),
            ) => push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableUnknownCatalog,
                edge.semantic_target.clone(),
                edge.location.clone(),
                format!("variable references unknown catalog: {catalog}"),
            ),
            (
                ReferenceSource::VariableResolveDefault { variable: _ },
                ReferenceTarget::VariableValue { variable: _, value },
            ) => {
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.semantic_target.clone(),
                    edge.location.clone(),
                    format!("resolve default references unknown value: {value}"),
                );
            }
            (
                ReferenceSource::VariableResolveDefault { variable },
                ReferenceTarget::CatalogEntry { catalog, value },
            ) => {
                if !ctx.index.catalogs.contains_key(catalog)
                    || variable_catalog_id(ctx, variable).is_none_or(|id| id != catalog)
                {
                    continue;
                }
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.semantic_target.clone(),
                    edge.location.clone(),
                    format!("resolve default references unknown catalog value: {value}"),
                );
            }
            (
                ReferenceSource::VariableRuleConditionVariable {
                    variable: _,
                    rule: _,
                },
                ReferenceTarget::Variable(referenced),
            ) => push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableRuleUnknownVariable,
                edge.semantic_target.clone(),
                edge.location.clone(),
                format!("rule references unknown variable: {referenced}"),
            ),
            (
                ReferenceSource::VariableQueryVariable { variable: _ },
                ReferenceTarget::Variable(referenced),
            ) => push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableRuleUnknownVariable,
                edge.semantic_target.clone(),
                edge.location.clone(),
                format!("query references unknown variable: {referenced}"),
            ),
            (
                ReferenceSource::VariableRuleValue {
                    variable: _,
                    rule: _,
                },
                ReferenceTarget::VariableValue { variable: _, value },
            ) => {
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.semantic_target.clone(),
                    edge.location.clone(),
                    format!("rule references unknown value: {value}"),
                );
            }
            (
                ReferenceSource::VariableRuleValue { variable, rule: _ },
                ReferenceTarget::CatalogEntry { catalog, value },
            ) => {
                if !ctx.index.catalogs.contains_key(catalog)
                    || variable_catalog_id(ctx, variable).is_none_or(|id| id != catalog)
                {
                    continue;
                }
                push_reference_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    edge.semantic_target.clone(),
                    edge.location.clone(),
                    format!("rule references unknown catalog value: {value}"),
                );
            }
            _ => {}
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_variable_values(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for variable in ctx.index.variables.values() {
        let Some(type_kind) = variable_type_kind(&variable.type_source) else {
            continue;
        };
        match &type_kind.value {
            VariableTypeKind::Primitive(type_name) => {
                let Some(primitive) =
                    lint_primitive_type(&mut diagnostics, variable, &type_kind.location, type_name)
                else {
                    continue;
                };
                lint_primitive_resolve_values(&mut diagnostics, variable, primitive);
            }
            VariableTypeKind::Catalog(_) => lint_catalog_resolve_values(&mut diagnostics, variable),
            VariableTypeKind::List(id) => {
                lint_list_resolve_values(&mut diagnostics, ctx, variable, &type_kind.location, id);
            }
            VariableTypeKind::Array(item) => {
                lint_array_resolve_values(
                    &mut diagnostics,
                    ctx,
                    variable,
                    &type_kind.location,
                    item,
                );
            }
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_primitive_type(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    location: &crate::diagnostics::DiagnosticLocation,
    type_name: &str,
) -> Option<PrimitiveType> {
    let primitive = PrimitiveType::from_str(type_name);
    if primitive.is_none() {
        let message = match retired_colon_binding(type_name) {
            Some(spelling) => format!(
                "variable declares unknown type: {type_name}; the binder is `=` now, write {spelling}"
            ),
            None => format!("variable declares unknown type: {type_name}"),
        };
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableUnknownType,
            variable.field_target(SemanticField::VariableType),
            location.clone(),
            message,
        );
    }
    primitive
}

/// Detects the retired `catalog:<id>` / `list:<id>` colon spelling and
/// returns the `=` form to write instead.
fn retired_colon_binding(type_name: &str) -> Option<String> {
    let (class, id) = type_name.split_once(':')?;
    match crate::address::EntityClass::parse_name(class) {
        Some(class) if !id.is_empty() => Some(format!("{}={id}", class.as_str())),
        _ => None,
    }
}

fn lint_array_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    variable: &VariableNode,
    location: &crate::diagnostics::DiagnosticLocation,
    item: &VariableTypeKind,
) {
    match item {
        VariableTypeKind::Catalog(_) => lint_catalog_array_resolve_values(diagnostics, variable),
        VariableTypeKind::Primitive(type_name) => {
            let Some(primitive) = lint_primitive_type(diagnostics, variable, location, type_name)
            else {
                return;
            };
            lint_primitive_array_resolve_values(diagnostics, variable, primitive);
        }
        VariableTypeKind::List(id) => {
            lint_list_array_resolve_values(diagnostics, ctx, variable, location, id);
        }
        VariableTypeKind::Array(_) => push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableUnknownType,
            variable.field_target(SemanticField::VariableType),
            location.clone(),
            "nested array variable types are not supported",
        ),
    }
}

/// The declared members of a list, when both halves are present and valid.
/// Missing pieces are reported by the list lints, so value validation simply
/// skips when it cannot know the member set.
fn list_member_values<'a>(ctx: &'a LintContext, id: &str) -> Option<Vec<&'a JsonValue>> {
    if !ctx.index.lists.contains_key(id) {
        return None;
    }
    let members = ctx.index.lists.get(id)?;
    let ProjectField::Present(members) = &members.members else {
        return None;
    };
    Some(members.value.iter().map(|member| &member.value).collect())
}

fn lint_list_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    variable: &VariableNode,
    location: &crate::diagnostics::DiagnosticLocation,
    id: &str,
) {
    if !ctx.index.lists.contains_key(id) {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableUnknownList,
            variable.field_target(SemanticField::VariableType),
            location.clone(),
            format!("variable references unknown list: {id}"),
        );
        return;
    }
    let Some(members) = list_member_values(ctx, id) else {
        return;
    };
    for_each_resolve_value(variable, |target, value, value_location, label| {
        if !members.contains(&value) {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::VariableUnknownValue,
                target,
                value_location.clone(),
                format!(
                    "{label} is not a member of list {id}: {}",
                    value_label(value)
                ),
            );
        }
    });
}

fn lint_list_array_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    variable: &VariableNode,
    location: &crate::diagnostics::DiagnosticLocation,
    id: &str,
) {
    if !ctx.index.lists.contains_key(id) {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableUnknownList,
            variable.field_target(SemanticField::VariableType),
            location.clone(),
            format!("variable references unknown list: {id}"),
        );
        return;
    }
    let Some(members) = list_member_values(ctx, id) else {
        return;
    };
    for_each_resolve_value(variable, |target, value, value_location, label| {
        let Some(values) = value.as_array() else {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::VariableValueTypeMismatch,
                target,
                value_location.clone(),
                format!("{label} for array<list> variable must be an array"),
            );
            return;
        };
        for value in values {
            if !members.contains(&value) {
                push_value_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableUnknownValue,
                    variable.field_target(SemanticField::VariableResolveDefault),
                    value_location.clone(),
                    format!(
                        "{label} is not a member of list {id}: {}",
                        value_label(value)
                    ),
                );
            }
        }
    });
}

/// Visit the resolve default, every literal rule value, and every assign
/// value with its semantic target, value, location, and human label.
fn for_each_resolve_value(
    variable: &VariableNode,
    mut visit: impl FnMut(
        crate::diagnostics::SemanticTarget,
        &JsonValue,
        &crate::diagnostics::DiagnosticLocation,
        &str,
    ),
) {
    let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
        return;
    };
    if let ProjectField::Present(default) = default.as_ref() {
        visit(
            variable.field_target(SemanticField::VariableResolveDefault),
            &default.value,
            &default.location,
            "resolve default",
        );
    }
    if let Some(assignments) = variable.resolve.as_assignments() {
        for assign in &assignments.assigns {
            if assign.invalid_shape {
                continue;
            }
            if let ProjectField::Present(value) = &assign.value {
                visit(
                    variable.field_target(SemanticField::VariableAssignValue),
                    &value.value,
                    &value.location,
                    "assign value",
                );
            }
        }
    }
    let RuleCollection::Rules(rules) = rules else {
        return;
    };
    for rule in rules {
        if rule.invalid_shape {
            continue;
        }
        if let ProjectField::Present(value) = &rule.value {
            visit(
                rule.field_target(&variable.id, SemanticField::VariableRuleValue),
                &value.value,
                &value.location,
                "rule value",
            );
        }
    }
}

fn lint_primitive_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    primitive: PrimitiveType,
) {
    for_each_resolve_value(variable, |target, value, value_location, label| {
        lint_primitive_value(diagnostics, target, value, value_location, primitive, label);
    });
}

fn lint_primitive_array_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    primitive: PrimitiveType,
) {
    for_each_resolve_value(variable, |target, value, value_location, label| {
        lint_primitive_list_value(diagnostics, target, value, value_location, primitive, label);
    });
}

fn lint_primitive_list_value(
    diagnostics: &mut Vec<LintDiagnostic>,
    target: impl Into<crate::diagnostics::SemanticTarget>,
    value: &JsonValue,
    location: &crate::diagnostics::DiagnosticLocation,
    primitive: PrimitiveType,
    label: &str,
) {
    let Some(values) = value.as_array() else {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableValueTypeMismatch,
            target,
            location.clone(),
            format!("{label} does not match array<{}>", primitive.as_str()),
        );
        return;
    };
    if values.iter().all(|value| primitive.matches(value)) {
        return;
    }
    push_value_diagnostic(
        diagnostics,
        RototoRuleId::VariableValueTypeMismatch,
        target,
        location.clone(),
        format!(
            "{label} contains values that do not match {}",
            primitive.as_str()
        ),
    );
}

fn lint_primitive_value(
    diagnostics: &mut Vec<LintDiagnostic>,
    target: impl Into<crate::diagnostics::SemanticTarget>,
    value: &JsonValue,
    location: &crate::diagnostics::DiagnosticLocation,
    primitive: PrimitiveType,
    label: &str,
) {
    if primitive.matches(value) {
        return;
    }

    push_value_diagnostic(
        diagnostics,
        RototoRuleId::VariableValueTypeMismatch,
        target,
        location.clone(),
        format!(
            "{label} does not match type {}: {}",
            primitive.as_str(),
            value_label(value)
        ),
    );
}

fn lint_catalog_resolve_values(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    for_each_resolve_value(variable, |target, value, value_location, label| {
        lint_catalog_selector(diagnostics, target, value, value_location, label);
    });
}

fn lint_catalog_array_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
) {
    for_each_resolve_value(variable, |target, value, value_location, label| {
        lint_catalog_selector_list(diagnostics, target, value, value_location, label);
    });
}

fn lint_catalog_selector(
    diagnostics: &mut Vec<LintDiagnostic>,
    target: impl Into<crate::diagnostics::SemanticTarget>,
    value: &JsonValue,
    location: &crate::diagnostics::DiagnosticLocation,
    label: &str,
) {
    if value.is_string() {
        return;
    }

    push_value_diagnostic(
        diagnostics,
        RototoRuleId::VariableValueTypeMismatch,
        target,
        location.clone(),
        format!("{label} for catalog-backed variable must be a string"),
    );
}

fn lint_catalog_selector_list(
    diagnostics: &mut Vec<LintDiagnostic>,
    target: impl Into<crate::diagnostics::SemanticTarget>,
    value: &JsonValue,
    location: &crate::diagnostics::DiagnosticLocation,
    label: &str,
) {
    let Some(values) = value.as_array() else {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableValueTypeMismatch,
            target,
            location.clone(),
            format!("{label} for array<catalog> variable must be an array"),
        );
        return;
    };
    if values.iter().all(JsonValue::is_string) {
        return;
    }
    push_value_diagnostic(
        diagnostics,
        RototoRuleId::VariableValueTypeMismatch,
        target,
        location.clone(),
        format!("{label} for array<catalog> variable must contain only strings"),
    );
}

fn value_label(value: &JsonValue) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<value>".to_owned())
}

fn variable_catalog_id<'a>(ctx: &'a LintContext, variable: &str) -> Option<&'a str> {
    let variable = ctx.index.variables.get(variable)?;
    match &variable.type_source {
        TypeSourceNode::Catalog(catalog) => Some(catalog.value.as_str()),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum PrimitiveType {
    Bool,
    Int,
    Number,
    String,
    Array,
}

impl PrimitiveType {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "number" => Some(Self::Number),
            "string" => Some(Self::String),
            "array" => Some(Self::Array),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
        }
    }

    fn matches(self, value: &JsonValue) -> bool {
        match self {
            Self::Bool => value.is_boolean(),
            Self::Int => value.as_i64().is_some() || value.as_u64().is_some(),
            Self::Number => value.is_number(),
            Self::String => value.is_string(),
            Self::Array => value.is_array(),
        }
    }
}
