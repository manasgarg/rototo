use serde_json::Value as JsonValue;

use crate::diagnostics::{DocId, LintStage};

use super::super::engine::{LintContext, variable_values};
use super::super::nodes::*;
use super::super::project::json_from_toml_value;
use super::{RegisteredLintEntity, RegisteredLintField, SchemaLintField, ValueLintField};

pub(super) fn expanded_variable_toml_json(ctx: &LintContext, variable: &VariableNode) -> JsonValue {
    let mut toml = ctx
        .syntax
        .toml
        .get(&variable.doc)
        .map(|parsed| json_from_toml_value(&parsed.plain))
        .unwrap_or_else(|| JsonValue::Object(serde_json::Map::new()));
    let mut values = serde_json::Map::new();
    for value in variable_values(ctx, variable) {
        values.insert(value.key.clone(), value.value.clone());
    }

    if let JsonValue::Object(object) = &mut toml {
        object.insert("values".to_owned(), JsonValue::Object(values));
    }
    toml
}

pub(super) fn parsed_toml_json(ctx: &LintContext, doc: DocId) -> JsonValue {
    ctx.syntax
        .toml
        .get(&doc)
        .map(|parsed| json_from_toml_value(&parsed.plain))
        .unwrap_or(JsonValue::Null)
}

pub(super) fn selected_value_field(
    value: &JsonValue,
    field: Option<&RegisteredLintField>,
) -> JsonValue {
    match field {
        Some(RegisteredLintField::Value(ValueLintField::JsonPath(path))) => {
            json_value_at_path(value, path)
                .cloned()
                .unwrap_or(JsonValue::Null)
        }
        _ => value.clone(),
    }
}

pub(super) fn selected_schema_field(
    schema: &JsonValue,
    field: Option<&RegisteredLintField>,
) -> JsonValue {
    match field {
        Some(RegisteredLintField::Schema(SchemaLintField::JsonPath(path))) => {
            json_value_at_path(schema, path)
                .cloned()
                .unwrap_or(JsonValue::Null)
        }
        _ => schema.clone(),
    }
}

fn json_value_at_path<'a>(value: &'a JsonValue, path: &[String]) -> Option<&'a JsonValue> {
    let mut current = value;
    for segment in path {
        current = current.get(segment)?;
    }
    Some(current)
}

pub(super) fn lint_stage_label(stage: LintStage) -> &'static str {
    match stage {
        LintStage::Discover => "discover",
        LintStage::Parse => "parse",
        LintStage::Project => "project",
        LintStage::Register => "register",
        LintStage::Reference => "reference",
        LintStage::Value => "value",
        LintStage::Graph => "graph",
        LintStage::Policy => "policy",
    }
}

pub(super) fn registered_lint_entity_label(entity: RegisteredLintEntity) -> &'static str {
    match entity {
        RegisteredLintEntity::Workspace => "workspace",
        RegisteredLintEntity::Qualifier => "qualifier",
        RegisteredLintEntity::Variable => "variable",
        RegisteredLintEntity::Value => "value",
        RegisteredLintEntity::Schema => "schema",
    }
}
