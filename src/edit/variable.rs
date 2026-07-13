use serde_json::Value as JsonValue;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

use crate::error::{Result, RototoError};

use super::engine::WorkingTree;
use super::operation::ChangeRecord;
use super::paths::{checked_id, variable_path};
use super::value::{
    json_from_item, json_from_table, max_table_position, reorder_array_of_tables,
    set_value_preserving_decor, toml_value_from_json,
};

/// Shared by variables and lists: both keep a root-level `description`. The
/// engine resolves the target to a file path and canonical address.
pub(super) fn set_description(
    work: &mut WorkingTree<'_>,
    path: &str,
    canonical: &str,
    text: Option<&str>,
) -> Result<ChangeRecord> {
    let mut document = work.parse_existing(path, canonical)?;
    let root = document.as_table_mut();
    let before = root.get("description").and_then(json_from_item);
    match text {
        Some(text) => set_value_preserving_decor(root, "description", Value::from(text)),
        None => {
            root.remove("description");
        }
    }
    work.write(path.to_owned(), document.to_string());
    Ok(ChangeRecord {
        operation: "set_description",
        address: format!("{canonical}#/description"),
        before,
        after: text.map(|text| JsonValue::String(text.to_owned())),
    })
}

pub(super) fn set_type(
    work: &mut WorkingTree<'_>,
    variable: &str,
    variable_type: &str,
) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    if variable_type.trim().is_empty() {
        return Err(RototoError::new("type must not be empty"));
    }
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let root = document.as_table_mut();
    let before = root.get("type").and_then(json_from_item);
    set_value_preserving_decor(root, "type", Value::from(variable_type));
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "set_type",
        address: format!("variable={variable}#/type"),
        before,
        after: Some(JsonValue::String(variable_type.to_owned())),
    })
}

pub(super) fn set_default(
    work: &mut WorkingTree<'_>,
    variable: &str,
    value: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    let new_value = toml_value_from_json(value)?;
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let resolve = resolve_table_mut(&mut document, &path)?;
    let before = resolve.get("default").and_then(json_from_item);
    set_value_preserving_decor(resolve, "default", new_value);
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "set_default",
        address: format!("variable={variable}#/resolve/default"),
        before,
        after: Some(value.clone()),
    })
}

pub(super) fn add_rule(
    work: &mut WorkingTree<'_>,
    variable: &str,
    position: Option<usize>,
    when: &str,
    value: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    if when.trim().is_empty() {
        return Err(RototoError::new("a rule needs a non-empty when expression"));
    }
    let rule_value = toml_value_from_json(value)?;
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let document_max = max_table_position(&document);

    let mut rule = Table::new();
    rule["when"] = Item::Value(Value::from(when));
    rule["value"] = Item::Value(rule_value);

    let resolve = resolve_table_mut(&mut document, &path)?;
    if resolve.get("rule").is_none() {
        resolve.insert("rule", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let rules = rules_mut(resolve, &path)?;
    let count = rules.len();
    let position = position.unwrap_or(count);
    if position > count {
        return Err(RototoError::new(format!(
            "rule position {position} is out of range (the variable has {count} rules)"
        )));
    }
    let mut tables: Vec<Table> = rules.iter().cloned().collect();
    tables.insert(position, rule);
    reorder_array_of_tables(document_max, rules, tables);

    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "add_rule",
        address: format!("variable={variable}#/resolve/rule/{position}"),
        before: None,
        after: Some(serde_json::json!({ "when": when, "value": value })),
    })
}

pub(super) fn update_rule(
    work: &mut WorkingTree<'_>,
    variable: &str,
    index: usize,
    when: Option<&str>,
    value: Option<&JsonValue>,
) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    if when.is_none() && value.is_none() {
        return Err(RototoError::new(
            "update_rule needs `when`, `value`, or both",
        ));
    }
    if let Some(when) = when
        && when.trim().is_empty()
    {
        return Err(RototoError::new("a rule needs a non-empty when expression"));
    }
    let new_value = value.map(toml_value_from_json).transpose()?;
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let rules = existing_rules_mut(&mut document, variable, &path)?;
    let count = rules.len();
    let rule = rules
        .get_mut(index)
        .ok_or_else(|| rule_out_of_range(index, count))?;
    let before = json_from_table(rule);
    if let Some(when) = when {
        set_value_preserving_decor(rule, "when", Value::from(when));
    }
    if let Some(new_value) = new_value {
        set_value_preserving_decor(rule, "value", new_value);
    }
    let after = json_from_table(rule);
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "update_rule",
        address: format!("variable={variable}#/resolve/rule/{index}"),
        before: Some(before),
        after: Some(after),
    })
}

pub(super) fn remove_rule(
    work: &mut WorkingTree<'_>,
    variable: &str,
    index: usize,
) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let rules = existing_rules_mut(&mut document, variable, &path)?;
    let count = rules.len();
    if index >= count {
        return Err(rule_out_of_range(index, count));
    }
    let before = rules.get(index).map(json_from_table);
    rules.remove(index);
    if rules.is_empty() {
        resolve_table_mut(&mut document, &path)?.remove("rule");
    }
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "remove_rule",
        address: format!("variable={variable}#/resolve/rule/{index}"),
        before,
        after: None,
    })
}

