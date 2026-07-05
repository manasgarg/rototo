use serde_json::Value as JsonValue;
use toml_edit::{ArrayOfTables, Item, Table, Value};

use crate::error::{Result, RototoError};

use super::engine::WorkingTree;
use super::operation::{AllocationArmInput, ChangeRecord};
use super::paths::{checked_id, layer_path};
use super::value::{
    json_from_item, json_from_table, max_table_position, set_value_preserving_decor,
};

pub(super) fn add_allocation(
    work: &mut WorkingTree<'_>,
    layer_id: &str,
    id: &str,
    status: Option<&str>,
    eligibility: Option<&str>,
    arms: &[AllocationArmInput],
) -> Result<ChangeRecord> {
    checked_id("layer", layer_id)?;
    checked_id("allocation", id)?;
    if arms.is_empty() {
        return Err(RototoError::new(
            "an allocation needs at least one arm with a bucket range",
        ));
    }
    for arm in arms {
        if arm.name.trim().is_empty() {
            return Err(RototoError::new("an arm needs a non-empty name"));
        }
        checked_buckets(&arm.buckets)?;
    }

    let path = layer_path(layer_id);
    let mut document = work.parse_existing(&path, &format!("layer `{layer_id}`"))?;
    let document_max = max_table_position(&document);

    let mut allocation = Table::new();
    allocation.set_position(document_max + 1);
    allocation["id"] = Item::Value(Value::from(id));
    if let Some(status) = status {
        allocation["status"] = Item::Value(Value::from(status));
    }
    if let Some(eligibility) = eligibility {
        allocation["eligibility"] = Item::Value(Value::from(eligibility));
    }
    let mut arm_tables = ArrayOfTables::new();
    for (index, arm) in arms.iter().enumerate() {
        let mut table = Table::new();
        table.set_position(document_max + 2 + index);
        table["name"] = Item::Value(Value::from(arm.name.as_str()));
        table["buckets"] = Item::Value(Value::from(arm.buckets.as_str()));
        arm_tables.push(table);
    }
    allocation["arm"] = Item::ArrayOfTables(arm_tables);

    let root = document.as_table_mut();
    if root.get("allocation").is_none() {
        root.insert("allocation", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let allocations = allocations_mut(root, &path)?;
    if allocation_index(allocations, id).is_some() {
        return Err(RototoError::new(format!(
            "allocation `{id}` already exists in layer `{layer_id}`"
        )));
    }
    let record_index = allocations.len();
    let after = json_from_table(&allocation);
    allocations.push(allocation);

    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "add_allocation",
        address: format!("layer={layer_id}#/allocation/{record_index}"),
        before: None,
        after: Some(after),
    })
}

pub(super) fn remove_allocation(
    work: &mut WorkingTree<'_>,
    layer_id: &str,
    id: &str,
) -> Result<ChangeRecord> {
    checked_id("layer", layer_id)?;
    let path = layer_path(layer_id);
    let mut document = work.parse_existing(&path, &format!("layer `{layer_id}`"))?;
    let root = document.as_table_mut();
    let allocations = existing_allocations_mut(root, layer_id, &path)?;
    let Some(index) = allocation_index(allocations, id) else {
        return Err(no_such_allocation(layer_id, id));
    };
    let before = allocations.get(index).map(json_from_table);
    allocations.remove(index);
    if allocations.is_empty() {
        root.remove("allocation");
    }
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "remove_allocation",
        address: format!("layer={layer_id}#/allocation/{index}"),
        before,
        after: None,
    })
}

pub(super) fn set_allocation_status(
    work: &mut WorkingTree<'_>,
    layer_id: &str,
    id: &str,
    status: &str,
) -> Result<ChangeRecord> {
    checked_id("layer", layer_id)?;
    if status.trim().is_empty() {
        return Err(RototoError::new("status must not be empty"));
    }
    let path = layer_path(layer_id);
    let mut document = work.parse_existing(&path, &format!("layer `{layer_id}`"))?;
    let allocations = existing_allocations_mut(document.as_table_mut(), layer_id, &path)?;
    let Some(index) = allocation_index(allocations, id) else {
        return Err(no_such_allocation(layer_id, id));
    };
    let allocation = allocations.get_mut(index).expect("index found above");
    let before = allocation.get("status").and_then(json_from_item);
    set_value_preserving_decor(allocation, "status", Value::from(status));
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "set_allocation_status",
        address: format!("layer={layer_id}#/allocation/{index}/status"),
        before,
        after: Some(JsonValue::String(status.to_owned())),
    })
}

