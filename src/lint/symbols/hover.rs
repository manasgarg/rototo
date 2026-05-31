use serde_json::Value as JsonValue;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, DiagnosticRule, LintDiagnostic,
    Severity, SourcePosition,
};

use super::super::WorkspaceLintSnapshot;
use super::super::builtins::custom_rule_definitions_from_collection;
use super::super::nodes::*;
use super::WorkspaceHover;
use super::common::{
    location_contains_position, predicate_op_project_field_value, source_range_size,
    string_project_field_value,
};

pub(crate) fn hover(
    snapshot: &WorkspaceLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<WorkspaceHover> {
    let mut candidates = Vec::new();
    push_diagnostic_hover_candidates(snapshot, path, position, &mut candidates);
    push_manifest_hover_candidates(&snapshot.index, path, position, &mut candidates);
    push_qualifier_hover_candidates(&snapshot.index, path, position, &mut candidates);
    push_variable_hover_candidates(&snapshot.index, path, position, &mut candidates);
    sort_hover_candidates(&mut candidates);
    candidates
        .into_iter()
        .next()
        .map(|candidate| candidate.hover)
        .or_else(|| file_hover(&snapshot.index, path))
}

struct HoverCandidate {
    priority: u8,
    span_size: usize,
    hover: WorkspaceHover,
}

fn push_diagnostic_hover_candidates(
    snapshot: &WorkspaceLintSnapshot,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    for diagnostic in &snapshot.lint.diagnostics {
        let contents = diagnostic_hover_contents(&snapshot.index, diagnostic);
        push_hover_candidate(candidates, path, position, &diagnostic.primary, 0, contents);
    }
}

fn push_manifest_hover_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    let Some(manifest) = &index.manifest else {
        return;
    };
    let CustomRuleCollection::Rules(rules) = &manifest.custom_rules else {
        return;
    };

    for rule in rules {
        let Some(definition) = custom_rule_definition_from_declaration(rule) else {
            continue;
        };
        push_hover_candidate(
            candidates,
            path,
            position,
            &rule.location,
            1,
            custom_rule_hover_contents(&definition),
        );
        for location in [
            Some(rule.id.location()),
            Some(rule.title.location()),
            Some(rule.help.location()),
            rule.severity.as_ref().map(ProjectField::location),
        ]
        .into_iter()
        .flatten()
        {
            push_hover_candidate(
                candidates,
                path,
                position,
                &location,
                1,
                custom_rule_hover_contents(&definition),
            );
        }
    }
}

fn push_qualifier_hover_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    for qualifier in index.qualifiers.values() {
        if qualifier.location.path != path {
            continue;
        }

        if let Some(ProjectField::Present(description)) = &qualifier.description {
            push_hover_candidate(
                candidates,
                path,
                position,
                &description.location,
                2,
                qualifier_hover_contents(qualifier),
            );
        }

        if let PredicateCollection::Predicates(predicates) = &qualifier.predicates {
            for predicate in predicates {
                push_hover_candidate(
                    candidates,
                    path,
                    position,
                    &predicate.location,
                    3,
                    predicate_hover_contents(qualifier, predicate),
                );
                for location in [
                    Some(predicate.attribute.location()),
                    Some(predicate.op.location()),
                    predicate.value.as_ref().map(|value| value.location.clone()),
                    predicate.salt.as_ref().map(ProjectField::location),
                    predicate.range.as_ref().map(|range| range.location.clone()),
                ]
                .into_iter()
                .flatten()
                {
                    push_hover_candidate(
                        candidates,
                        path,
                        position,
                        &location,
                        2,
                        predicate_hover_contents(qualifier, predicate),
                    );
                }
            }
        }
    }
}

fn push_variable_hover_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    for variable in index.variables.values() {
        if variable.location.path != path {
            continue;
        }

        if let Some(ProjectField::Present(description)) = &variable.description {
            push_hover_candidate(
                candidates,
                path,
                position,
                &description.location,
                2,
                variable_hover_contents(variable),
            );
        }

        push_hover_candidate(
            candidates,
            path,
            position,
            &variable.type_source.location(),
            2,
            variable_type_hover_contents(variable),
        );

        push_hover_candidate(
            candidates,
            path,
            position,
            &variable.values.location,
            4,
            variable_values_hover_contents(variable),
        );
        for value in variable.values.inline_values.values() {
            push_hover_candidate(
                candidates,
                path,
                position,
                &value.location,
                2,
                value_hover_contents(&variable.id, value),
            );
        }

        if let EnvironmentCollection::Environments(environments) = &variable.environments {
            for block in environments.values() {
                push_hover_candidate(
                    candidates,
                    path,
                    position,
                    &block.value.location(),
                    3,
                    environment_block_hover_contents(variable, block),
                );
                push_hover_candidate(
                    candidates,
                    path,
                    position,
                    &block.location,
                    4,
                    environment_block_hover_contents(variable, block),
                );
                if let RuleCollection::Rules(rules) = &block.rules {
                    for rule in rules {
                        push_hover_candidate(
                            candidates,
                            path,
                            position,
                            &rule.location,
                            3,
                            variable_rule_hover_contents(variable, block, rule),
                        );
                        for location in [rule.qualifier.location(), rule.value.location()] {
                            push_hover_candidate(
                                candidates,
                                path,
                                position,
                                &location,
                                2,
                                variable_rule_hover_contents(variable, block, rule),
                            );
                        }
                    }
                }
            }
        }
    }

    for (variable_id, values) in &index.external_values {
        for value in values.values() {
            push_hover_candidate(
                candidates,
                path,
                position,
                &value.location,
                2,
                value_hover_contents(variable_id, value),
            );
        }
    }
}

