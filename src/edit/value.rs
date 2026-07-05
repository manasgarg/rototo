use serde_json::Value as JsonValue;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::error::{Result, RototoError};

/// Converts a JSON value into a TOML value. TOML has no null, so null is a
/// structural refusal; clearing a field is its own operation.
pub(super) fn toml_value_from_json(value: &JsonValue) -> Result<Value> {
    match value {
        JsonValue::Null => Err(RototoError::new(
            "TOML has no null; use the operation that clears the field instead",
        )),
        JsonValue::Bool(flag) => Ok(Value::from(*flag)),
        JsonValue::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(Value::from(integer))
            } else if let Some(float) = number.as_f64() {
                Ok(Value::from(float))
            } else {
                Err(RototoError::new(format!(
                    "number {number} does not fit a TOML integer or float"
                )))
            }
        }
        JsonValue::String(text) => Ok(Value::from(text.as_str())),
        JsonValue::Array(items) => {
            let mut array = Array::new();
            for item in items {
                array.push(toml_value_from_json(item)?);
            }
            Ok(Value::Array(array))
        }
        JsonValue::Object(map) => {
            let mut table = InlineTable::new();
            for (key, item) in map {
                table.insert(key, toml_value_from_json(item)?);
            }
            Ok(Value::InlineTable(table))
        }
    }
}

/// Converts a JSON value into a TOML item, using standard tables for
/// objects: the shape entry files use for nested fields.
pub(super) fn toml_item_from_json(value: &JsonValue) -> Result<Item> {
    match value {
        JsonValue::Object(map) => {
            let mut table = Table::new();
            table.set_implicit(false);
            for (key, item) in map {
                table.insert(key, toml_item_from_json(item)?);
            }
            Ok(Item::Table(table))
        }
        other => Ok(Item::Value(toml_value_from_json(other)?)),
    }
}

pub(super) fn json_from_toml_value(value: &Value) -> JsonValue {
    match value {
        Value::String(text) => JsonValue::String(text.value().clone()),
        Value::Integer(integer) => JsonValue::from(*integer.value()),
        Value::Float(float) => serde_json::Number::from_f64(*float.value())
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Boolean(flag) => JsonValue::Bool(*flag.value()),
        Value::Datetime(datetime) => JsonValue::String(datetime.value().to_string()),
        Value::Array(array) => JsonValue::Array(array.iter().map(json_from_toml_value).collect()),
        Value::InlineTable(table) => JsonValue::Object(
            table
                .iter()
                .map(|(key, value)| (key.to_owned(), json_from_toml_value(value)))
                .collect(),
        ),
    }
}

pub(super) fn json_from_item(item: &Item) -> Option<JsonValue> {
    match item {
        Item::None => None,
        Item::Value(value) => Some(json_from_toml_value(value)),
        Item::Table(table) => Some(json_from_table(table)),
        Item::ArrayOfTables(array) => Some(JsonValue::Array(
            array.iter().map(json_from_table).collect(),
        )),
    }
}

pub(super) fn json_from_table(table: &Table) -> JsonValue {
    JsonValue::Object(
        table
            .iter()
            .filter_map(|(key, item)| json_from_item(item).map(|value| (key.to_owned(), value)))
            .collect(),
    )
}

/// Replaces `table[key]` with a new value. When the key already existed as a
/// value, its surrounding decor (the space after `=`, a trailing comment)
/// carries over to the replacement.
pub(super) fn set_value_preserving_decor(table: &mut Table, key: &str, new_value: Value) {
    let old_decor = table
        .get(key)
        .and_then(Item::as_value)
        .map(|value| value.decor().clone());
    table[key] = Item::Value(new_value);
    if let (Some(decor), Some(value)) = (old_decor, table[key].as_value_mut()) {
        if let Some(prefix) = decor.prefix() {
            value.decor_mut().set_prefix(prefix.clone());
        }
        if let Some(suffix) = decor.suffix() {
            value.decor_mut().set_suffix(suffix.clone());
        }
    }
}

/// Rewrites an array of tables into a new order. The serializer follows each
/// table's document position, not the array's order, so the slots' original
/// positions are reassigned across the new sequence; tables beyond the
/// original count (net-new ones) get positions past everything else in the
/// document.
pub(super) fn reorder_array_of_tables(
    document_max_position: usize,
    array: &mut ArrayOfTables,
    new_order: Vec<Table>,
) {
    let mut positions: Vec<usize> = array.iter().filter_map(Table::position).collect();
    positions.sort_unstable();
    array.clear();
    for (index, mut table) in new_order.into_iter().enumerate() {
        match positions.get(index) {
            Some(position) => table.set_position(*position),
            None => table.set_position(document_max_position + 1 + index - positions.len()),
        }
        array.push(table);
    }
}

/// The highest table position anywhere in the document, so net-new tables
/// can be placed after everything that exists.
pub(super) fn max_table_position(document: &DocumentMut) -> usize {
    fn walk(table: &Table, max: &mut usize) {
        if let Some(position) = table.position() {
            *max = (*max).max(position);
        }
        for (_, item) in table.iter() {
            match item {
                Item::Table(child) => walk(child, max),
                Item::ArrayOfTables(array) => {
                    for child in array.iter() {
                        walk(child, max);
                    }
                }
                _ => {}
            }
        }
    }
    let mut max = 0;
    walk(document.as_table(), &mut max);
    max
}

pub(super) fn parse_toml_document(path: &str, content: &str) -> Result<DocumentMut> {
    content
        .parse::<DocumentMut>()
        .map_err(|err| RototoError::new(format!("{path} does not parse as TOML: {err}")))
}
