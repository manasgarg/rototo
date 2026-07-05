use serde_json::Value as JsonValue;
use toml_edit::{Array, Item, Value};

use crate::error::{Result, RototoError};

use super::engine::WorkingTree;
use super::operation::ChangeRecord;
use super::paths::{checked_id, enum_path};
use super::value::{json_from_toml_value, toml_value_from_json};

pub(super) fn add_member(
    work: &mut WorkingTree<'_>,
    enum_id: &str,
    value: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("enum", enum_id)?;
    checked_member(value)?;
    let new_member = toml_value_from_json(value)?;
    let path = enum_path(enum_id);
    let mut document = work.parse_existing(&path, &format!("enum `{enum_id}`"))?;
    let members = members_mut(&mut document, &path)?;
    let before = members_json(members);
    if members
        .iter()
        .any(|member| json_from_toml_value(member) == *value)
    {
        return Err(RototoError::new(format!(
            "{value} is already a member of enum `{enum_id}`"
        )));
    }
    members.push(new_member);
    let after = members_json(members);
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "add_member",
        address: format!("enum={enum_id}#/members"),
        before: Some(before),
        after: Some(after),
    })
}

pub(super) fn remove_member(
    work: &mut WorkingTree<'_>,
    enum_id: &str,
    value: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("enum", enum_id)?;
    let path = enum_path(enum_id);
    let mut document = work.parse_existing(&path, &format!("enum `{enum_id}`"))?;
    let members = members_mut(&mut document, &path)?;
    let before = members_json(members);
    let Some(index) = members
        .iter()
        .position(|member| json_from_toml_value(member) == *value)
    else {
        return Err(RototoError::new(format!(
            "{value} is not a member of enum `{enum_id}`"
        )));
    };
    // The first element usually has no leading space while its successors
    // do; hand the removed element's prefix to whatever slides into its
    // place so the array keeps its shape.
    let removed_prefix = members
        .get(index)
        .and_then(|member| member.decor().prefix().cloned());
    members.remove(index);
    if index == 0
        && let (Some(prefix), Some(first)) = (removed_prefix, members.get_mut(0))
    {
        first.decor_mut().set_prefix(prefix);
    }
    let after = members_json(members);
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "remove_member",
        address: format!("enum={enum_id}#/members"),
        before: Some(before),
        after: Some(after),
    })
}

fn checked_member(value: &JsonValue) -> Result<()> {
    match value {
        JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => Ok(()),
        _ => Err(RototoError::new(
            "enum members are scalar values (string, int, number, or bool)",
        )),
    }
}

/// The `members` array, created when absent: a file missing it is already
/// broken, and adding the first member is the repair.
fn members_mut<'a>(document: &'a mut toml_edit::DocumentMut, path: &str) -> Result<&'a mut Array> {
    let root = document.as_table_mut();
    if root.get("members").is_none() {
        root.insert("members", Item::Value(Value::Array(Array::new())));
    }
    match root.get_mut("members") {
        Some(Item::Value(Value::Array(array))) => Ok(array),
        _ => Err(RototoError::new(format!(
            "members in {path} is not an array"
        ))),
    }
}

fn members_json(members: &Array) -> JsonValue {
    JsonValue::Array(members.iter().map(json_from_toml_value).collect())
}
