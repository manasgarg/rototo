use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};
use crate::lint::{
    RuntimeCatalogQuery, RuntimePackage, RuntimeRule, RuntimeRuleSelection, RuntimeSelectedValue,
    compile_runtime_package,
};
use crate::model::{QualifierResolution, VariableResolution, VariableResolutionSource};
use crate::model::{
    QualifierResolutionTrace, VariableResolutionTrace, VariableRuleResolutionTrace,
};

pub async fn resolve_qualifier(package_root: &Path, id: &str, context: &JsonValue) -> Result<bool> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context_for_qualifier(id, context)?;
    resolve_qualifier_unchecked(&runtime, id, context)
}

pub async fn trace_qualifier_resolution(
    package_root: &Path,
    id: &str,
    context: &JsonValue,
) -> Result<QualifierResolutionTrace> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context_for_qualifier(id, context)?;
    trace_qualifier_unchecked(&runtime, id, context)
}

pub(crate) fn resolve_qualifier_unchecked(
    runtime: &RuntimePackage,
    id: &str,
    context: &JsonValue,
) -> Result<bool> {
    let mut state = QualifierState::new(runtime, context);
    state.resolve(id)
}

pub(crate) fn trace_qualifier_unchecked(
    runtime: &RuntimePackage,
    id: &str,
    context: &JsonValue,
) -> Result<QualifierResolutionTrace> {
    let mut state = QualifierState::new(runtime, context);
    state.resolve(id)?;
    state
        .take_qualifier_trace(id)
        .ok_or_else(|| RototoError::new(format!("qualifier trace not found: qualifier://{id}")))
}

pub async fn resolve_qualifiers(
    package_root: &Path,
    context: &JsonValue,
) -> Result<Vec<QualifierResolution>> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context(context)?;
    resolve_qualifiers_unchecked(&runtime, context)
}

pub async fn trace_qualifier_resolutions(
    package_root: &Path,
    context: &JsonValue,
) -> Result<Vec<QualifierResolutionTrace>> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context(context)?;
    trace_qualifier_resolutions_unchecked(&runtime, context)
}

pub(crate) fn resolve_qualifiers_unchecked(
    runtime: &RuntimePackage,
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

pub(crate) fn trace_qualifier_resolutions_unchecked(
    runtime: &RuntimePackage,
    context: &JsonValue,
) -> Result<Vec<QualifierResolutionTrace>> {
    let ids: Vec<String> = runtime.qualifiers.keys().cloned().collect();
    let mut state = QualifierState::new(runtime, context);

    let mut traces = Vec::new();
    for id in ids {
        state.resolve(&id)?;
        traces.push(state.take_qualifier_trace(&id).ok_or_else(|| {
            RototoError::new(format!("qualifier trace not found: qualifier://{id}"))
        })?);
    }
    Ok(traces)
}

pub async fn resolve_variable(
    package_root: &Path,
    id: &str,
    context: &JsonValue,
) -> Result<VariableResolution> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context_for_variable(id, context)?;
    resolve_variable_unchecked(&runtime, id, context)
}

pub async fn trace_variable_resolution(
    package_root: &Path,
    id: &str,
    context: &JsonValue,
) -> Result<VariableResolutionTrace> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context_for_variable(id, context)?;
    trace_variable_unchecked(&runtime, id, context)
}

pub(crate) fn resolve_variable_unchecked(
    runtime: &RuntimePackage,
    id: &str,
    context: &JsonValue,
) -> Result<VariableResolution> {
    let mut state = QualifierState::new(runtime, context);
    resolve_variable_with_state(runtime, &mut state, id)
}

pub(crate) fn trace_variable_unchecked(
    runtime: &RuntimePackage,
    id: &str,
    context: &JsonValue,
) -> Result<VariableResolutionTrace> {
    let mut state = QualifierState::new(runtime, context);
    resolve_variable_trace_with_state(runtime, &mut state, id)
}

pub async fn resolve_variables(
    package_root: &Path,
    context: &JsonValue,
) -> Result<Vec<VariableResolution>> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context(context)?;
    resolve_variables_unchecked(&runtime, context)
}

pub async fn trace_variable_resolutions(
    package_root: &Path,
    context: &JsonValue,
) -> Result<Vec<VariableResolutionTrace>> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context(context)?;
    trace_variable_resolutions_unchecked(&runtime, context)
}

