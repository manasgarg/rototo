use std::cmp::Ordering;
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
    let schema_path = context_schema_path(root, schema_ref).await?;
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

async fn context_schema_path(root: &Path, schema_ref: &str) -> Result<PathBuf> {
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
    let root = tokio::fs::canonicalize(root).await.map_err(|err| {
        RototoError::new(format!(
            "failed to canonicalize workspace root {}: {err}",
            root.display()
        ))
    })?;
    let schema_path = root.join(schema_ref);
    let canonical_schema = tokio::fs::canonicalize(&schema_path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to read context schema {}: {err}",
            schema_path.display()
        ))
    })?;
    if !canonical_schema.starts_with(&root) {
        return Err(RototoError::new(
            "context schema path must resolve inside the workspace",
        ));
    }
    Ok(canonical_schema)
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
    if let Some(rule_value) = block.get("rule") {
        let rules = rule_value
            .as_array()
            .ok_or_else(|| RototoError::new(format!("environment rule must be an array: {id}")))?;
        for rule in rules {
            let rule = rule.as_table().ok_or_else(|| {
                RototoError::new(format!("environment rule must be a table: {id}"))
            })?;
            let qualifier = rule
                .get("qualifier")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| {
                    RototoError::new(format!("environment rule must reference a qualifier: {id}"))
                })?;
            if state.resolve(qualifier).await? {
                value_key = Some(
                    rule.get("value")
                        .and_then(toml::Value::as_str)
                        .ok_or_else(|| {
                            RototoError::new(format!(
                                "matching environment rule must reference a value: {id}"
                            ))
                        })?
                        .to_owned(),
                );
                break;
            }
        }
    }

    let value_key = match value_key {
        Some(value_key) => value_key,
        None => block
            .get("value")
            .ok_or_else(|| RototoError::new(format!("environment block has no value: {id}")))?
            .as_str()
            .ok_or_else(|| {
                RototoError::new(format!("environment block value must be a string: {id}"))
            })?
            .to_owned(),
    };
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
            if predicates.is_empty() {
                return Err(RototoError::new(format!(
                    "qualifier must contain at least one predicate: {id}"
                )));
            }

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
            if range.len() != 2 {
                return Err(RototoError::new(
                    "bucket predicate range must contain two integers",
                ));
            }
            let start = range
                .first()
                .and_then(toml::Value::as_integer)
                .ok_or_else(|| {
                    RototoError::new("bucket predicate range must contain two integers")
                })?;
            let end = range
                .get(1)
                .and_then(toml::Value::as_integer)
                .ok_or_else(|| {
                    RototoError::new("bucket predicate range must contain two integers")
                })?;
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
            "eq" => json_values_equal(&actual, &expected),
            "neq" => !json_values_equal(&actual, &expected),
            "in" => expected
                .as_array()
                .is_some_and(|values| values.iter().any(|value| json_values_equal(value, &actual))),
            "not_in" => expected.as_array().is_some_and(|values| {
                values
                    .iter()
                    .all(|value| !json_values_equal(value, &actual))
            }),
            "gt" => numeric_compare(&actual, &expected, |ordering| ordering == Ordering::Greater),
            "gte" => numeric_compare(&actual, &expected, |ordering| {
                matches!(ordering, Ordering::Greater | Ordering::Equal)
            }),
            "lt" => numeric_compare(&actual, &expected, |ordering| ordering == Ordering::Less),
            "lte" => numeric_compare(&actual, &expected, |ordering| {
                matches!(ordering, Ordering::Less | Ordering::Equal)
            }),
            _ => {
                return Err(RototoError::new(format!(
                    "unknown predicate operator: {op}"
                )));
            }
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

fn json_values_equal(left: &JsonValue, right: &JsonValue) -> bool {
    match (left.as_number(), right.as_number()) {
        (Some(left), Some(right)) => exact_number_ordering(left, right) == Some(Ordering::Equal),
        _ => left == right,
    }
}

fn numeric_compare(
    actual: &JsonValue,
    expected: &JsonValue,
    compare: impl FnOnce(Ordering) -> bool,
) -> bool {
    let (Some(actual), Some(expected)) = (actual.as_number(), expected.as_number()) else {
        return false;
    };
    exact_number_ordering(actual, expected).is_some_and(compare)
}

fn exact_number_ordering(
    left: &serde_json::Number,
    right: &serde_json::Number,
) -> Option<Ordering> {
    match (number_integer(left), number_integer(right)) {
        (Some(IntegerNumber::Signed(left)), Some(IntegerNumber::Signed(right))) => {
            return Some(left.cmp(&right));
        }
        (Some(IntegerNumber::Unsigned(left)), Some(IntegerNumber::Unsigned(right))) => {
            return Some(left.cmp(&right));
        }
        (Some(IntegerNumber::Signed(left)), Some(IntegerNumber::Unsigned(right))) => {
            return if left < 0 {
                Some(Ordering::Less)
            } else {
                Some((left as u64).cmp(&right))
            };
        }
        (Some(IntegerNumber::Unsigned(left)), Some(IntegerNumber::Signed(right))) => {
            return if right < 0 {
                Some(Ordering::Greater)
            } else {
                Some(left.cmp(&(right as u64)))
            };
        }
        _ => {}
    }

    let left = exact_f64(left)?;
    let right = exact_f64(right)?;
    left.partial_cmp(&right)
}

