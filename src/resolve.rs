use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};
use crate::lint::{
    RuntimeAttribute, RuntimeCompareOp, RuntimePredicate, RuntimeWorkspace,
    compile_runtime_workspace,
};
use crate::model::{QualifierResolution, VariableResolution};

pub async fn resolve_qualifier(
    workspace_root: &Path,
    id: &str,
    context: &JsonValue,
) -> Result<QualifierResolution> {
    let runtime = compile_runtime_workspace(workspace_root).await?;
    runtime.validate_context(context)?;
    resolve_qualifier_unchecked(&runtime, id, context).await
}

pub(crate) async fn resolve_qualifier_unchecked(
    runtime: &RuntimeWorkspace,
    id: &str,
    context: &JsonValue,
) -> Result<QualifierResolution> {
    let mut state = QualifierState::new(runtime, context);
    let value = state.resolve(id)?;
    Ok(QualifierResolution {
        id: id.to_owned(),
        value,
    })
}

pub async fn resolve_qualifiers(
    workspace_root: &Path,
    context: &JsonValue,
) -> Result<Vec<QualifierResolution>> {
    let runtime = compile_runtime_workspace(workspace_root).await?;
    runtime.validate_context(context)?;
    resolve_qualifiers_unchecked(&runtime, context).await
}

pub(crate) async fn resolve_qualifiers_unchecked(
    runtime: &RuntimeWorkspace,
    context: &JsonValue,
) -> Result<Vec<QualifierResolution>> {
    let mut state = QualifierState::new(runtime, context);
    let ids: Vec<String> = runtime.qualifiers.keys().cloned().collect();

    let mut resolutions = Vec::new();
    for id in ids {
        let value = state.resolve(&id)?;
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
    let runtime = compile_runtime_workspace(workspace_root).await?;
    runtime.validate_environment(environment)?;
    runtime.validate_context(context)?;
    resolve_variable_unchecked(&runtime, id, environment, context).await
}

pub(crate) async fn resolve_variable_unchecked(
    runtime: &RuntimeWorkspace,
    id: &str,
    environment: &str,
    context: &JsonValue,
) -> Result<VariableResolution> {
    let mut state = QualifierState::new(runtime, context);
    resolve_variable_with_state(runtime, &mut state, id, environment)
}

pub async fn resolve_variables(
    workspace_root: &Path,
    environment: &str,
    context: &JsonValue,
) -> Result<Vec<VariableResolution>> {
    let runtime = compile_runtime_workspace(workspace_root).await?;
    runtime.validate_environment(environment)?;
    runtime.validate_context(context)?;
    resolve_variables_unchecked(&runtime, environment, context).await
}

pub(crate) async fn resolve_variables_unchecked(
    runtime: &RuntimeWorkspace,
    environment: &str,
    context: &JsonValue,
) -> Result<Vec<VariableResolution>> {
    let ids: Vec<String> = runtime.variables.keys().cloned().collect();
    let mut state = QualifierState::new(runtime, context);

    let mut resolutions = Vec::new();
    for id in ids {
        resolutions.push(resolve_variable_with_state(
            runtime,
            &mut state,
            &id,
            environment,
        )?);
    }
    Ok(resolutions)
}

fn resolve_variable_with_state(
    runtime: &RuntimeWorkspace,
    state: &mut QualifierState<'_>,
    id: &str,
    environment: &str,
) -> Result<VariableResolution> {
    let variable = runtime
        .variables
        .get(id)
        .ok_or_else(|| RototoError::new(format!("variable not found: variable://{id}")))?;
    let block = variable
        .environments
        .get(environment)
        .or_else(|| variable.environments.get("_"))
        .ok_or_else(|| RototoError::new(format!("variable has no environment fallback: {id}")))?;

    let mut value_key = None;
    for rule in &block.rules {
        if state.resolve(&rule.qualifier)? {
            value_key = Some(rule.value.clone());
            break;
        }
    }

    let value_key = value_key.unwrap_or_else(|| block.value.clone());
    let value = variable.values.get(&value_key).ok_or_else(|| {
        RototoError::new(format!("variable references unknown value: {value_key}"))
    })?;

    Ok(VariableResolution {
        id: id.to_owned(),
        environment: environment.to_owned(),
        value_key,
        value: value.clone(),
    })
}

struct QualifierState<'a> {
    runtime: &'a RuntimeWorkspace,
    context: &'a JsonValue,
    cache: HashMap<String, bool>,
    resolving: HashSet<String>,
}