pub(crate) fn resolve_variables_unchecked(
    runtime: &RuntimePackage,
    context: &JsonValue,
) -> Result<Vec<VariableResolution>> {
    let ids: Vec<String> = runtime.variables.keys().cloned().collect();
    let mut state = QualifierState::new(runtime, context);

    let mut resolutions = Vec::new();
    for id in ids {
        resolutions.push(resolve_variable_with_state(runtime, &mut state, &id)?);
    }
    Ok(resolutions)
}

pub(crate) fn trace_variable_resolutions_unchecked(
    runtime: &RuntimePackage,
    context: &JsonValue,
) -> Result<Vec<VariableResolutionTrace>> {
    let ids: Vec<String> = runtime.variables.keys().cloned().collect();

    let mut traces = Vec::new();
    for id in ids {
        let mut state = QualifierState::new(runtime, context);
        traces.push(resolve_variable_trace_with_state(runtime, &mut state, &id)?);
    }
    Ok(traces)
}

fn resolve_variable_with_state(
    runtime: &RuntimePackage,
    state: &mut QualifierState<'_>,
    id: &str,
) -> Result<VariableResolution> {
    Ok(resolve_variable_trace_with_state(runtime, state, id)?.resolution)
}

fn resolve_variable_trace_with_state(
    runtime: &RuntimePackage,
    state: &mut QualifierState<'_>,
    id: &str,
) -> Result<VariableResolutionTrace> {
    let variable = runtime
        .variables
        .get(id)
        .ok_or_else(|| RototoError::new(format!("variable not found: variable://{id}")))?;

    let mut selected = None;
    let mut rules = Vec::new();
    for rule in &variable.rules {
        let matched = evaluate_rule_selector(state, rule)?;
        let rule_value = if matched {
            Some(resolve_rule_selection(runtime, state, &rule.selection)?)
        } else {
            None
        };
        let trace_value = rule_value
            .as_ref()
            .map(|value| value.value().clone())
            .or_else(|| {
                static_rule_selection_value(&rule.selection).map(|value| value.value().clone())
            })
            .unwrap_or(JsonValue::Null);
        let trace_source = rule_value
            .as_ref()
            .map(selected_value_source)
            .or_else(|| static_rule_selection_value(&rule.selection).map(selected_value_source))
            .unwrap_or(VariableResolutionSource::Literal);
        rules.push(VariableRuleResolutionTrace {
            index: rule.index,
            condition: rule
                .when
                .as_ref()
                .map(|when| when.source().to_owned())
                .unwrap_or_else(|| "<query>".to_owned()),
            value: trace_value,
            source: trace_source,
            matched,
        });
        if matched {
            selected = rule_value;
            break;
        }
    }

    let selected = selected.unwrap_or_else(|| variable.default.clone());

    let resolution = VariableResolution {
        id: id.to_owned(),
        value: selected.value().clone(),
        source: selected_value_source(&selected),
    };
    let qualifier_traces = state.qualifier_traces();

    Ok(VariableResolutionTrace {
        resolution,
        default_value: variable.default.value().clone(),
        default_source: selected_value_source(&variable.default),
        rules,
        qualifier_traces,
    })
}

fn selected_value_source(value: &RuntimeSelectedValue) -> VariableResolutionSource {
    match value {
        RuntimeSelectedValue::Literal(_) => VariableResolutionSource::Literal,
        RuntimeSelectedValue::Catalog { catalog, name, .. } => VariableResolutionSource::Catalog {
            catalog: catalog.clone(),
            value: name.clone(),
        },
        RuntimeSelectedValue::CatalogList { catalog, names, .. } => {
            VariableResolutionSource::CatalogList {
                catalog: catalog.clone(),
                values: names.clone(),
            }
        }
    }
}

fn evaluate_rule_selector(state: &mut QualifierState<'_>, rule: &RuntimeRule) -> Result<bool> {
    if let Some(when) = &rule.when {
        let context = state.context;
        let now = state.now.clone();
        let mut resolve_qualifier = |id: &str| state.resolve(id);
        return when.evaluate_bool(context, None, &now, &mut resolve_qualifier);
    }
    Ok(matches!(rule.selection, RuntimeRuleSelection::Query(_)))
}

fn static_rule_selection_value(selection: &RuntimeRuleSelection) -> Option<&RuntimeSelectedValue> {
    match selection {
        RuntimeRuleSelection::Value(value) => Some(value),
        RuntimeRuleSelection::Query(_) => None,
    }
}

