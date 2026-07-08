use serde_json::Value as JsonValue;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::address::{Address, EntityClass, StepId};
use crate::error::{Result, RototoError};

use super::engine::WorkingTree;
use super::operation::ChangeRecord;
use super::paths::{
    catalog_data_dir, catalog_schema_path, checked_id, context_schema_path, entry_path, layer_path,
    list_path, sample_path, samples_dir, variable_path,
};
use super::value::{json_from_table, toml_item_from_json, toml_value_from_json};

pub(super) fn create_variable(
    work: &mut WorkingTree<'_>,
    id: &str,
    variable_type: &str,
    description: Option<&str>,
    default: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("variable", id)?;
    if variable_type.trim().is_empty() {
        return Err(RototoError::new("type must not be empty"));
    }
    let default_value = toml_value_from_json(default)?;
    let path = variable_path(id);
    ensure_absent(work, &path, &format!("variable `{id}`"))?;

    let mut document = DocumentMut::new();
    document["schema_version"] = Item::Value(Value::from(1));
    if let Some(description) = description {
        document["description"] = Item::Value(Value::from(description));
        blank_line_before(document.as_table_mut(), "description");
    }
    document["type"] = Item::Value(Value::from(variable_type));
    if description.is_none() {
        blank_line_before(document.as_table_mut(), "type");
    }

    let mut resolve = Table::new();
    resolve.set_implicit(false);
    resolve.insert("default", Item::Value(default_value));
    comment_before(
        &mut resolve,
        "default",
        "# The value when no rule matches. Rules run top to bottom; the first\n# match wins.\n",
    );
    document.insert("resolve", Item::Table(resolve));

    let after = json_from_table(document.as_table());
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "create_variable",
        address: format!("variable={id}"),
        before: None,
        after: Some(after),
    })
}

pub(super) fn create_catalog(
    work: &mut WorkingTree<'_>,
    id: &str,
    schema: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("catalog", id)?;
    checked_schema(schema)?;
    let path = catalog_schema_path(id);
    ensure_absent(work, &path, &format!("catalog `{id}`"))?;
    work.write(path, pretty_json(schema)?);
    Ok(ChangeRecord {
        operation: "create_catalog",
        address: format!("catalog={id}"),
        before: None,
        after: Some(schema.clone()),
    })
}

pub(super) fn create_entry(
    work: &mut WorkingTree<'_>,
    catalog: &str,
    key: &str,
    fields: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("catalog", catalog)?;
    checked_id("entry", key)?;
    if !work.exists(&catalog_schema_path(catalog)) {
        return Err(RototoError::new(format!(
            "catalog `{catalog}` does not exist ({} not found)",
            catalog_schema_path(catalog)
        )));
    }
    let JsonValue::Object(fields_map) = fields else {
        return Err(RototoError::new("entry fields must be a JSON object"));
    };
    let path = entry_path(catalog, key);
    ensure_absent(
        work,
        &path,
        &format!("entry `{key}` of catalog `{catalog}`"),
    )?;

    let mut document = DocumentMut::new();
    for (field, value) in fields_map {
        document.insert(field, toml_item_from_json(value)?);
    }
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "create_entry",
        address: format!("catalog={catalog}:entry={key}"),
        before: None,
        after: Some(fields.clone()),
    })
}

pub(super) fn create_list(
    work: &mut WorkingTree<'_>,
    id: &str,
    member_type: &str,
    members: &[JsonValue],
    description: Option<&str>,
) -> Result<ChangeRecord> {
    checked_id("list", id)?;
    if member_type.trim().is_empty() {
        return Err(RototoError::new("type must not be empty"));
    }
    if members.is_empty() {
        return Err(RototoError::new("a list needs at least one member"));
    }
    let mut member_values = toml_edit::Array::new();
    for (index, member) in members.iter().enumerate() {
        if !matches!(
            member,
            JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_)
        ) {
            return Err(RototoError::new(
                "list members are scalar values (string, int, number, or bool)",
            ));
        }
        if members[..index].contains(member) {
            return Err(RototoError::new(format!(
                "member {member} appears more than once"
            )));
        }
        member_values.push(toml_value_from_json(member)?);
    }
    let path = list_path(id);
    ensure_absent(work, &path, &format!("list `{id}`"))?;

    let mut document = DocumentMut::new();
    document["schema_version"] = Item::Value(Value::from(1));
    if let Some(description) = description {
        document["description"] = Item::Value(Value::from(description));
    }
    document["type"] = Item::Value(Value::from(member_type));
    document["members"] = Item::Value(Value::Array(member_values));
    blank_line_before(document.as_table_mut(), "members");

    let after = json_from_table(document.as_table());
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "create_list",
        address: format!("list={id}"),
        before: None,
        after: Some(after),
    })
}

