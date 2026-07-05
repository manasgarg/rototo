use serde_json::Value as JsonValue;
use toml_edit::{ArrayOfTables, InlineTable, Item, Table, Value};

use crate::address::{Address, EntityClass, StepId};
use crate::error::{Result, RototoError};

use super::engine::WorkingTree;
use super::operation::ChangeRecord;
use super::paths::entry_path;
use super::value::{
    json_from_item, json_from_toml_value, set_value_preserving_decor, toml_item_from_json,
    toml_value_from_json,
};

/// A parsed `catalog=<id>:entry=<key>#<pointer>` target.
pub(super) struct EntryTarget {
    catalog: String,
    key: String,
    /// Decoded RFC 6901 segments.
    segments: Vec<String>,
    canonical: String,
}

impl EntryTarget {
    pub(super) fn ownership_addresses(&self) -> Vec<String> {
        vec![
            format!("catalog={}", self.catalog),
            format!("catalog={}:entry={}", self.catalog, self.key),
        ]
    }

    fn path(&self) -> String {
        entry_path(&self.catalog, &self.key)
    }

    fn entity(&self) -> String {
        format!("entry `{}` of catalog `{}`", self.key, self.catalog)
    }
}

pub(super) fn parse_entry_target(target: &str) -> Result<EntryTarget> {
    let address = Address::parse(target)?;
    let steps = address.steps();
    let (catalog, key) = match steps {
        [catalog_step, entry_step]
            if catalog_step.class == EntityClass::Catalog
                && entry_step.class == EntityClass::Entry =>
        {
            match (&catalog_step.id, &entry_step.id) {
                (StepId::Entity(catalog), StepId::Entity(key)) => (catalog.clone(), key.clone()),
                _ => {
                    return Err(RototoError::new(
                        "field operations take a concrete `catalog=<id>:entry=<key>` target",
                    ));
                }
            }
        }
        _ => {
            return Err(RototoError::new(
                "field operations take a `catalog=<id>:entry=<key>#<pointer>` target",
            ));
        }
    };
    let pointer = address.pointer().unwrap_or("");
    if pointer.is_empty() {
        return Err(RototoError::new(
            "field operations need a `#/field` pointer into the entry",
        ));
    }
    let segments: Vec<String> = pointer[1..]
        .split('/')
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .collect();
    if segments.iter().any(String::is_empty) {
        return Err(RototoError::new(format!(
            "pointer `{pointer}` has an empty segment"
        )));
    }
    Ok(EntryTarget {
        catalog,
        key,
        segments,
        canonical: address.to_string(),
    })
}

pub(super) fn set_field(
    work: &mut WorkingTree<'_>,
    target: &EntryTarget,
    value: &JsonValue,
) -> Result<ChangeRecord> {
    let path = target.path();
    let mut document = work.parse_existing(&path, &target.entity())?;
    let (parent, last) = split_target(&target.segments);
    let node = navigate(
        Node::Table(document.as_table_mut()),
        parent,
        true,
        &target.canonical,
    )?;
    let before = assign(node, last, value, &target.canonical)?;
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "set_field",
        address: target.canonical.clone(),
        before,
        after: Some(value.clone()),
    })
}

pub(super) fn unset_field(
    work: &mut WorkingTree<'_>,
    target: &EntryTarget,
) -> Result<ChangeRecord> {
    let path = target.path();
    let mut document = work.parse_existing(&path, &target.entity())?;
    let (parent, last) = split_target(&target.segments);
    let node = navigate(
        Node::Table(document.as_table_mut()),
        parent,
        false,
        &target.canonical,
    )?;
    let before = match node {
        Node::Table(table) => {
            let before = table.get(last).and_then(json_from_item);
            if before.is_none() {
                return Err(field_missing(&target.canonical, last));
            }
            table.remove(last);
            before
        }
        Node::Value(Value::InlineTable(table)) => {
            let before = table.get(last).map(json_from_toml_value);
            if before.is_none() {
                return Err(field_missing(&target.canonical, last));
            }
            table.remove(last);
            before
        }
        _ => {
            return Err(RototoError::new(
                "unset_field removes object fields, not array elements",
            ));
        }
    };
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "unset_field",
        address: target.canonical.clone(),
        before,
        after: None,
    })
}

fn split_target(segments: &[String]) -> (&[String], &str) {
    let (last, parent) = segments.split_last().expect("pointer has segments");
    (parent, last)
}

enum Node<'a> {
    Table(&'a mut Table),
    Tables(&'a mut ArrayOfTables),
    Value(&'a mut Value),
}

fn navigate<'a>(
    node: Node<'a>,
    segments: &[String],
    create: bool,
    target: &str,
) -> Result<Node<'a>> {
    let Some((first, rest)) = segments.split_first() else {
        return Ok(node);
    };
    let child = descend(node, first, create, target)?;
    navigate(child, rest, create, target)
}