#[derive(Clone, Copy)]
enum IntegerNumber {
    Signed(i64),
    Unsigned(u64),
}

fn number_integer(number: &serde_json::Number) -> Option<IntegerNumber> {
    number
        .as_i64()
        .map(IntegerNumber::Signed)
        .or_else(|| number.as_u64().map(IntegerNumber::Unsigned))
}

fn exact_f64(number: &serde_json::Number) -> Option<f64> {
    if let Some(value) = number.as_i64() {
        let as_f64 = value as f64;
        return ((as_f64 as i64) == value).then_some(as_f64);
    }
    if let Some(value) = number.as_u64() {
        let as_f64 = value as f64;
        return ((as_f64 as u64) == value).then_some(as_f64);
    }
    number.as_f64().filter(|value| value.is_finite())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_predicate_operator_true_and_false_outcomes() {
        let workspace = workspace_with_qualifiers(&[
            ("eq-true", predicate("user.tier", "eq", r#""premium""#)),
            ("eq-false", predicate("user.tier", "eq", r#""free""#)),
            ("neq-true", predicate("user.tier", "neq", r#""free""#)),
            ("neq-false", predicate("user.tier", "neq", r#""premium""#)),
            (
                "in-true",
                predicate("user.tier", "in", r#"["free", "premium"]"#),
            ),
            ("in-false", predicate("user.tier", "in", r#"["free"]"#)),
            (
                "not-in-true",
                predicate("user.tier", "not_in", r#"["free"]"#),
            ),
            (
                "not-in-false",
                predicate("user.tier", "not_in", r#"["premium"]"#),
            ),
            ("gt-true", predicate("account.seats", "gt", "10")),
            ("gt-false", predicate("account.seats", "gt", "100")),
            ("gte-true", predicate("account.seats", "gte", "42")),
            ("gte-false", predicate("account.seats", "gte", "43")),
            ("lt-true", predicate("account.seats", "lt", "100")),
            ("lt-false", predicate("account.seats", "lt", "10")),
            ("lte-true", predicate("account.seats", "lte", "42")),
            ("lte-false", predicate("account.seats", "lte", "41")),
            (
                "missing-neq-false",
                predicate("missing.path", "neq", r#""anything""#),
            ),
            (
                "missing-not-in-false",
                predicate("missing.path", "not_in", r#"["anything"]"#),
            ),
        ]);
        let context = serde_json::json!({
            "user": { "tier": "premium" },
            "account": { "seats": 42 }
        });

        for id in [
            "eq-true",
            "neq-true",
            "in-true",
            "not-in-true",
            "gt-true",
            "gte-true",
            "lt-true",
            "lte-true",
        ] {
            assert!(
                resolve_qualifier(workspace.path(), id, &context)
                    .await
                    .unwrap()
                    .value,
                "{id}"
            );
        }

        for id in [
            "eq-false",
            "neq-false",
            "in-false",
            "not-in-false",
            "gt-false",
            "gte-false",
            "lt-false",
            "lte-false",
            "missing-neq-false",
            "missing-not-in-false",
        ] {
            assert!(
                !resolve_qualifier(workspace.path(), id, &context)
                    .await
                    .unwrap()
                    .value,
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn resolves_bucket_boundaries_and_is_deterministic() {
        assert_eq!(
            bucket_value("known-salt", &serde_json::json!("user-123")),
            9913
        );
        assert_eq!(
            bucket_value("known-salt", &serde_json::json!("user-123")),
            bucket_value("known-salt", &serde_json::json!("user-123"))
        );
        let workspace = workspace_with_qualifiers(&[
            ("bucket-in", bucket_predicate("9913, 9914")),
            ("bucket-start-exclusive", bucket_predicate("9914, 9915")),
            ("bucket-end-exclusive", bucket_predicate("9912, 9913")),
        ]);
        let context = serde_json::json!({ "user": { "id": "user-123" } });

        assert!(
            resolve_qualifier(workspace.path(), "bucket-in", &context)
                .await
                .unwrap()
                .value
        );
        assert!(
            !resolve_qualifier(workspace.path(), "bucket-start-exclusive", &context)
                .await
                .unwrap()
                .value
        );
        assert!(
            !resolve_qualifier(workspace.path(), "bucket-end-exclusive", &context)
                .await
                .unwrap()
                .value
        );
    }

    #[tokio::test]
    async fn resolves_qualifier_indirection_and_cycles() {
        let workspace = workspace_with_qualifiers(&[
            ("premium", predicate("user.tier", "eq", r#""premium""#)),
            ("free", predicate("user.tier", "eq", r#""free""#)),
            (
                "premium-derived",
                predicate("qualifier.premium", "eq", "true"),
            ),
            ("free-derived", predicate("qualifier.free", "eq", "true")),
            ("cycle-a", predicate("qualifier.cycle-b", "eq", "true")),
            ("cycle-b", predicate("qualifier.cycle-a", "eq", "true")),
        ]);
        let context = serde_json::json!({ "user": { "tier": "premium" } });

        assert!(
            resolve_qualifier(workspace.path(), "premium-derived", &context)
                .await
                .unwrap()
                .value
        );
        assert!(
            !resolve_qualifier(workspace.path(), "free-derived", &context)
                .await
                .unwrap()
                .value
        );
        let err = resolve_qualifier(workspace.path(), "cycle-a", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("qualifier cycle detected"));
    }

    #[tokio::test]
    async fn resolves_variable_environment_fallback_and_fails_closed() {
        let workspace =
            workspace_with_qualifiers(&[("premium", predicate("user.tier", "eq", r#""premium""#))]);
        std::fs::create_dir_all(workspace.path().join("variables")).unwrap();
        std::fs::write(
            workspace.path().join("variables/message.toml"),
            r#"schema_version = 1
type = "string"

[values]
control = "control"
premium = "premium"

[env._]
value = "control"

[env.prod]
value = "control"
rule = [
  { qualifier = "premium", value = "premium" },
]
"#,
        )
        .unwrap();
        let context = serde_json::json!({ "user": { "tier": "free" } });

        let fallback = resolve_variable(workspace.path(), "message", "stage", &context)
            .await
            .unwrap();
        assert_eq!(fallback.value_key, "control");

        std::fs::write(
            workspace.path().join("variables/bad-rule.toml"),
            r#"schema_version = 1
type = "string"

[values]
control = "control"

[env._]
value = "control"
rule = ["not-a-table"]
"#,
        )
        .unwrap();
        let err = resolve_variable(workspace.path(), "bad-rule", "prod", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("environment rule must be a table"));
    }

    #[tokio::test]
    async fn numeric_equality_is_exact_without_lossy_large_integer_casts() {
        let workspace = workspace_with_qualifiers(&[
            ("int-float-equal", predicate("n", "eq", "100.0")),
            (
                "large-int-float-not-equal",
                predicate("large", "eq", "9007199254740992.0"),
            ),
            (
                "large-int-self-equal",
                predicate("large", "eq", "9007199254740993"),
            ),
        ]);
        let context = serde_json::json!({
            "n": 100,
            "large": 9007199254740993_i64
        });

        assert!(
            resolve_qualifier(workspace.path(), "int-float-equal", &context)
                .await
                .unwrap()
                .value
        );
        assert!(
            !resolve_qualifier(workspace.path(), "large-int-float-not-equal", &context)
                .await
                .unwrap()
                .value
        );
        assert!(
            resolve_qualifier(workspace.path(), "large-int-self-equal", &context)
                .await
                .unwrap()
                .value
        );
    }

    #[tokio::test]
    async fn malformed_predicates_return_errors_during_unchecked_resolution() {
        let workspace = workspace_with_qualifiers(&[
            (
                "unknown-op",
                predicate("user.tier", "contains", r#""premium""#),
            ),
            (
                "empty",
                String::from("schema_version = 1\npredicate = []\n"),
            ),
            (
                "bad-bucket",
                String::from(
                    r#"schema_version = 1

[[predicate]]
attribute = "user.id"
op = "bucket"
salt = "known-salt"
range = [1.5, 2.5]
"#,
                ),
            ),
        ]);
        let context = serde_json::json!({ "user": { "tier": "premium", "id": "user-123" } });

        let err = resolve_qualifier(workspace.path(), "unknown-op", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown predicate operator"));

        let err = resolve_qualifier(workspace.path(), "empty", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("at least one predicate"));

        let err = resolve_qualifier(workspace.path(), "bad-bucket", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("range must contain two integers"));
    }

    fn workspace_with_qualifiers(qualifiers: &[(&str, String)]) -> tempfile::TempDir {
        let workspace = tempfile::TempDir::new().unwrap();
        std::fs::write(
            workspace.path().join("rototo-workspace.toml"),
            r#"schema_version = 1

[environments]
values = ["prod", "stage"]
"#,
        )
        .unwrap();
        std::fs::create_dir_all(workspace.path().join("qualifiers")).unwrap();
        for (id, contents) in qualifiers {
            std::fs::write(
                workspace.path().join(format!("qualifiers/{id}.toml")),
                contents,
            )
            .unwrap();
        }
        workspace
    }

    fn predicate(attribute: &str, op: &str, value: &str) -> String {
        format!(
            r#"schema_version = 1

[[predicate]]
attribute = "{attribute}"
op = "{op}"
value = {value}
"#
        )
    }

    fn bucket_predicate(range: &str) -> String {
        format!(
            r#"schema_version = 1

[[predicate]]
attribute = "user.id"
op = "bucket"
salt = "known-salt"
range = [{range}]
"#
        )
    }
}