pub(super) fn move_rule(
    work: &mut WorkingTree<'_>,
    variable: &str,
    from: usize,
    to: usize,
) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    if from == to {
        return Err(RototoError::new(format!(
            "move_rule from and to are both {from}; nothing to move"
        )));
    }
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let document_max = max_table_position(&document);
    let rules = existing_rules_mut(&mut document, variable, &path)?;
    let count = rules.len();
    if from >= count {
        return Err(rule_out_of_range(from, count));
    }
    if to >= count {
        return Err(rule_out_of_range(to, count));
    }
    let mut tables: Vec<Table> = rules.iter().cloned().collect();
    let moved = tables.remove(from);
    tables.insert(to, moved);
    reorder_array_of_tables(document_max, rules, tables);
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "move_rule",
        address: format!("variable={variable}#/resolve/rule"),
        before: Some(JsonValue::from(from)),
        after: Some(JsonValue::from(to)),
    })
}

pub(super) fn set_query(
    work: &mut WorkingTree<'_>,
    variable: &str,
    from: &str,
    filter: &str,
    sort: Option<&str>,
    order: Option<&str>,
    limit: Option<i64>,
) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    if from.trim().is_empty() {
        return Err(RototoError::new("a query needs a non-empty from catalog"));
    }
    if filter.trim().is_empty() {
        return Err(RototoError::new(
            "a query needs a non-empty filter expression",
        ));
    }
    if let Some(order) = order
        && order != "asc"
        && order != "desc"
    {
        return Err(RototoError::new(format!(
            "query order is `asc` or `desc`, not `{order}`"
        )));
    }
    if order.is_some() && sort.is_none() {
        return Err(RototoError::new("query order needs a sort expression"));
    }
    if let Some(limit) = limit
        && limit < 1
    {
        return Err(RototoError::new(format!(
            "query limit must be at least 1, not {limit}"
        )));
    }
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let resolve = resolve_table_mut(&mut document, &path)?;
    if resolve.get("method").and_then(|item| item.as_str()) == Some("allocation") {
        return Err(RototoError::new(format!(
            "variable `{variable}` resolves by allocation; end the allocation \
             before switching it to a query"
        )));
    }
    let before = json_from_table(resolve);
    set_value_preserving_decor(resolve, "method", Value::from("query"));
    set_value_preserving_decor(resolve, "from", Value::from(from));
    set_value_preserving_decor(resolve, "filter", Value::from(filter));
    for (key, text) in [("sort", sort), ("order", order)] {
        match text {
            Some(text) => set_value_preserving_decor(resolve, key, Value::from(text)),
            None => {
                resolve.remove(key);
            }
        }
    }
    match limit {
        Some(limit) => set_value_preserving_decor(resolve, "limit", Value::from(limit)),
        None => {
            resolve.remove("limit");
        }
    }
    resolve.remove("rule");
    let after = json_from_table(resolve);
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "set_query",
        address: format!("variable={variable}#/resolve"),
        before: Some(before),
        after: Some(after),
    })
}

pub(super) fn clear_query(work: &mut WorkingTree<'_>, variable: &str) -> Result<ChangeRecord> {
    checked_id("variable", variable)?;
    let path = variable_path(variable);
    let mut document = work.parse_existing(&path, &format!("variable `{variable}`"))?;
    let resolve = resolve_table_mut(&mut document, &path)?;
    if resolve.get("method").and_then(|item| item.as_str()) != Some("query") {
        return Err(RototoError::new(format!(
            "variable `{variable}` does not resolve by query"
        )));
    }
    let before = json_from_table(resolve);
    for key in ["method", "from", "filter", "sort", "order", "limit"] {
        resolve.remove(key);
    }
    let after = json_from_table(resolve);
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "clear_query",
        address: format!("variable={variable}#/resolve"),
        before: Some(before),
        after: Some(after),
    })
}

/// The `[resolve]` table, created when absent: a variable file without one
/// is already broken, and the edit that adds a default or rule is the
/// repair.
fn resolve_table_mut<'a>(document: &'a mut DocumentMut, path: &str) -> Result<&'a mut Table> {
    let root = document.as_table_mut();
    if root.get("resolve").is_none() {
        let mut table = Table::new();
        table.set_implicit(false);
        root.insert("resolve", Item::Table(table));
    }
    root.get_mut("resolve")
        .expect("resolve inserted above")
        .as_table_mut()
        .ok_or_else(|| RototoError::new(format!("[resolve] in {path} is not a table")))
}

fn rules_mut<'a>(resolve: &'a mut Table, path: &str) -> Result<&'a mut ArrayOfTables> {
    resolve
        .get_mut("rule")
        .expect("caller ensures the rule key exists")
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            RototoError::new(format!(
                "resolve.rule in {path} is not an array of [[resolve.rule]] tables"
            ))
        })
}

fn existing_rules_mut<'a>(
    document: &'a mut DocumentMut,
    variable: &str,
    path: &str,
) -> Result<&'a mut ArrayOfTables> {
    let resolve = resolve_table_mut(document, path)?;
    if resolve.get("rule").is_none() {
        return Err(RototoError::new(format!(
            "variable `{variable}` has no rules"
        )));
    }
    rules_mut(resolve, path)
}

fn rule_out_of_range(index: usize, count: usize) -> RototoError {
    RototoError::new(format!(
        "rule index {index} is out of range (the variable has {count} rules)"
    ))
}