fn resolve_rule_selection(
    runtime: &RuntimePackage,
    state: &mut QualifierState<'_>,
    selection: &RuntimeRuleSelection,
) -> Result<RuntimeSelectedValue> {
    match selection {
        RuntimeRuleSelection::Value(value) => Ok(value.clone()),
        RuntimeRuleSelection::Query(query) => resolve_catalog_query(runtime, state, query),
    }
}

fn resolve_catalog_query(
    runtime: &RuntimePackage,
    state: &mut QualifierState<'_>,
    query: &RuntimeCatalogQuery,
) -> Result<RuntimeSelectedValue> {
    let entries = runtime
        .catalog_entries
        .get(&query.catalog)
        .ok_or_else(|| RototoError::new(format!("catalog has no entries: {}", query.catalog)))?;
    let mut names = Vec::new();
    let mut values = Vec::new();
    let now = state.now.clone();
    for (name, entry) in entries {
        let entry_view = catalog_entry_view(runtime, &query.catalog, name, entry);
        let context = state.context;
        let mut resolve_qualifier = |id: &str| state.resolve(id);
        if query.expression.evaluate_bool(
            context,
            Some(&entry_view),
            &now,
            &mut resolve_qualifier,
        )? {
            names.push(name.clone());
            values.push(entry_view);
        }
    }
    Ok(RuntimeSelectedValue::CatalogList {
        catalog: query.catalog.clone(),
        names,
        value: JsonValue::Array(values),
    })
}

fn catalog_entry_view(
    runtime: &RuntimePackage,
    catalog: &str,
    name: &str,
    entry: &JsonValue,
) -> JsonValue {
    let mut stack = BTreeSet::new();
    hydrate_catalog_entry(runtime, catalog, name, entry, &mut stack)
}

