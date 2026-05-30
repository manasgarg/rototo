use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;

use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};
use crate::model::{QualifierResolution, VariableResolution, WorkspaceInspection};
use crate::workspace::{
    inspect_workspace, qualifier_for_id, read_toml, read_variable_toml, variable_for_id,
};

pub async fn resolve_qualifier(
    workspace_root: &Path,
    id: &str,
    context: &JsonValue,
) -> Result<QualifierResolution> {
    let inspection = inspect_workspace(workspace_root).await?;
    validate_context_schema(&inspection.root, context).await?;
    resolve_qualifier_unchecked(&inspection, id, context).await
}

pub(crate) async fn resolve_qualifier_unchecked(
    inspection: &WorkspaceInspection,
    id: &str,
    context: &JsonValue,
) -> Result<QualifierResolution> {
    let mut state = QualifierState::new(inspection, context);
    let value = state.resolve(id).await?;
    Ok(QualifierResolution {
        id: id.to_owned(),
        value,
    })
}

pub async fn resolve_qualifiers(
    workspace_root: &Path,
    context: &JsonValue,
) -> Result<Vec<QualifierResolution>> {
    let inspection = inspect_workspace(workspace_root).await?;
    validate_context_schema(&inspection.root, context).await?;
    resolve_qualifiers_unchecked(&inspection, context).await
}

pub(crate) async fn resolve_qualifiers_unchecked(
    inspection: &WorkspaceInspection,
    context: &JsonValue,
) -> Result<Vec<QualifierResolution>> {
    let mut state = QualifierState::new(inspection, context);
    let ids: Vec<String> = inspection
        .qualifiers
        .iter()
        .map(|qualifier| qualifier.id.clone())
        .collect();

    let mut resolutions = Vec::new();
    for id in ids {
        let value = state.resolve(&id).await?;
        resolutions.push(QualifierResolution { id, value });
    }
    Ok(resolutions)
}

pub async fn resolve_variable(
    workspace_root: &Path,
    id: &str,
    environment: &str,
    context: &JsonValue,
) -> Result<VariableResolution> {
    let inspection = inspect_workspace(workspace_root).await?;
    validate_environment(&inspection, environment)?;
    validate_context_schema(&inspection.root, context).await?;
    resolve_variable_unchecked(&inspection, id, environment, context).await
}

pub(crate) async fn resolve_variable_unchecked(
    inspection: &WorkspaceInspection,
    id: &str,
    environment: &str,
    context: &JsonValue,
) -> Result<VariableResolution> {
    let mut state = QualifierState::new(inspection, context);
    resolve_variable_with_state(inspection, &mut state, id, environment).await
}

pub async fn resolve_variables(
    workspace_root: &Path,
    environment: &str,
    context: &JsonValue,
) -> Result<Vec<VariableResolution>> {
    let inspection = inspect_workspace(workspace_root).await?;
    validate_environment(&inspection, environment)?;
    validate_context_schema(&inspection.root, context).await?;
    resolve_variables_unchecked(&inspection, environment, context).await
}

pub(crate) async fn resolve_variables_unchecked(
    inspection: &WorkspaceInspection,
    environment: &str,
    context: &JsonValue,
) -> Result<Vec<VariableResolution>> {
    let ids: Vec<String> = inspection
        .variables
        .iter()
        .map(|variable| variable.id.clone())
        .collect();
    let mut state = QualifierState::new(inspection, context);

    let mut resolutions = Vec::new();
    for id in ids {
        resolutions
            .push(resolve_variable_with_state(inspection, &mut state, &id, environment).await?);
    }
    Ok(resolutions)
}

fn validate_environment(inspection: &WorkspaceInspection, environment: &str) -> Result<()> {
    if inspection
        .environments
        .iter()
        .any(|known| known == environment)
    {
        Ok(())
    } else {
        Err(RototoError::new(format!(
            "unknown environment: {environment}"
        )))
    }
}