impl<'a> QualifierState<'a> {
    fn new(runtime: &'a RuntimeWorkspace, context: &'a JsonValue) -> Self {
        Self {
            runtime,
            context,
            cache: HashMap::new(),
            resolving: HashSet::new(),
        }
    }

    fn resolve(&mut self, id: &str) -> Result<bool> {
        if let Some(value) = self.cache.get(id) {
            return Ok(*value);
        }
        if !self.resolving.insert(id.to_owned()) {
            return Err(RototoError::new(format!(
                "qualifier cycle detected at qualifier://{id}"
            )));
        }

        let result = self.resolve_uncached(id);
        self.resolving.remove(id);
        let value = result?;
        self.cache.insert(id.to_owned(), value);
        Ok(value)
    }

    fn resolve_uncached(&mut self, id: &str) -> Result<bool> {
        let qualifier =
            self.runtime.qualifiers.get(id).ok_or_else(|| {
                RototoError::new(format!("qualifier not found: qualifier://{id}"))
            })?;
        for predicate in &qualifier.predicates {
            if !self.evaluate_predicate(predicate)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn evaluate_predicate(&mut self, predicate: &RuntimePredicate) -> Result<bool> {
        match predicate {
            RuntimePredicate::Bucket {
                attribute,
                salt,
                start,
                end,
            } => {
                let Some(context_value) = context_path(self.context, attribute) else {
                    return Ok(false);
                };
                let bucket = bucket_value(salt, context_value);
                Ok(i64::from(bucket) >= *start && i64::from(bucket) < *end)
            }
            RuntimePredicate::Compare {
                attribute,
                op,
                value,
            } => {
                let actual = match attribute {
                    RuntimeAttribute::Qualifier(qualifier) => {
                        JsonValue::Bool(self.resolve(qualifier)?)
                    }
                    RuntimeAttribute::ContextPath(path) => {
                        let Some(value) = context_path(self.context, path) else {
                            return Ok(false);
                        };
                        value.clone()
                    }
                };

                Ok(match op {
                    RuntimeCompareOp::Eq => json_values_equal(&actual, value),
                    RuntimeCompareOp::Neq => !json_values_equal(&actual, value),
                    RuntimeCompareOp::In => value.as_array().is_some_and(|values| {
                        values.iter().any(|value| json_values_equal(value, &actual))
                    }),
                    RuntimeCompareOp::NotIn => value.as_array().is_some_and(|values| {
                        values
                            .iter()
                            .all(|value| !json_values_equal(value, &actual))
                    }),
                    RuntimeCompareOp::Gt => {
                        numeric_compare(&actual, value, |ordering| ordering == Ordering::Greater)
                    }
                    RuntimeCompareOp::Gte => numeric_compare(&actual, value, |ordering| {
                        matches!(ordering, Ordering::Greater | Ordering::Equal)
                    }),
                    RuntimeCompareOp::Lt => {
                        numeric_compare(&actual, value, |ordering| ordering == Ordering::Less)
                    }
                    RuntimeCompareOp::Lte => numeric_compare(&actual, value, |ordering| {
                        matches!(ordering, Ordering::Less | Ordering::Equal)
                    }),
                })
            }
        }
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
        assert!(err.to_string().contains("rule must be a table"));
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
        let context = serde_json::json!({ "user": { "tier": "premium", "id": "user-123" } });

        let workspace = workspace_with_qualifiers(&[(
            "unknown-op",
            predicate("user.tier", "contains", r#""premium""#),
        )]);
        let err = resolve_qualifier(workspace.path(), "unknown-op", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown predicate operator"));

        let workspace = workspace_with_qualifiers(&[(
            "empty",
            String::from("schema_version = 1\npredicate = []\n"),
        )]);
        let err = resolve_qualifier(workspace.path(), "empty", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("at least one predicate"));

        let workspace = workspace_with_qualifiers(&[(
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
        )]);
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