fn push_hover_candidate(
    candidates: &mut Vec<HoverCandidate>,
    path: &str,
    position: SourcePosition,
    location: &DiagnosticLocation,
    priority: u8,
    contents: String,
) {
    if !location_contains_position(location, path, position) {
        return;
    }
    candidates.push(HoverCandidate {
        priority,
        span_size: location.range.map(source_range_size).unwrap_or(usize::MAX),
        hover: WorkspaceHover {
            contents,
            location: location.clone(),
        },
    });
}

fn sort_hover_candidates(candidates: &mut [HoverCandidate]) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.span_size.cmp(&right.span_size))
            .then_with(|| left.hover.contents.cmp(&right.hover.contents))
    });
}

fn file_hover(index: &SemanticIndex, path: &str) -> Option<WorkspaceHover> {
    index
        .variables
        .values()
        .find(|variable| variable.location.path == path)
        .map(|variable| WorkspaceHover {
            contents: variable_hover_contents(variable),
            location: variable.location.clone(),
        })
        .or_else(|| {
            index
                .qualifiers
                .values()
                .find(|qualifier| qualifier.location.path == path)
                .map(|qualifier| WorkspaceHover {
                    contents: qualifier_hover_contents(qualifier),
                    location: qualifier.location.clone(),
                })
        })
        .or_else(|| {
            index
                .external_values
                .iter()
                .find_map(|(variable_id, values)| {
                    values
                        .values()
                        .find(|value| value.location.path == path)
                        .map(|value| WorkspaceHover {
                            contents: value_hover_contents(variable_id, value),
                            location: value.location.clone(),
                        })
                })
        })
}

fn diagnostic_hover_contents(index: &SemanticIndex, diagnostic: &LintDiagnostic) -> String {
    let (title, help) = diagnostic_rule_title_help(index, &diagnostic.rule);
    format!(
        "### {title}\n\n`{}`\n\n{}\n\n{}",
        diagnostic.rule.as_string(),
        diagnostic.message,
        help
    )
}

fn diagnostic_rule_title_help(index: &SemanticIndex, rule: &DiagnosticRule) -> (String, String) {
    match rule {
        DiagnosticRule::Rototo(rule) => {
            let meta = rule.meta();
            (meta.title.to_owned(), meta.help.to_owned())
        }
        DiagnosticRule::Custom(rule) => custom_rule_definition(index, rule)
            .map(|definition| (definition.title, definition.help))
            .unwrap_or_else(|| {
                (
                    rule.as_str().to_owned(),
                    "Workspace custom lint.".to_owned(),
                )
            }),
    }
}

fn custom_rule_definition(
    index: &SemanticIndex,
    rule: &CustomRuleId,
) -> Option<CustomRuleDefinition> {
    let manifest = index.manifest.as_ref()?;
    custom_rule_definitions_from_collection(&manifest.custom_rules)
        .into_iter()
        .map(|(definition, _)| definition)
        .find(|definition| &definition.rule == rule)
}

fn custom_rule_definition_from_declaration(
    rule: &CustomRuleDeclarationNode,
) -> Option<CustomRuleDefinition> {
    let (ProjectField::Present(id), ProjectField::Present(title), ProjectField::Present(help)) =
        (&rule.id, &rule.title, &rule.help)
    else {
        return None;
    };
    let rule_id = CustomRuleId::parse(&id.value).ok()?;
    let severity = match &rule.severity {
        Some(ProjectField::Present(severity)) => severity.value,
        Some(ProjectField::Invalid { .. }) => return None,
        Some(ProjectField::Missing { .. }) | None => Severity::Error,
    };
    Some(CustomRuleDefinition::with_severity(
        rule_id,
        severity,
        title.value.clone(),
        help.value.clone(),
    ))
}

fn custom_rule_hover_contents(definition: &CustomRuleDefinition) -> String {
    format!(
        "### Custom rule `{}`\n\n{}\n\n{}",
        definition.rule, definition.title, definition.help
    )
}