async fn validate_context_schema(root: &Path, context: &JsonValue) -> Result<()> {
    let manifest = read_toml(&root.join("rototo-workspace.toml")).await?;
    let Some(context_config) = manifest.get("context") else {
        return Ok(());
    };
    let context_config = context_config
        .as_table()
        .ok_or_else(|| RototoError::new("[context] must be a table"))?;
    let schema_ref = context_config
        .get("schema")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| RototoError::new("[context] must declare schema"))?;
    let schema_path = context_schema_path(root, schema_ref)?;
    let text = tokio::fs::read_to_string(&schema_path)
        .await
        .map_err(|err| {
            RototoError::new(format!(
                "failed to read context schema {}: {err}",
                schema_path.display()
            ))
        })?;
    let schema = serde_json::from_str::<JsonValue>(&text).map_err(|err| {
        RototoError::new(format!(
            "failed to parse context schema {}: {err}",
            schema_path.display()
        ))
    })?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|err| RototoError::new(format!("context schema is invalid: {err}")))?;
    validator
        .validate(context)
        .map_err(|err| RototoError::new(format!("resolve context does not match schema: {err}")))
}

fn context_schema_path(root: &Path, schema_ref: &str) -> Result<PathBuf> {
    let schema_ref = Path::new(schema_ref);
    if schema_ref.as_os_str().is_empty()
        || schema_ref.is_absolute()
        || schema_ref
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(RototoError::new(
            "context schema path must be a relative path inside the workspace",
        ));
    }
    Ok(root.join(schema_ref))
}

async fn resolve_variable_with_state(
    inspection: &WorkspaceInspection,
    state: &mut QualifierState<'_>,
    id: &str,
    environment: &str,
) -> Result<VariableResolution> {
    let variable = variable_for_id(inspection, id)?;
    let toml = read_variable_toml(&inspection.root, variable).await?;
    let variable_toml = toml
        .as_table()
        .ok_or_else(|| RototoError::new(format!("variable TOML root is not a table: {id}")))?;
    let values = variable_toml
        .get("values")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| RototoError::new(format!("variable has no values: {id}")))?;
    let env = variable_toml
        .get("env")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| RototoError::new(format!("variable has no environments: {id}")))?;

    let block = env
        .get(environment)
        .or_else(|| env.get("_"))
        .ok_or_else(|| RototoError::new(format!("variable has no environment fallback: {id}")))?;
    let block = block
        .as_table()
        .ok_or_else(|| RototoError::new(format!("environment block is not a table: {id}")))?;

    let mut value_key = None;
    if let Some(rules) = block.get("rule").and_then(toml::Value::as_array) {
        for rule in rules {
            let Some(rule) = rule.as_table() else {
                continue;
            };
            let Some(qualifier) = rule.get("qualifier").and_then(toml::Value::as_str) else {
                continue;
            };
            if state.resolve(qualifier).await? {
                value_key = rule
                    .get("value")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);
                if value_key.is_some() {
                    break;
                }
            }
        }
    }

    let value_key = value_key
        .or_else(|| {
            block
                .get("value")
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| RototoError::new(format!("environment block has no value: {id}")))?;
    let value = values.get(&value_key).ok_or_else(|| {
        RototoError::new(format!("variable references unknown value: {value_key}"))
    })?;
    let value = serde_json::to_value(value).map_err(|err| RototoError::new(err.to_string()))?;

    Ok(VariableResolution {
        id: id.to_owned(),
        environment: environment.to_owned(),
        value_key,
        value,
    })
}

struct QualifierState<'a> {
    inspection: &'a WorkspaceInspection,
    context: &'a JsonValue,
    cache: HashMap<String, bool>,
    resolving: HashSet<String>,
}

impl<'a> QualifierState<'a> {
    fn new(inspection: &'a WorkspaceInspection, context: &'a JsonValue) -> Self {
        Self {
            inspection,
            context,
            cache: HashMap::new(),
            resolving: HashSet::new(),
        }
    }