/// Creates the evaluation context schema plus an empty starter sample, so
/// the samples directory exists as a place to grow coverage from day one.
pub(super) fn create_context(
    work: &mut WorkingTree<'_>,
    id: &str,
    schema: &JsonValue,
) -> Result<Vec<ChangeRecord>> {
    checked_id("evaluation context", id)?;
    checked_schema(schema)?;
    let path = context_schema_path(id);
    ensure_absent(work, &path, &format!("evaluation context `{id}`"))?;
    work.write(path, pretty_json(schema)?);
    let sample = sample_path(id, "default");
    let mut records = vec![ChangeRecord {
        operation: "create_context",
        address: format!("evaluation-context={id}"),
        before: None,
        after: Some(schema.clone()),
    }];
    if !work.exists(&sample) {
        let starter = JsonValue::Object(serde_json::Map::new());
        work.write(sample, pretty_json(&starter)?);
        records.push(ChangeRecord {
            operation: "create_context",
            address: format!("evaluation-context={id}:sample=default"),
            before: None,
            after: Some(starter),
        });
    }
    Ok(records)
}

pub(super) fn create_layer(
    work: &mut WorkingTree<'_>,
    id: &str,
    unit: &str,
    buckets: i64,
) -> Result<ChangeRecord> {
    checked_id("layer", id)?;
    if unit.trim().is_empty() {
        return Err(RototoError::new(
            "a layer needs a unit expression, like `context.user.id`",
        ));
    }
    if buckets <= 0 {
        return Err(RototoError::new("buckets must be a positive count"));
    }
    let path = layer_path(id);
    ensure_absent(work, &path, &format!("layer `{id}`"))?;

    let mut document = DocumentMut::new();
    document["schema_version"] = Item::Value(Value::from(1));
    document["unit"] = Item::Value(Value::from(unit));
    blank_line_before(document.as_table_mut(), "unit");
    document["buckets"] = Item::Value(Value::from(buckets));

    let after = json_from_table(document.as_table());
    work.write(path, document.to_string());
    Ok(ChangeRecord {
        operation: "create_layer",
        address: format!("layer={id}"),
        before: None,
        after: Some(after),
    })
}

pub(super) fn create_sample(
    work: &mut WorkingTree<'_>,
    context: &str,
    key: &str,
    content: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("evaluation context", context)?;
    checked_id("sample", key)?;
    if !work.exists(&context_schema_path(context)) {
        return Err(RototoError::new(format!(
            "evaluation context `{context}` does not exist ({} not found)",
            context_schema_path(context)
        )));
    }
    if !content.is_object() {
        return Err(RototoError::new(
            "a sample is a JSON object of context facts",
        ));
    }
    let path = sample_path(context, key);
    ensure_absent(
        work,
        &path,
        &format!("sample `{key}` of evaluation context `{context}`"),
    )?;
    work.write(path, pretty_json(content)?);
    Ok(ChangeRecord {
        operation: "create_sample",
        address: format!("evaluation-context={context}:sample={key}"),
        before: None,
        after: Some(content.clone()),
    })
}

pub(super) fn replace_sample(
    work: &mut WorkingTree<'_>,
    context: &str,
    key: &str,
    content: &JsonValue,
) -> Result<ChangeRecord> {
    checked_id("evaluation context", context)?;
    checked_id("sample", key)?;
    if !content.is_object() {
        return Err(RototoError::new(
            "a sample is a JSON object of context facts",
        ));
    }
    let path = sample_path(context, key);
    let before = match work.content(&path) {
        Some(existing) => serde_json::from_str::<JsonValue>(existing).ok(),
        None => {
            return Err(RototoError::new(format!(
                "sample `{key}` of evaluation context `{context}` does not exist ({path} not found)"
            )));
        }
    };
    work.write(path, pretty_json(content)?);
    Ok(ChangeRecord {
        operation: "replace_sample",
        address: format!("evaluation-context={context}:sample={key}"),
        before,
        after: Some(content.clone()),
    })
}

