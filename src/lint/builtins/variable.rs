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
    let diagnostics = &mut ctx.diagnostics;
    for variable in ctx.index.variables.values() {
        if !field_is_integer(&variable.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableSchemaVersion,
                variable.field_target(SemanticField::SchemaVersion),
                variable.schema_version.location(),
                "variable must declare schema_version = 1",
            );
        }

        lint_type_source(diagnostics, variable);
        lint_values_shape(diagnostics, variable);
        lint_resolve_shape(diagnostics, variable);
    }
}

/// Flag root identifiers a variable rule `when`/`query` expression uses that
/// rototo does not provide: the retired qualifier roots, unknown `env`
/// members, and any other unknown identifier.
pub(super) fn lint_variable_expression_roots(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for variable in ctx.index.variables.values() {
        let ResolveNode::Resolve { rules, .. } = &variable.resolve else {
            continue;
        };
        let RuleCollection::Rules(rules) = rules else {
            continue;
        };
        for rule in rules {
            if rule.invalid_shape {
                continue;
            }
            for (field, expression) in [
                (SemanticField::VariableRuleWhen, &rule.when),
                (SemanticField::VariableRuleQuery, &rule.query),
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
                        rule.field_target(&variable.id, field.clone()),
                        expression.location.clone(),
                        issue.describe(),
                    );
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

fn lint_resolve_shape(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    let (default, rules) = match &variable.resolve {
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
        ResolveNode::Resolve { default, rules, .. } => (default, rules),
    };

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
    if let Some(ProjectField::Invalid { location } | ProjectField::Missing { location }) =
        &rule.query
    {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            rule.field_target(&variable.id, SemanticField::VariableRuleQuery),
            location.clone(),
            "rule query expression is invalid",
        );
    }
    if matches!(rule.query, Some(ProjectField::Present(_)))
        && variable_type_kind(&variable.type_source)
            .is_none_or(|type_kind| type_kind.value.list_catalog().is_none())
    {
        let location = rule
            .query
            .as_ref()
            .map(ProjectField::location)
            .unwrap_or_else(|| rule.location.clone());
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            rule.field_target(&variable.id, SemanticField::VariableRuleQuery),
            location,
            "rule query is only valid for list<catalog:...> variables",
        );
    }

    let has_selector = matches!(rule.when, Some(ProjectField::Present(_)))
        || matches!(rule.query, Some(ProjectField::Present(_)));
    if !has_selector {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            rule.field_target(&variable.id, SemanticField::VariableRuleWhen),
            rule.location.clone(),
            "rule must declare when or query",
        );
    }
    if rule.query.is_none() && field_is_not_present(&rule.value) {
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
            VariableTypeKind::List(item) => {
                lint_list_resolve_values(&mut diagnostics, variable, &type_kind.location, item);
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
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableUnknownType,
            variable.field_target(SemanticField::VariableType),
            location.clone(),
            format!("variable declares unknown type: {type_name}"),
        );
    }
    primitive
}

fn lint_list_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    location: &crate::diagnostics::DiagnosticLocation,
    item: &VariableTypeKind,
) {
    match item {
        VariableTypeKind::Catalog(_) => lint_catalog_list_resolve_values(diagnostics, variable),
        VariableTypeKind::Primitive(type_name) => {
            let Some(primitive) = lint_primitive_type(diagnostics, variable, location, type_name)
            else {
                return;
            };
            lint_primitive_list_resolve_values(diagnostics, variable, primitive);
        }
        VariableTypeKind::List(_) => push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableUnknownType,
            variable.field_target(SemanticField::VariableType),
            location.clone(),
            "nested list variable types are not supported",
        ),
    }
}

fn lint_primitive_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    primitive: PrimitiveType,
) {
    let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
        return;
    };
    if let ProjectField::Present(default) = default.as_ref() {
        lint_primitive_value(
            diagnostics,
            variable.field_target(SemanticField::VariableResolveDefault),
            &default.value,
            &default.location,
            primitive,
            "resolve default",
        );
    }
    let RuleCollection::Rules(rules) = rules else {
        return;
    };
    for rule in rules {
        if rule.invalid_shape {
            continue;
        }
        if let ProjectField::Present(value) = &rule.value {
            lint_primitive_value(
                diagnostics,
                rule.field_target(&variable.id, SemanticField::VariableRuleValue),
                &value.value,
                &value.location,
                primitive,
                "rule value",
            );
        }
    }
}

fn lint_primitive_list_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    primitive: PrimitiveType,
) {
    let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
        return;
    };
    if let ProjectField::Present(default) = default.as_ref() {
        lint_primitive_list_value(
            diagnostics,
            variable.field_target(SemanticField::VariableResolveDefault),
            &default.value,
            &default.location,
            primitive,
            "resolve default",
        );
    }
    let RuleCollection::Rules(rules) = rules else {
        return;
    };
    for rule in rules {
        if rule.invalid_shape || rule.query.is_some() {
            continue;
        }
        if let ProjectField::Present(value) = &rule.value {
            lint_primitive_list_value(
                diagnostics,
                rule.field_target(&variable.id, SemanticField::VariableRuleValue),
                &value.value,
                &value.location,
                primitive,
                "rule value",
            );
        }
    }
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
            format!("{label} does not match list<{}>", primitive.as_str()),
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
    let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
        return;
    };
    if let ProjectField::Present(default) = default.as_ref() {
        lint_catalog_selector(
            diagnostics,
            variable.field_target(SemanticField::VariableResolveDefault),
            &default.value,
            &default.location,
            "resolve default",
        );
    }
    let RuleCollection::Rules(rules) = rules else {
        return;
    };
    for rule in rules {
        if rule.invalid_shape {
            continue;
        }
        if let ProjectField::Present(value) = &rule.value {
            lint_catalog_selector(
                diagnostics,
                rule.field_target(&variable.id, SemanticField::VariableRuleValue),
                &value.value,
                &value.location,
                "rule value",
            );
        }
    }
}

fn lint_catalog_list_resolve_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
) {
    let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
        return;
    };
    if let ProjectField::Present(default) = default.as_ref() {
        lint_catalog_selector_list(
            diagnostics,
            variable.field_target(SemanticField::VariableResolveDefault),
            &default.value,
            &default.location,
            "resolve default",
        );
    }
    let RuleCollection::Rules(rules) = rules else {
        return;
    };
    for rule in rules {
        if rule.invalid_shape || rule.query.is_some() {
            continue;
        }
        if let ProjectField::Present(value) = &rule.value {
            lint_catalog_selector_list(
                diagnostics,
                rule.field_target(&variable.id, SemanticField::VariableRuleValue),
                &value.value,
                &value.location,
                "rule value",
            );
        }
    }
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
            format!("{label} for list<catalog> variable must be a list"),
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
        format!("{label} for list<catalog> variable must contain only strings"),
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
    List,
}

impl PrimitiveType {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "number" => Some(Self::Number),
            "string" => Some(Self::String),
            "list" => Some(Self::List),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Number => "number",
            Self::String => "string",
            Self::List => "list",
        }
    }

    fn matches(self, value: &JsonValue) -> bool {
        match self {
            Self::Bool => value.is_boolean(),
            Self::Int => value.as_i64().is_some() || value.as_u64().is_some(),
            Self::Number => value.is_number(),
            Self::String => value.is_string(),
            Self::List => value.is_array(),
        }
    }
}