    fn resolve<'b>(&'b mut self, id: &'b str) -> Pin<Box<dyn Future<Output = Result<bool>> + 'b>> {
        Box::pin(async move {
            if let Some(value) = self.cache.get(id) {
                return Ok(*value);
            }
            if !self.resolving.insert(id.to_owned()) {
                return Err(RototoError::new(format!(
                    "qualifier cycle detected at qualifier://{id}"
                )));
            }

            let qualifier = qualifier_for_id(self.inspection, id)?;
            let toml = read_toml(&self.inspection.root.join(&qualifier.path)).await?;
            let predicates = toml
                .get("predicate")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| RototoError::new(format!("qualifier has no predicates: {id}")))?;

            let mut value = true;
            for predicate in predicates {
                if !self.evaluate_predicate(predicate).await? {
                    value = false;
                    break;
                }
            }

            self.resolving.remove(id);
            self.cache.insert(id.to_owned(), value);
            Ok(value)
        })
    }

    async fn evaluate_predicate(&mut self, predicate: &toml::Value) -> Result<bool> {
        let predicate = predicate
            .as_table()
            .ok_or_else(|| RototoError::new("predicate must be a table"))?;
        let attribute = predicate
            .get("attribute")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| RototoError::new("predicate must contain attribute"))?;
        let op = predicate
            .get("op")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| RototoError::new("predicate must contain op"))?;

        if op == "bucket" {
            let Some(context_value) = context_path(self.context, attribute) else {
                return Ok(false);
            };
            let salt = predicate
                .get("salt")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| RototoError::new("bucket predicate must contain salt"))?;
            let range = predicate
                .get("range")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| RototoError::new("bucket predicate must contain range"))?;
            let start = range.first().and_then(toml::Value::as_integer).unwrap_or(0);
            let end = range.get(1).and_then(toml::Value::as_integer).unwrap_or(0);
            let bucket = bucket_value(salt, context_value);
            return Ok(i64::from(bucket) >= start && i64::from(bucket) < end);
        }

        let actual = if let Some(qualifier) = attribute.strip_prefix("qualifier.") {
            JsonValue::Bool(self.resolve(qualifier).await?)
        } else {
            let Some(value) = context_path(self.context, attribute) else {
                return Ok(false);
            };
            value.clone()
        };
        let expected = predicate
            .get("value")
            .ok_or_else(|| RototoError::new("predicate must contain value"))?;
        let expected =
            serde_json::to_value(expected).map_err(|err| RototoError::new(err.to_string()))?;

        Ok(match op {
            "eq" => actual == expected,
            "neq" => actual != expected,
            "in" => expected
                .as_array()
                .is_some_and(|values| values.iter().any(|value| value == &actual)),
            "not_in" => expected
                .as_array()
                .is_some_and(|values| values.iter().all(|value| value != &actual)),
            "gt" => numeric_compare(&actual, &expected, |left, right| left > right),
            "gte" => numeric_compare(&actual, &expected, |left, right| left >= right),
            "lt" => numeric_compare(&actual, &expected, |left, right| left < right),
            "lte" => numeric_compare(&actual, &expected, |left, right| left <= right),
            _ => false,
        })
    }
}

fn context_path<'a>(context: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut current = context;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn numeric_compare(
    actual: &JsonValue,
    expected: &JsonValue,
    compare: impl FnOnce(f64, f64) -> bool,
) -> bool {
    let Some(actual) = actual.as_f64() else {
        return false;
    };
    let Some(expected) = expected.as_f64() else {
        return false;
    };
    compare(actual, expected)
}

fn bucket_value(salt: &str, value: &JsonValue) -> u16 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in salt
        .bytes()
        .chain([b':'])
        .chain(canonical_context_value(value).bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    (hash % 10_000) as u16
}

fn canonical_context_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        _ => value.to_string(),
    }
}