pub(super) fn delete(work: &mut WorkingTree<'_>, address: &Address) -> Result<Vec<ChangeRecord>> {
    if address.pointer().is_some() {
        return Err(RototoError::new(
            "delete takes an entity, not a field; use unset_field for fields",
        ));
    }
    let steps = address.steps();
    let entity_id = |step: &crate::address::Step| -> Result<String> {
        match &step.id {
            StepId::Entity(id) => Ok(id.clone()),
            _ => Err(RototoError::new(
                "delete takes a concrete entity, not a collective or subtree",
            )),
        }
    };
    match steps {
        [step] if step.class == EntityClass::Variable => {
            let id = entity_id(step)?;
            Ok(vec![delete_toml_document(
                work,
                &variable_path(&id),
                format!("variable={id}"),
            )?])
        }
        [step] if step.class == EntityClass::List => {
            let id = entity_id(step)?;
            Ok(vec![delete_toml_document(
                work,
                &list_path(&id),
                format!("list={id}"),
            )?])
        }
        [step] if step.class == EntityClass::Layer => {
            let id = entity_id(step)?;
            Ok(vec![delete_toml_document(
                work,
                &layer_path(&id),
                format!("layer={id}"),
            )?])
        }
        [step] if step.class == EntityClass::Catalog => {
            let id = entity_id(step)?;
            let schema = catalog_schema_path(&id);
            let mut records = vec![delete_json_document(
                work,
                &schema,
                format!("catalog={id}"),
            )?];
            let data_dir = catalog_data_dir(&id);
            for path in work.paths_under(&data_dir) {
                if let Some(key) = path
                    .strip_prefix(&data_dir)
                    .and_then(|rest| rest.strip_suffix(".toml"))
                {
                    records.push(delete_toml_document(
                        work,
                        &path,
                        format!("catalog={id}:entry={key}"),
                    )?);
                } else {
                    work.delete(&path);
                }
            }
            Ok(records)
        }
        [catalog_step, entry_step]
            if catalog_step.class == EntityClass::Catalog
                && entry_step.class == EntityClass::Entry =>
        {
            let catalog = entity_id(catalog_step)?;
            let key = entity_id(entry_step)?;
            Ok(vec![delete_toml_document(
                work,
                &entry_path(&catalog, &key),
                format!("catalog={catalog}:entry={key}"),
            )?])
        }
        [step] if step.class == EntityClass::EvaluationContext => {
            let id = entity_id(step)?;
            let schema = context_schema_path(&id);
            let mut records = vec![delete_json_document(
                work,
                &schema,
                format!("evaluation-context={id}"),
            )?];
            let dir = samples_dir(&id);
            for path in work.paths_under(&dir) {
                if let Some(key) = path
                    .strip_prefix(&dir)
                    .and_then(|rest| rest.strip_suffix(".json"))
                {
                    records.push(delete_json_document(
                        work,
                        &path,
                        format!("evaluation-context={id}:sample={key}"),
                    )?);
                } else {
                    work.delete(&path);
                }
            }
            Ok(records)
        }
        [context_step, sample_step]
            if context_step.class == EntityClass::EvaluationContext
                && sample_step.class == EntityClass::Sample =>
        {
            let context = entity_id(context_step)?;
            let key = entity_id(sample_step)?;
            Ok(vec![delete_json_document(
                work,
                &sample_path(&context, &key),
                format!("evaluation-context={context}:sample={key}"),
            )?])
        }
        _ => Err(RototoError::new(
            "delete supports variables, catalogs, entries, lists, evaluation contexts, \
             samples, and layers; everything else is edited as source",
        )),
    }
}

fn delete_toml_document(
    work: &mut WorkingTree<'_>,
    path: &str,
    canonical: String,
) -> Result<ChangeRecord> {
    let content = work.content(path).ok_or_else(|| {
        RototoError::new(format!("`{canonical}` does not exist ({path} not found)"))
    })?;
    let before = content
        .parse::<DocumentMut>()
        .ok()
        .map(|document| json_from_table(document.as_table()));
    work.delete(path);
    Ok(ChangeRecord {
        operation: "delete",
        address: canonical,
        before,
        after: None,
    })
}

fn delete_json_document(
    work: &mut WorkingTree<'_>,
    path: &str,
    canonical: String,
) -> Result<ChangeRecord> {
    let content = work.content(path).ok_or_else(|| {
        RototoError::new(format!("`{canonical}` does not exist ({path} not found)"))
    })?;
    let before = serde_json::from_str::<JsonValue>(content).ok();
    work.delete(path);
    Ok(ChangeRecord {
        operation: "delete",
        address: canonical,
        before,
        after: None,
    })
}

fn ensure_absent(work: &WorkingTree<'_>, path: &str, entity: &str) -> Result<()> {
    if work.exists(path) {
        return Err(RototoError::new(format!(
            "{entity} already exists ({path})"
        )));
    }
    Ok(())
}

fn checked_schema(schema: &JsonValue) -> Result<()> {
    if schema.is_object() {
        Ok(())
    } else {
        Err(RototoError::new("a schema must be a JSON object"))
    }
}

fn pretty_json(value: &JsonValue) -> Result<String> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|err| RototoError::new(err.to_string()))?;
    text.push('\n');
    Ok(text)
}

/// A blank line before a key, the visual grouping the hand-written package
/// files use.
fn blank_line_before(table: &mut Table, key: &str) {
    comment_before(table, key, "\n");
}

fn comment_before(table: &mut Table, key: &str, prefix: &str) {
    if let Some(mut key) = table.key_mut(key) {
        key.leaf_decor_mut().set_prefix(prefix);
    }
}
