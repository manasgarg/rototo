use serde_json::Value as JsonValue;

use crate::diagnostics::DocId;

use super::super::engine::{LintContext, variable_values};
use super::super::index::*;
use super::super::project::json_from_toml_value;

pub(super) fn expanded_variable_toml_json(ctx: &LintContext, variable: &VariableNode) -> JsonValue {
    let mut toml = ctx
        .syntax
        .toml
        .get(&variable.doc)
        .map(|parsed| json_from_toml_value(parsed.root()))
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
        .map(|parsed| json_from_toml_value(parsed.root()))
        .unwrap_or(JsonValue::Null)
}