pub(super) fn set_allocation_eligibility(
    work: &mut WorkingTree<'_>,
    layer_id: &str,
    id: &str,
    when: Option<&str>,
) -> Result<ChangeRecord> {
    checked_id("layer", layer_id)?;
    if let Some(when) = when
        && when.trim().is_empty()
    {
        return Err(RototoError::new(
            "eligibility needs a non-empty expression; omit `when` to clear it",
        ));
    }
    let path = layer_path(layer_id);
    let mut document = work.parse_existing(&path, &format!("layer `{layer_id}`"))?;
    let allocations = existing_allocations_mut(document.as_table_mut(), layer_id, &path)?;
    let Some(index) = allocation_index(allocations, id) else {
        return Err(no_such_allocation(layer_id, id));
    };
    let allocation = allocations.get_mut(index).expect("index found above");
    let before = allocation.get("eligibility").and_then(json_from_item);
    match when {
        Some(when) => set_value_preserving_decor(allocation, "eligibility", Value::from(when)),
        None => {
            allocation.remove("eligibility");
        }
    }
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "set_allocation_eligibility",
        address: format!("layer={layer_id}#/allocation/{index}/eligibility"),
        before,
        after: when.map(|when| JsonValue::String(when.to_owned())),
    })
}

pub(super) fn set_arm_buckets(
    work: &mut WorkingTree<'_>,
    layer_id: &str,
    allocation_id: &str,
    arm: &str,
    buckets: &str,
) -> Result<ChangeRecord> {
    checked_id("layer", layer_id)?;
    checked_buckets(buckets)?;
    let path = layer_path(layer_id);
    let mut document = work.parse_existing(&path, &format!("layer `{layer_id}`"))?;
    let allocations = existing_allocations_mut(document.as_table_mut(), layer_id, &path)?;
    let Some(index) = allocation_index(allocations, allocation_id) else {
        return Err(no_such_allocation(layer_id, allocation_id));
    };
    let allocation = allocations.get_mut(index).expect("index found above");
    let arms = match allocation.get_mut("arm") {
        Some(Item::ArrayOfTables(arms)) => arms,
        Some(_) => {
            return Err(RototoError::new(format!(
                "arm in allocation `{allocation_id}` is not an array of [[allocation.arm]] tables"
            )));
        }
        None => {
            return Err(RototoError::new(format!(
                "allocation `{allocation_id}` has no arms"
            )));
        }
    };
    let Some(arm_index) = arms
        .iter()
        .position(|table| table.get("name").and_then(Item::as_str) == Some(arm))
    else {
        return Err(RototoError::new(format!(
            "allocation `{allocation_id}` has no arm named `{arm}`"
        )));
    };
    let arm_table = arms.get_mut(arm_index).expect("index found above");
    let before = arm_table.get("buckets").and_then(json_from_item);
    set_value_preserving_decor(arm_table, "buckets", Value::from(buckets));
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "set_arm_buckets",
        address: format!("layer={layer_id}#/allocation/{index}/arm/{arm_index}/buckets"),
        before,
        after: Some(JsonValue::String(buckets.to_owned())),
    })
}

fn checked_buckets(buckets: &str) -> Result<()> {
    if buckets.trim().is_empty() {
        return Err(RototoError::new(
            "buckets must be a non-empty range like \"0-499\"",
        ));
    }
    Ok(())
}

fn allocations_mut<'a>(root: &'a mut Table, path: &str) -> Result<&'a mut ArrayOfTables> {
    root.get_mut("allocation")
        .expect("caller ensures the allocation key exists")
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            RototoError::new(format!(
                "allocation in {path} is not an array of [[allocation]] tables"
            ))
        })
}

fn existing_allocations_mut<'a>(
    root: &'a mut Table,
    layer_id: &str,
    path: &str,
) -> Result<&'a mut ArrayOfTables> {
    if root.get("allocation").is_none() {
        return Err(RototoError::new(format!(
            "layer `{layer_id}` has no allocations"
        )));
    }
    allocations_mut(root, path)
}

fn allocation_index(allocations: &ArrayOfTables, id: &str) -> Option<usize> {
    allocations
        .iter()
        .position(|table| table.get("id").and_then(Item::as_str) == Some(id))
}

fn no_such_allocation(layer_id: &str, id: &str) -> RototoError {
    RototoError::new(format!("layer `{layer_id}` has no allocation `{id}`"))
}