fn descend<'a>(node: Node<'a>, segment: &str, create: bool, target: &str) -> Result<Node<'a>> {
    match node {
        Node::Table(table) => {
            if table.get(segment).is_none() {
                if !create {
                    return Err(field_missing(target, segment));
                }
                let mut child = Table::new();
                child.set_implicit(true);
                table.insert(segment, Item::Table(child));
            }
            match table.get_mut(segment).expect("present or inserted above") {
                Item::Table(child) => Ok(Node::Table(child)),
                Item::ArrayOfTables(array) => Ok(Node::Tables(array)),
                Item::Value(value) => Ok(Node::Value(value)),
                Item::None => Err(field_missing(target, segment)),
            }
        }
        Node::Tables(array) => {
            let index = parse_index(segment, array.len(), target)?;
            Ok(Node::Table(array.get_mut(index).expect("index checked")))
        }
        Node::Value(value) => match value {
            Value::InlineTable(table) => {
                if table.get(segment).is_none() {
                    if !create {
                        return Err(field_missing(target, segment));
                    }
                    table.insert(segment, Value::InlineTable(InlineTable::new()));
                }
                Ok(Node::Value(
                    table.get_mut(segment).expect("present or inserted above"),
                ))
            }
            Value::Array(array) => {
                let index = parse_index(segment, array.len(), target)?;
                Ok(Node::Value(array.get_mut(index).expect("index checked")))
            }
            _ => Err(RototoError::new(format!(
                "`{target}` points through a scalar at `{segment}`"
            ))),
        },
    }
}

/// Sets the final segment on its parent, returning the previous value.
fn assign(
    node: Node<'_>,
    last: &str,
    value: &JsonValue,
    target: &str,
) -> Result<Option<JsonValue>> {
    match node {
        Node::Table(table) => {
            let before = table.get(last).and_then(json_from_item);
            enum Existing {
                StandardTable,
                Tables,
                Value,
                Absent,
            }
            let existing = match table.get(last) {
                Some(Item::Table(_)) => Existing::StandardTable,
                Some(Item::ArrayOfTables(_)) => Existing::Tables,
                Some(_) => Existing::Value,
                None => Existing::Absent,
            };
            match existing {
                // A standard table stays a standard table when the new value
                // is an object; anything else becomes a plain value with the
                // old decor carried over.
                Existing::StandardTable if value.is_object() => {
                    table[last] = toml_item_from_json(value)?;
                }
                Existing::Tables => {
                    return Err(RototoError::new(format!(
                        "`{target}` replaces a whole [[{last}]] list; set its fields individually"
                    )));
                }
                Existing::StandardTable | Existing::Value => {
                    set_value_preserving_decor(table, last, toml_value_from_json(value)?);
                }
                Existing::Absent => {
                    table.insert(last, toml_item_from_json(value)?);
                }
            }
            Ok(before)
        }
        Node::Tables(_) => Err(RototoError::new(format!(
            "`{target}` replaces a whole [[table]] element; set its fields individually"
        ))),
        Node::Value(parent) => match parent {
            Value::InlineTable(table) => {
                let before = table.get(last).map(json_from_toml_value);
                let old_decor = table.get(last).map(|old| old.decor().clone());
                let mut new_value = toml_value_from_json(value)?;
                if let Some(decor) = old_decor {
                    if let Some(prefix) = decor.prefix() {
                        new_value.decor_mut().set_prefix(prefix.clone());
                    }
                    if let Some(suffix) = decor.suffix() {
                        new_value.decor_mut().set_suffix(suffix.clone());
                    }
                }
                table.insert(last, new_value);
                Ok(before)
            }
            Value::Array(array) => {
                let index = parse_index(last, array.len(), target)?;
                let slot = array.get_mut(index).expect("index checked");
                let before = json_from_toml_value(slot);
                let mut new_value = toml_value_from_json(value)?;
                if let Some(prefix) = slot.decor().prefix() {
                    new_value.decor_mut().set_prefix(prefix.clone());
                }
                if let Some(suffix) = slot.decor().suffix() {
                    new_value.decor_mut().set_suffix(suffix.clone());
                }
                *slot = new_value;
                Ok(Some(before))
            }
            _ => Err(RototoError::new(format!(
                "`{target}` points through a scalar at `{last}`"
            ))),
        },
    }
}

fn parse_index(segment: &str, count: usize, target: &str) -> Result<usize> {
    let index: usize = segment.parse().map_err(|_| {
        RototoError::new(format!(
            "`{target}` indexes a list with `{segment}`, which is not a number"
        ))
    })?;
    if index >= count {
        return Err(RototoError::new(format!(
            "`{target}` index {index} is out of range (the list has {count} items)"
        )));
    }
    Ok(index)
}

fn field_missing(target: &str, segment: &str) -> RototoError {
    RototoError::new(format!("`{target}` has nothing at `{segment}`"))
}