fn hydrate_catalog_entry(
    runtime: &RuntimePackage,
    catalog: &str,
    name: &str,
    entry: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> JsonValue {
    let mut value = entry.clone();
    if let JsonValue::Object(object) = &mut value {
        object.insert("id".to_owned(), JsonValue::String(name.to_owned()));
    }
    if !stack.insert((catalog.to_owned(), name.to_owned())) {
        return value;
    }
    let hydrated = runtime
        .catalog_schemas
        .get(catalog)
        .map(|schema| hydrate_schema_value(runtime, catalog, schema, &value, stack))
        .unwrap_or(value);
    stack.remove(&(catalog.to_owned(), name.to_owned()));
    hydrated
}

fn hydrate_schema_value(
    runtime: &RuntimePackage,
    schema_catalog: &str,
    schema: &JsonValue,
    value: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> JsonValue {
    if let Some(reference) = schema.get("$ref").and_then(JsonValue::as_str)
        && let Some((catalog, schema)) = resolve_schema_ref(runtime, schema_catalog, reference)
    {
        return hydrate_schema_value(runtime, &catalog, schema, value, stack);
    }

    if let Some(ref_value) = schema.get("x-rototo-catalog-ref")
        && let Some(hydrated) = hydrate_catalog_reference(runtime, ref_value, value, stack)
    {
        return hydrated;
    }

    let mut hydrated = value.clone();

    if let (Some(properties), JsonValue::Object(object)) = (
        schema.get("properties").and_then(JsonValue::as_object),
        &mut hydrated,
    ) {
        for (key, subschema) in properties {
            let Some(child) = object.get(key).cloned() else {
                continue;
            };
            object.insert(
                key.clone(),
                hydrate_schema_value(runtime, schema_catalog, subschema, &child, stack),
            );
        }
    }

    if let (Some(items), JsonValue::Array(values)) = (schema.get("items"), &mut hydrated) {
        for value in values {
            let child = value.clone();
            *value = hydrate_schema_value(runtime, schema_catalog, items, &child, stack);
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(schemas) = schema.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for subschema in schemas {
            hydrated = hydrate_schema_value(runtime, schema_catalog, subschema, &hydrated, stack);
        }
    }

    hydrated
}

fn hydrate_catalog_reference(
    runtime: &RuntimePackage,
    ref_spec: &JsonValue,
    value: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> Option<JsonValue> {
    if ref_spec == &JsonValue::Bool(true) {
        let object = value.as_object()?;
        let catalog = object.get("catalog")?.as_str()?;
        let entry = object.get("entry")?.as_str()?;
        let pointer = object.get("pointer").and_then(JsonValue::as_str);
        return hydrate_catalog_reference_target(runtime, catalog, entry, pointer, value, stack);
    }

    let target_catalogs = if let Some(catalog) = ref_spec.as_str() {
        vec![catalog]
    } else if let Some(catalogs) = ref_spec.as_array() {
        catalogs.iter().filter_map(JsonValue::as_str).collect()
    } else {
        return None;
    };

    let target = value.as_str()?;
    let (entry, pointer) = split_catalog_entry_ref(target)?;
    for catalog in target_catalogs {
        if runtime
            .catalog_entries
            .get(catalog)
            .is_some_and(|entries| entries.contains_key(entry))
        {
            return hydrate_catalog_reference_target(
                runtime, catalog, entry, pointer, value, stack,
            );
        }
    }
    None
}

fn hydrate_catalog_reference_target(
    runtime: &RuntimePackage,
    catalog: &str,
    entry: &str,
    pointer: Option<&str>,
    fallback: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> Option<JsonValue> {
    if stack.contains(&(catalog.to_owned(), entry.to_owned())) {
        return Some(fallback.clone());
    }
    let entry_value = runtime.catalog_entries.get(catalog)?.get(entry)?;
    let hydrated = hydrate_catalog_entry(runtime, catalog, entry, entry_value, stack);
    match pointer {
        Some(pointer) => hydrated
            .pointer(pointer)
            .cloned()
            .or_else(|| Some(fallback.clone())),
        None => Some(hydrated),
    }
}

fn split_catalog_entry_ref(value: &str) -> Option<(&str, Option<&str>)> {
    let Some((entry, pointer)) = value.split_once('#') else {
        return (!value.is_empty()).then_some((value, None));
    };
    if entry.is_empty() || !is_json_pointer(pointer) {
        return None;
    }
    Some((entry, (!pointer.is_empty()).then_some(pointer)))
}

fn is_json_pointer(value: &str) -> bool {
    value.is_empty() || value.starts_with('/')
}

fn resolve_schema_ref<'a>(
    runtime: &'a RuntimePackage,
    current_catalog: &str,
    reference: &str,
) -> Option<(String, &'a JsonValue)> {
    let (uri, pointer) = reference
        .split_once('#')
        .map_or((reference, ""), |(uri, pointer)| (uri, pointer));
    let catalog = if uri.is_empty() {
        current_catalog.to_owned()
    } else if let Some(catalog) = uri
        .strip_prefix("rototo://catalogs/")
        .and_then(|path| path.strip_suffix(".schema.json"))
    {
        catalog.to_owned()
    } else {
        runtime
            .catalog_schemas
            .iter()
            .find_map(|(catalog, schema)| {
                (schema.get("$id").and_then(JsonValue::as_str) == Some(uri))
                    .then(|| catalog.clone())
            })?
    };
    let schema = runtime.catalog_schemas.get(&catalog)?;
    if pointer.is_empty() {
        return Some((catalog, schema));
    }
    let pointer = if pointer.starts_with('/') {
        pointer
    } else {
        return None;
    };
    schema.pointer(pointer).map(|schema| (catalog, schema))
}

struct QualifierState<'a> {
    runtime: &'a RuntimePackage,
    context: &'a JsonValue,
    /// The evaluation timestamp exposed to expressions as `env.now`. Captured
    /// once when the resolution starts so every `env.now` in one resolution sees
    /// the same instant.
    now: String,
    cache: HashMap<String, bool>,
    resolving: HashSet<String>,
    traces: HashMap<String, QualifierResolutionTrace>,
}

impl<'a> QualifierState<'a> {
    fn new(runtime: &'a RuntimePackage, context: &'a JsonValue) -> Self {
        Self {
            runtime,
            context,
            now: crate::predicate::now_rfc3339(),
            cache: HashMap::new(),
            resolving: HashSet::new(),
            traces: HashMap::new(),
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
        let context = self.context;
        let now = self.now.clone();
        let mut resolve_qualifier = |qualifier_id: &str| self.resolve(qualifier_id);
        let value = qualifier
            .when
            .evaluate_bool(context, None, &now, &mut resolve_qualifier)?;
        self.traces.insert(
            id.to_owned(),
            QualifierResolutionTrace {
                id: id.to_owned(),
                when: qualifier.when.source().to_owned(),
                value,
            },
        );
        Ok(value)
    }

    fn qualifier_traces(&self) -> Vec<QualifierResolutionTrace> {
        let mut traces = self.traces.values().cloned().collect::<Vec<_>>();
        traces.sort_by(|left, right| left.id.cmp(&right.id));
        traces
    }

    fn take_qualifier_trace(&mut self, id: &str) -> Option<QualifierResolutionTrace> {
        self.traces.remove(id)
    }
}

pub(crate) fn bucket_value(salt: &str, value: &JsonValue) -> u16 {
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
        let package = package_with_qualifiers(&[
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
                resolve_qualifier(package.path(), id, &context)
                    .await
                    .unwrap(),
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
        ] {
            assert!(
                !resolve_qualifier(package.path(), id, &context)
                    .await
                    .unwrap(),
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn missing_context_paths_fail_resolution() {
        let package = package_with_qualifiers(&[
            (
                "missing-compare",
                predicate("missing.path", "neq", r#""anything""#),
            ),
            (
                "missing-bucket",
                bucket_predicate_for("missing.id", "0, 1000"),
            ),
            (
                "missing-after-false",
                r#"schema_version = 1
when = "context.user.tier == \"premium\" && context.missing.path == \"anything\""
"#
                .to_owned(),
            ),
        ]);
        let context = serde_json::json!({ "user": { "id": "user-123" } });
        let non_matching_context = serde_json::json!({
            "user": { "id": "user-123", "tier": "free" }
        });

        let err = resolve_qualifier(package.path(), "missing-compare", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No such key"));

        let err = resolve_qualifier(package.path(), "missing-bucket", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No such key"));

        assert!(
            !resolve_qualifier(package.path(), "missing-after-false", &non_matching_context,)
                .await
                .unwrap()
        );
        let err = resolve_qualifier(
            package.path(),
            "missing-after-false",
            &serde_json::json!({ "user": { "tier": "premium" } }),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("No such key"));
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
        let package = package_with_qualifiers(&[
            ("bucket-in", bucket_predicate("9913, 9914")),
            ("bucket-start-exclusive", bucket_predicate("9914, 9915")),
            ("bucket-end-exclusive", bucket_predicate("9912, 9913")),
        ]);
        let context = serde_json::json!({ "user": { "id": "user-123" } });

        assert!(
            resolve_qualifier(package.path(), "bucket-in", &context)
                .await
                .unwrap()
        );
        assert!(
            !resolve_qualifier(package.path(), "bucket-start-exclusive", &context)
                .await
                .unwrap()
        );
        assert!(
            !resolve_qualifier(package.path(), "bucket-end-exclusive", &context)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn resolves_expanded_predicate_operators() {
        let package = package_with_qualifiers(&[
            (
                "prefix-true",
                predicate("user.email", "prefix", r#""ava@""#),
            ),
            (
                "prefix-false",
                predicate("user.email", "prefix", r#""sam@""#),
            ),
            (
                "suffix-true",
                predicate("user.email", "suffix", r#""example.com""#),
            ),
            (
                "contains-true",
                predicate("user.email", "contains", r#""@example.""#),
            ),
            (
                "regex-true",
                predicate("user.email", "regex", r#""^ava@.*\\.com$""#),
            ),
            (
                "glob-true",
                predicate("user.email", "glob", r#""ava@*.com""#),
            ),
            (
                "semver-true",
                predicate("app.version", "semver", r#"">=1.7.0, <2.0.0""#),
            ),
            (
                "semver-false",
                predicate("app.version", "semver", r#"">=2.0.0""#),
            ),
            (
                "time-gt-true",
                predicate("request.time", "time_gt", r#""2026-07-10T09:00:00Z""#),
            ),
            (
                "time-gt-false",
                predicate("request.time", "time_gt", r#""2026-07-10T11:00:00Z""#),
            ),
            (
                "time-between-true",
                condition(
                    r#"time_between(context.request.time, "2026-07-10T10:00:00Z", "2026-07-10T11:00:00Z")"#,
                ),
            ),
            (
                "time-between-false",
                condition(
                    r#"time_between(context.request.time, "2026-07-10T09:00:00Z", "2026-07-10T10:30:00Z")"#,
                ),
            ),
            (
                "between-true",
                condition("context.cart.total >= 40 && context.cart.total < 50"),
            ),
            (
                "between-false",
                condition("context.cart.total >= 0 && context.cart.total < 42"),
            ),
            (
                "contains-any-true",
                predicate("user.roles", "contains_any", r#"["admin", "owner"]"#),
            ),
            (
                "contains-all-true",
                predicate("user.roles", "contains_all", r#"["admin", "billing"]"#),
            ),
            (
                "contains-none-true",
                predicate("user.roles", "contains_none", r#"["owner"]"#),
            ),
            (
                "contains-none-false",
                predicate("user.roles", "contains_none", r#"["admin"]"#),
            ),
            (
                "cidr-true",
                predicate("request.ip", "cidr", r#""10.0.0.0/8""#),
            ),
            (
                "cidr-false",
                predicate("request.ip", "cidr", r#""192.168.0.0/16""#),
            ),
            (
                "exists-true",
                predicate_without_value("user.nickname", "exists"),
            ),
            (
                "exists-false",
                predicate_without_value("user.missing", "exists"),
            ),
            (
                "missing-true",
                predicate_without_value("user.missing", "missing"),
            ),
            (
                "is-null-true",
                predicate_without_value("user.nickname", "is_null"),
            ),
            (
                "not-null-true",
                predicate_without_value("user.email", "not_null"),
            ),
            (
                "not-true",
                negated_predicate("user.email", "prefix", r#""sam@""#),
            ),
            (
                "not-false",
                negated_predicate("user.email", "prefix", r#""ava@""#),
            ),
        ]);
        let context = serde_json::json!({
            "app": { "version": "1.7.3" },
            "cart": { "total": 42 },
            "request": {
                "ip": "10.2.3.4",
                "time": "2026-07-10T12:30:00+02:00"
            },
            "user": {
                "email": "ava@example.com",
                "nickname": null,
                "roles": ["admin", "billing"]
            }
        });

        for id in [
            "prefix-true",
            "suffix-true",
            "contains-true",
            "regex-true",
            "glob-true",
            "semver-true",
            "time-gt-true",
            "time-between-true",
            "between-true",
            "contains-any-true",
            "contains-all-true",
            "contains-none-true",
            "cidr-true",
            "exists-true",
            "missing-true",
            "is-null-true",
            "not-null-true",
            "not-true",
        ] {
            assert!(
                resolve_qualifier(package.path(), id, &context)
                    .await
                    .unwrap(),
                "{id}"
            );
        }

        for id in [
            "prefix-false",
            "semver-false",
            "time-gt-false",
            "time-between-false",
            "between-false",
            "contains-none-false",
            "cidr-false",
            "exists-false",
            "not-false",
        ] {
            assert!(
                !resolve_qualifier(package.path(), id, &context)
                    .await
                    .unwrap(),
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn resolves_qualifier_indirection_and_cycles() {
        let package = package_with_qualifiers(&[
            ("premium", predicate("user.tier", "eq", r#""premium""#)),
            ("free", predicate("user.tier", "eq", r#""free""#)),
            ("premium-derived", condition(r#"env.qualifier["premium"]"#)),
            ("free-derived", condition(r#"env.qualifier["free"]"#)),
            ("cycle-a", condition(r#"env.qualifier["cycle-b"]"#)),
            ("cycle-b", condition(r#"env.qualifier["cycle-a"]"#)),
        ]);
        let context = serde_json::json!({ "user": { "tier": "premium" } });

        assert!(
            resolve_qualifier(package.path(), "premium-derived", &context)
                .await
                .unwrap()
        );
        assert!(
            !resolve_qualifier(package.path(), "free-derived", &context)
                .await
                .unwrap()
        );
        let err = resolve_qualifier(package.path(), "cycle-a", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("qualifier cycle detected"));
    }

    #[tokio::test]
    async fn resolves_variable_default_and_fails_closed() {
        let package =
            package_with_qualifiers(&[("premium", predicate("user.tier", "eq", r#""premium""#))]);
        std::fs::create_dir_all(package.path().join("variables")).unwrap();
        std::fs::write(
            package.path().join("variables/message.toml"),
            r#"schema_version = 1
type = "string"

[resolve]
default = "control"

[[resolve.rule]]
when = 'env.qualifier["premium"]'
value = "premium"
"#,
        )
        .unwrap();
        let context = serde_json::json!({ "user": { "tier": "free" } });

        let fallback = resolve_variable(package.path(), "message", &context)
            .await
            .unwrap();
        assert_eq!(fallback.value, serde_json::json!("control"));

        std::fs::write(
            package.path().join("variables/bad-rule.toml"),
            r#"schema_version = 1
type = "string"

[resolve]
default = "control"
rule = ["not-a-table"]
"#,
        )
        .unwrap();
        let err = resolve_variable(package.path(), "bad-rule", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("rule must be a table"));
    }

    #[tokio::test]
    async fn numeric_equality_is_exact_without_lossy_large_integer_casts() {
        let package = package_with_qualifiers(&[
            // cel uses IEEE-754 semantics for int/double comparison, so we no
            // longer assert exact large-int-vs-float inequality (cel casts the
            // int to f64 first). Equality and exact int-vs-int equality hold.
            ("int-float-equal", predicate("n", "eq", "100.0")),
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
            resolve_qualifier(package.path(), "int-float-equal", &context)
                .await
                .unwrap()
        );
        assert!(
            resolve_qualifier(package.path(), "large-int-self-equal", &context)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn resolves_when_qualifiers_and_catalog_query_variables() {
        let package = package_with_qualifiers(&[(
            "premium",
            r#"schema_version = 1
when = "context.user.tier == \"premium\""
"#
            .to_owned(),
        )]);
        std::fs::create_dir_all(package.path().join("catalogs/message-template-entries")).unwrap();
        std::fs::create_dir_all(package.path().join("catalogs/hero-banner-entries")).unwrap();
        std::fs::create_dir_all(package.path().join("catalogs/page-entries")).unwrap();
        std::fs::create_dir_all(package.path().join("variables")).unwrap();
        std::fs::write(
            package.path().join("catalogs/message-template.schema.json"),
            r#"{
  "type": "object",
  "required": ["channel", "active", "body"],
  "properties": {
    "channel": { "type": "string" },
    "active": { "type": "boolean" },
    "body": { "type": "string" }
  }
}
"#,
        )
        .unwrap();
        std::fs::write(
            package
                .path()
                .join("catalogs/message-template-entries/email.toml"),
            r#"channel = "email"
active = true
body = "Email body"
"#,
        )
        .unwrap();
        std::fs::write(
            package
                .path()
                .join("catalogs/message-template-entries/sms.toml"),
            r#"channel = "sms"
active = false
body = "SMS body"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/templates.toml"),
            r#"schema_version = 1
type = "list<catalog:message-template>"

[resolve]
default = []

[[resolve.rule]]
query = "entry.channel == context.channel && entry.active == true && env.qualifier[\"premium\"]"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("catalogs/hero-banner.schema.json"),
            r#"{
  "type": "object",
  "required": ["cta"],
  "properties": {
    "cta": { "type": "string" }
  }
}
"#,
        )
        .unwrap();
        std::fs::write(
            package
                .path()
                .join("catalogs/hero-banner-entries/home.toml"),
            r#"cta = "Buy"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("catalogs/page.schema.json"),
            r#"{
  "type": "object",
  "required": ["hero", "title"],
  "properties": {
    "hero": {
      "type": "string",
      "x-rototo-catalog-ref": "hero-banner"
    },
    "title": { "type": "string" }
  }
}
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("catalogs/page-entries/home.toml"),
            r#"hero = "home"
title = "Home"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/pages.toml"),
            r#"schema_version = 1
type = "list<catalog:page>"

[resolve]
default = []

[[resolve.rule]]
query = "entry.hero.cta == \"Buy\""
"#,
        )
        .unwrap();

        let context = serde_json::json!({
            "channel": "email",
            "user": { "tier": "premium" }
        });

        assert!(
            resolve_qualifier(package.path(), "premium", &context)
                .await
                .unwrap()
        );
        let resolution = resolve_variable(package.path(), "templates", &context)
            .await
            .unwrap();
        assert_eq!(
            resolution.value,
            serde_json::json!([
                {
                    "id": "email",
                    "channel": "email",
                    "active": true,
                    "body": "Email body"
                }
            ])
        );

        let pages = resolve_variable(package.path(), "pages", &context)
            .await
            .unwrap();
        assert_eq!(
            pages.value,
            serde_json::json!([
                {
                    "id": "home",
                    "title": "Home",
                    "hero": {
                        "id": "home",
                        "cta": "Buy"
                    }
                }
            ])
        );
    }

    #[tokio::test]
    async fn malformed_qualifier_conditions_return_errors_during_unchecked_resolution() {
        let context = serde_json::json!({ "user": { "tier": "premium", "id": "user-123" } });

        let package = package_with_qualifiers(&[(
            "unknown-function",
            condition(r#"not_a_real_function(context.user.tier, "premium")"#),
        )]);
        let err = resolve_qualifier(package.path(), "unknown-function", &context)
            .await
            .unwrap_err();
        // The unknown function fails during evaluation; the message is cel's.
        assert!(!err.to_string().is_empty());

        let package =
            package_with_qualifiers(&[("missing-when", String::from("schema_version = 1\n"))]);
        let err = resolve_qualifier(package.path(), "missing-when", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("qualifier must declare when"));

        let package = package_with_qualifiers(&[("non-bool", condition("context.user.tier"))]);
        let err = resolve_qualifier(package.path(), "non-bool", &context)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("expression did not evaluate to bool")
        );
    }

    fn package_with_qualifiers(qualifiers: &[(&str, String)]) -> tempfile::TempDir {
        let package = tempfile::TempDir::new().unwrap();
        std::fs::write(
            package.path().join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .unwrap();
        std::fs::create_dir_all(package.path().join("qualifiers")).unwrap();
        for (id, contents) in qualifiers {
            std::fs::write(
                package.path().join(format!("qualifiers/{id}.toml")),
                contents,
            )
            .unwrap();
        }
        package
    }

    fn predicate(attribute: &str, op: &str, value: &str) -> String {
        condition(&predicate_expression(attribute, op, value))
    }

    fn predicate_without_value(attribute: &str, op: &str) -> String {
        condition(&predicate_without_value_expression(attribute, op))
    }

    fn negated_predicate(attribute: &str, op: &str, value: &str) -> String {
        condition(&format!(
            "!({})",
            predicate_expression(attribute, op, value)
        ))
    }

    fn bucket_predicate(range: &str) -> String {
        bucket_predicate_for("user.id", range)
    }

    fn bucket_predicate_for(attribute: &str, range: &str) -> String {
        let [start, end] = parse_range(range);
        condition(&format!(
            "bucket({}, \"known-salt\", {start}, {end})",
            attribute_expression(attribute)
        ))
    }

    fn condition(expression: &str) -> String {
        let escaped = expression.replace('\\', "\\\\").replace('"', "\\\"");
        format!("schema_version = 1\nwhen = \"{escaped}\"\n")
    }

    fn predicate_expression(attribute: &str, op: &str, value: &str) -> String {
        let actual = attribute_expression(attribute);
        match op {
            "eq" => format!("{actual} == {value}"),
            "neq" => format!("{actual} != {value}"),
            "in" => format!("{actual} in {value}"),
            "not_in" => format!("!({actual} in {value})"),
            "gt" => format!("{actual} > {value}"),
            "gte" => format!("{actual} >= {value}"),
            "lt" => format!("{actual} < {value}"),
            "lte" => format!("{actual} <= {value}"),
            "prefix" => format!("prefix({actual}, {value})"),
            "suffix" => format!("suffix({actual}, {value})"),
            "contains" => format!("contains({actual}, {value})"),
            "regex" => format!("regex({actual}, {value})"),
            "glob" => format!("glob({actual}, {value})"),
            "semver" => format!("semver({actual}, {value})"),
            "time_gt" => format!("time_after({actual}, {value})"),
            "time_gte" => format!("time_at_or_after({actual}, {value})"),
            "time_lt" => format!("time_before({actual}, {value})"),
            "time_lte" => format!("time_at_or_before({actual}, {value})"),
            "contains_any" => contains_expression(&actual, value, "||", false),
            "contains_all" => contains_expression(&actual, value, "&&", false),
            "contains_none" => contains_expression(&actual, value, "&&", true),
            "cidr" => format!("cidr({actual}, {value})"),
            _ => panic!("unsupported test operator: {op}"),
        }
    }

    fn predicate_without_value_expression(attribute: &str, op: &str) -> String {
        let actual = attribute_expression(attribute);
        match op {
            "exists" => format!("has({actual})"),
            "missing" => format!("!has({actual})"),
            "is_null" => format!("has({actual}) && {actual} == null"),
            "not_null" => format!("has({actual}) && {actual} != null"),
            _ => panic!("unsupported test operator: {op}"),
        }
    }

    fn attribute_expression(attribute: &str) -> String {
        if let Some(qualifier) = attribute.strip_prefix("qualifier.") {
            format!("env.qualifier[\"{qualifier}\"]")
        } else {
            format!("context.{attribute}")
        }
    }

    fn contains_expression(actual: &str, value: &str, join: &str, negate: bool) -> String {
        let values = serde_json::from_str::<serde_json::Value>(value).unwrap();
        let values = values.as_array().unwrap();
        values
            .iter()
            .map(|value| {
                let call = format!("contains({actual}, {})", value);
                if negate { format!("!{call}") } else { call }
            })
            .collect::<Vec<_>>()
            .join(&format!(" {join} "))
    }

    fn parse_range(range: &str) -> [i64; 2] {
        let parts = range
            .split(',')
            .map(|part| part.trim().parse::<i64>().unwrap())
            .collect::<Vec<_>>();
        [parts[0], parts[1]]
    }
}