fn qualifier_hover_contents(qualifier: &QualifierNode) -> String {
    let mut contents = format!("### Qualifier `{}`", qualifier.id);
    if let Some(description) = project_field_string(&qualifier.description) {
        contents.push_str("\n\n");
        contents.push_str(description);
    }
    contents
}

fn predicate_hover_contents(qualifier: &QualifierNode, predicate: &PredicateNode) -> String {
    let mut contents = format!(
        "### Predicate {} for `{}`\n\n{}",
        predicate.index + 1,
        qualifier.id,
        predicate_summary(predicate)
    );
    if let Some(value) = &predicate.value {
        contents.push_str("\n\nValue shape: `");
        contents.push_str(value.shape.as_str());
        contents.push('`');
    }
    contents
}

fn predicate_summary(predicate: &PredicateNode) -> String {
    match (
        string_project_field_value(&predicate.attribute),
        predicate_op_project_field_value(&predicate.op),
    ) {
        (Some(attribute), Some(op)) => format!("`{attribute}` `{op}`"),
        (Some(attribute), None) => format!("`{attribute}`"),
        (None, Some(op)) => format!("operator `{op}`"),
        (None, None) => "Incomplete predicate".to_owned(),
    }
}

fn variable_hover_contents(variable: &VariableNode) -> String {
    let mut contents = format!(
        "### Variable `{}`\n\n{}",
        variable.id,
        type_source_summary(variable)
    );
    if let Some(description) = project_field_string(&variable.description) {
        contents.push_str("\n\n");
        contents.push_str(description);
    }
    let values = variable_value_keys(variable);
    if !values.is_empty() {
        contents.push_str("\n\nValues: ");
        contents.push_str(&values.join(", "));
    }
    contents
}

fn variable_type_hover_contents(variable: &VariableNode) -> String {
    format!(
        "### Variable `{}`\n\n{}",
        variable.id,
        type_source_summary(variable)
    )
}

fn variable_values_hover_contents(variable: &VariableNode) -> String {
    let values = variable_value_keys(variable);
    if values.is_empty() {
        return format!("### Values for `{}`\n\nNo values declared.", variable.id);
    }
    format!("### Values for `{}`\n\n{}", variable.id, values.join(", "))
}

fn value_hover_contents(variable_id: &str, value: &ValueNode) -> String {
    format!(
        "### Value `{}`\n\nVariable: `{}`\n\nJSON shape: `{}`",
        value.key,
        variable_id,
        json_shape_label(&value.value)
    )
}

fn environment_block_hover_contents(
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
) -> String {
    match string_project_field_value(&block.value) {
        Some(value) => format!(
            "### Environment `{}`\n\nVariable: `{}`\n\nDefault value: `{}`",
            block.environment, variable.id, value
        ),
        None => format!(
            "### Environment `{}`\n\nVariable: `{}`",
            block.environment, variable.id
        ),
    }
}

fn variable_rule_hover_contents(
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    rule: &VariableRuleNode,
) -> String {
    format!(
        "### Rule {} for `{}` in `{}`\n\n{}",
        rule.index + 1,
        variable.id,
        block.environment,
        variable_rule_summary(rule)
    )
}

fn variable_rule_summary(rule: &VariableRuleNode) -> String {
    match (
        string_project_field_value(&rule.qualifier),
        string_project_field_value(&rule.value),
    ) {
        (Some(qualifier), Some(value)) => {
            format!("Qualifier `{qualifier}` selects value `{value}`.")
        }
        (Some(qualifier), None) => format!("Qualifier `{qualifier}`."),
        (None, Some(value)) => format!("Selects value `{value}`."),
        (None, None) => "Incomplete rule.".to_owned(),
    }
}

fn type_source_summary(variable: &VariableNode) -> String {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => format!("Type: `{}`", type_name.value),
        TypeSourceNode::Schema(schema) => format!("Schema: `{}`", schema.value),
        TypeSourceNode::Missing { .. } => "Type/schema: missing".to_owned(),
        TypeSourceNode::Conflict { .. } => "Type/schema: both declared".to_owned(),
        TypeSourceNode::Invalid { .. } => "Type/schema: invalid".to_owned(),
    }
}

fn variable_value_keys(variable: &VariableNode) -> Vec<String> {
    variable
        .values
        .inline_keys
        .iter()
        .chain(variable.values.external_keys.iter())
        .map(|value| format!("`{value}`"))
        .collect()
}

fn project_field_string(field: &Option<ProjectField<String>>) -> Option<&str> {
    let Some(ProjectField::Present(value)) = field else {
        return None;
    };
    Some(&value.value)
}

fn json_shape_label(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(number) if number.is_i64() || number.is_u64() => "int",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "list",
        JsonValue::Object(_) => "object",
    }
}
