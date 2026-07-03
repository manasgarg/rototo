use serde_json::Value as JsonValue;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, DiagnosticRule, LintDiagnostic,
    SourcePosition,
};

use super::super::PackageLintSnapshot;
use super::super::index::*;
use super::PackageHover;
use super::common::{
    expression_project_field_label, json_project_field_label, location_contains_position,
    source_range_size,
};

pub(crate) fn hover(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<PackageHover> {
    let mut candidates = Vec::new();
    push_diagnostic_hover_candidates(snapshot, path, position, &mut candidates);
    push_manifest_hover_candidates(&snapshot.index, path, position, &mut candidates);
    push_variable_hover_candidates(&snapshot.index, path, position, &mut candidates);
    push_catalog_hover_candidates(&snapshot.index, path, position, &mut candidates);
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
    hover: PackageHover,
}

fn push_diagnostic_hover_candidates(
    snapshot: &PackageLintSnapshot,
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
    _index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    let _ = (path, position, candidates);
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

        if let ResolveNode::Resolve {
            location,
            default,
            rules,
        } = &variable.resolve
        {
            push_hover_candidate(
                candidates,
                path,
                position,
                &default.location(),
                3,
                variable_resolve_hover_contents(variable, default),
            );
            push_hover_candidate(
                candidates,
                path,
                position,
                location,
                4,
                variable_resolve_hover_contents(variable, default),
            );
            if let RuleCollection::Rules(rules) = rules {
                for rule in rules {
                    push_hover_candidate(
                        candidates,
                        path,
                        position,
                        &rule.location,
                        3,
                        variable_rule_hover_contents(variable, rule),
                    );
                    for location in [
                        rule.when.as_ref().map(ProjectField::location),
                        rule.query.as_ref().map(ProjectField::location),
                        Some(rule.value.location()),
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
                            variable_rule_hover_contents(variable, rule),
                        );
                    }
                }
            }
        }
    }
}

fn push_catalog_hover_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    for catalog in index.catalogs.values() {
        if catalog.location.path != path {
            continue;
        }
        push_hover_candidate(
            candidates,
            path,
            position,
            &catalog.location,
            2,
            catalog_hover_contents(catalog),
        );
    }

    for entries in index.catalog_entries.values() {
        for entry in entries.values() {
            push_hover_candidate(
                candidates,
                path,
                position,
                &entry.location,
                2,
                catalog_entry_hover_contents(entry),
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
        hover: PackageHover {
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

fn file_hover(index: &SemanticIndex, path: &str) -> Option<PackageHover> {
    index
        .variables
        .values()
        .find(|variable| variable.location.path == path)
        .map(|variable| PackageHover {
            contents: variable_hover_contents(variable),
            location: variable.location.clone(),
        })
        .or_else(|| {
            index
                .catalogs
                .values()
                .find(|catalog| catalog.location.path == path)
                .map(|catalog| PackageHover {
                    contents: catalog_hover_contents(catalog),
                    location: catalog.location.clone(),
                })
        })
        .or_else(|| {
            index.catalog_entries.values().find_map(|entries| {
                entries
                    .values()
                    .find(|entry| entry.location.path == path)
                    .map(|entry| PackageHover {
                        contents: catalog_entry_hover_contents(entry),
                        location: entry.location.clone(),
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
            .unwrap_or_else(|| (rule.as_str().to_owned(), "Package custom lint.".to_owned())),
    }
}

fn custom_rule_definition(
    index: &SemanticIndex,
    rule: &CustomRuleId,
) -> Option<CustomRuleDefinition> {
    index
        .custom_lints
        .rules
        .get(rule)
        .map(|rule| rule.definition.clone())
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

fn catalog_hover_contents(catalog: &CatalogNode) -> String {
    let mut contents = format!("### Catalog `{}`", catalog.id);
    if let Some(description) = catalog
        .json
        .as_ref()
        .and_then(|json| json.get("description"))
        .and_then(JsonValue::as_str)
    {
        contents.push_str("\n\n");
        contents.push_str(description);
    }
    contents
}

fn catalog_entry_hover_contents(entry: &CatalogEntryNode) -> String {
    format!(
        "### Catalog value `{}`\n\nCatalog: `{}`\n\nJSON shape: `{}`",
        entry.key,
        entry.catalog_id,
        json_shape_label(&entry.value)
    )
}

fn variable_resolve_hover_contents(
    variable: &VariableNode,
    default: &ProjectField<JsonValue>,
) -> String {
    match json_project_field_label(default) {
        Some(value) => format!(
            "### Resolve for `{}`\n\nDefault value: `{}`",
            variable.id, value
        ),
        None => format!("### Resolve for `{}`", variable.id),
    }
}

fn variable_rule_hover_contents(variable: &VariableNode, rule: &VariableRuleNode) -> String {
    format!(
        "### Rule {} for `{}`\n\n{}",
        rule.index + 1,
        variable.id,
        variable_rule_summary(rule)
    )
}

fn variable_rule_summary(rule: &VariableRuleNode) -> String {
    let selector = expression_project_field_label(&rule.when)
        .or_else(|| expression_project_field_label(&rule.query));
    match (selector, json_project_field_label(&rule.value)) {
        (Some(condition), Some(value)) => {
            format!("Condition `{condition}` selects value `{value}`.")
        }
        (Some(condition), None) => format!("Condition `{condition}`."),
        (None, Some(value)) => format!("Selects value `{value}`."),
        (None, None) => "Incomplete rule.".to_owned(),
    }
}

fn type_source_summary(variable: &VariableNode) -> String {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => format!("Type: `{}`", type_name.value),
        TypeSourceNode::Catalog(catalog) => format!("Catalog type: `{}`", catalog.value),
        TypeSourceNode::Schema(schema) => format!("Schema: `{}`", schema.value),
        TypeSourceNode::Missing { .. } => "Type: missing".to_owned(),
        TypeSourceNode::Conflict { .. } => "Type: conflicting declarations".to_owned(),
        TypeSourceNode::Invalid { .. } => "Type: invalid".to_owned(),
    }
}

fn variable_value_keys(variable: &VariableNode) -> Vec<String> {
    variable
        .values
        .inline_values
        .keys()
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
