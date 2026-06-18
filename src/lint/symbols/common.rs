use crate::diagnostics::{DiagnosticLocation, SourcePosition, SourceRange};

use super::super::index::*;

pub(super) fn string_project_field_value(field: &ProjectField<String>) -> Option<&str> {
    match field {
        ProjectField::Present(value) => Some(&value.value),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

pub(super) fn json_project_field_label(field: &ProjectField<serde_json::Value>) -> Option<String> {
    match field {
        ProjectField::Present(value) => Some(value.value.to_string()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

pub(super) fn predicate_op_project_field_value(field: &ProjectField<PredicateOp>) -> Option<&str> {
    match field {
        ProjectField::Present(value) => Some(value.value.as_str()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

pub(super) fn location_contains_position(
    location: &DiagnosticLocation,
    path: &str,
    position: SourcePosition,
) -> bool {
    location.path == path
        && location
            .range
            .is_some_and(|range| source_range_contains_position(range, position))
}

fn source_range_contains_position(range: SourceRange, position: SourcePosition) -> bool {
    source_position_le(range.start, position) && source_position_lt(position, range.end)
}

fn source_position_le(left: SourcePosition, right: SourcePosition) -> bool {
    (left.line, left.character) <= (right.line, right.character)
}

fn source_position_lt(left: SourcePosition, right: SourcePosition) -> bool {
    (left.line, left.character) < (right.line, right.character)
}

pub(super) fn source_range_size(range: SourceRange) -> usize {
    range
        .end
        .line
        .saturating_sub(range.start.line)
        .saturating_mul(10_000)
        .saturating_add(range.end.character.saturating_sub(range.start.character))
}

pub(super) fn variable_value_definition_location(
    index: &SemanticIndex,
    variable: &VariableNode,
    value: &serde_json::Value,
) -> Option<DiagnosticLocation> {
    if let TypeSourceNode::Catalog(catalog) = &variable.type_source {
        let value = value.as_str()?;
        return index
            .catalog_entries
            .get(&catalog.value)
            .and_then(|entries| entries.get(value))
            .map(|entry| entry.location.clone());
    }
    None
}
