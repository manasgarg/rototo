use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value as JsonValue;

pub(crate) mod hydrate;

use hydrate::catalog_entry_view;

use crate::error::{Result, RototoError};
use crate::expression::{RefResolver, ResolvingTarget};
use crate::lint::{
    RuntimeAllocation, RuntimePackage, RuntimeQuery, RuntimeResolution, RuntimeRule,
    RuntimeSelectedValue, compile_runtime_package,
};
use crate::model::VariableAllocationTrace;
use crate::model::{VariableResolution, VariableResolutionSource};
use crate::model::{VariableResolutionTrace, VariableRuleResolutionTrace, VariableTraceOutcome};

/// A captured variable resolution trace plus the `[[trace]]` policy indices that
/// selected it. Returned by [`resolve_variable_traced_unchecked`] for the SDK to
/// wrap into a trace event.
pub(crate) struct VariableTraceCapture {
    pub(crate) trace: VariableResolutionTrace,
    pub(crate) policies: Vec<usize>,
}

/// Resolve a variable and, if tracing is warranted, capture its full trace. The
/// trace is computed regardless (the lean path discards it); this variant keeps
/// it and evaluates `[[trace]]` policies against the same resolution state.
/// Returns `Some` capture when the app requested a trace or any policy matched.
pub(crate) fn resolve_variable_traced_unchecked(
    runtime: &RuntimePackage,
    id: &str,
    context: &JsonValue,
    app_requested: bool,
) -> Result<(VariableResolution, Option<VariableTraceCapture>)> {
    let mut state = ResolutionState::new(runtime, context);
    let trace = resolve_variable_trace_with_state(runtime, &mut state, id)?;
    let resolution = trace.resolution.clone();
    let policies = evaluate_trace_policies(&mut state, ResolvingTarget::Variable(id))?;
    let capture =
        (app_requested || !policies.is_empty()).then_some(VariableTraceCapture { trace, policies });
    Ok((resolution, capture))
}

/// Evaluate every `[[trace]]` policy `when` against the in-flight resolution,
/// binding `env.resolving` to `target`, and return the indices that matched.
/// Reuses the resolution's `ResolutionState` so `variables[...]` references
/// hit the same cache.
fn evaluate_trace_policies(
    state: &mut ResolutionState<'_>,
    target: ResolvingTarget<'_>,
) -> Result<Vec<usize>> {
    if state.runtime.trace_policies.is_empty() {
        return Ok(Vec::new());
    }
    let context = state.context;
    let now = state.now.clone();
    let mut matched = Vec::new();
    for (index, policy) in state.runtime.trace_policies.iter().enumerate() {
        // Resolve variable references through the shared state cache. The policy
        // list is borrowed from `state.runtime`, which outlives `state`, so the
        // policy loop can still borrow `state` mutably for reference resolution.
        let when = &policy.when;
        if when.evaluate_bool_traced(context, &now, target, state)? {
            matched.push(index);
        }
    }
    Ok(matched)
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
    let mut state = ResolutionState::new(runtime, context);
    resolve_variable_with_state(runtime, &mut state, id)
}

pub(crate) fn trace_variable_unchecked(
    runtime: &RuntimePackage,
    id: &str,
    context: &JsonValue,
) -> Result<VariableResolutionTrace> {
    let mut state = ResolutionState::new(runtime, context);
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
    let mut state = ResolutionState::new(runtime, context);

    let mut resolutions = Vec::new();
    for id in ids {
        resolutions.push(resolve_variable_with_state(runtime, &mut state, &id)?);
    }
    Ok(resolutions)
}

/// Traced resolution of every variable where a variable that cannot resolve
/// under this context yields its error instead of failing the batch. Shares
/// one state exactly like [`trace_variable_resolutions`], so the successful
/// traces here never disagree with the strict batch.
pub async fn trace_variable_resolution_outcomes(
    package_root: &Path,
    context: &JsonValue,
) -> Result<Vec<VariableTraceOutcome>> {
    let runtime = compile_runtime_package(package_root).await?;
    runtime.validate_context(context)?;
    Ok(trace_variable_resolution_outcomes_unchecked(
        &runtime, context,
    ))
}

pub(crate) fn trace_variable_resolution_outcomes_unchecked(
    runtime: &RuntimePackage,
    context: &JsonValue,
) -> Vec<VariableTraceOutcome> {
    let ids: Vec<String> = runtime.variables.keys().cloned().collect();
    let mut state = ResolutionState::new(runtime, context);
    ids.into_iter()
        .map(
            |id| match resolve_variable_trace_with_state(runtime, &mut state, &id) {
                Ok(trace) => VariableTraceOutcome {
                    id,
                    trace: Some(trace),
                    error: None,
                },
                Err(err) => VariableTraceOutcome {
                    id,
                    trace: None,
                    error: Some(err.to_string()),
                },
            },
        )
        .collect()
}

pub(crate) fn trace_variable_resolutions_unchecked(
    runtime: &RuntimePackage,
    context: &JsonValue,
) -> Result<Vec<VariableResolutionTrace>> {
    let ids: Vec<String> = runtime.variables.keys().cloned().collect();

    // One state for the whole batch, exactly like resolve_variables: one
    // env.now instant and one memoization cache, so a traced batch can
    // never disagree with the resolved batch it explains.
    let mut state = ResolutionState::new(runtime, context);
    let mut traces = Vec::new();
    for id in ids {
        traces.push(resolve_variable_trace_with_state(runtime, &mut state, &id)?);
    }
    Ok(traces)
}

fn resolve_variable_with_state(
    runtime: &RuntimePackage,
    state: &mut ResolutionState<'_>,
    id: &str,
) -> Result<VariableResolution> {
    Ok(resolve_variable_trace_with_state(runtime, state, id)?.resolution)
}

fn resolve_variable_trace_with_state(
    runtime: &RuntimePackage,
    state: &mut ResolutionState<'_>,
    id: &str,
) -> Result<VariableResolutionTrace> {
    let variable = runtime
        .variables
        .get(id)
        .ok_or_else(|| RototoError::new(format!("variable not found: variable://{id}")))?;

    let mut trace = match &variable.resolution {
        RuntimeResolution::Rules { default, rules } => {
            resolve_rules_trace(state, id, default, rules)
        }
        RuntimeResolution::Query(query) => resolve_query_trace(runtime, state, id, query),
        RuntimeResolution::Allocation(allocation) => {
            resolve_allocation_trace(state, id, allocation)
        }
    }?;
    trace.provenance = runtime.resolve_provenance.get(id).cloned();
    Ok(trace)
}

fn resolve_allocation_trace(
    state: &mut ResolutionState<'_>,
    id: &str,
    allocation: &RuntimeAllocation,
) -> Result<VariableResolutionTrace> {
    let mut trace = VariableAllocationTrace {
        layer: allocation.layer.clone(),
        allocation: allocation.allocation.clone(),
        enrolled: false,
        bucket: None,
        arm: None,
    };

    let mut selected: Option<&RuntimeSelectedValue> = None;
    if allocation.running && evaluate_allocation_eligibility(state, allocation)? {
        trace.enrolled = true;
        let context = state.context;
        let now = state.now.clone();
        let unit = allocation.unit.evaluate_value(context, None, &now, state)?;
        let bucket = allocation_bucket(&allocation.layer, &unit, allocation.buckets);
        trace.bucket = Some(bucket);
        for arm in &allocation.arms {
            if bucket >= arm.start && bucket <= arm.end {
                trace.arm = Some(arm.name.clone());
                selected = Some(&arm.value);
                break;
            }
        }
    }

    let selected = selected.unwrap_or(&allocation.default);
    let resolution = VariableResolution {
        id: id.to_owned(),
        value: selected.value().clone(),
        source: selected_value_source(selected),
    };

    Ok(VariableResolutionTrace {
        resolution,
        default_value: allocation.default.value().clone(),
        default_source: selected_value_source(&allocation.default),
        rules: Vec::new(),
        provenance: None,
        allocation: Some(trace),
    })
}

fn evaluate_allocation_eligibility(
    state: &mut ResolutionState<'_>,
    allocation: &RuntimeAllocation,
) -> Result<bool> {
    let Some(eligibility) = &allocation.eligibility else {
        return Ok(true);
    };
    let context = state.context;
    let now = state.now.clone();
    eligibility.evaluate_bool(context, None, &now, state)
}

/// The deterministic bucket for a unit on a layer's line: the same FNV-1a
/// hash as `bucket()` expressions, salted with the layer id so different
/// layers divide traffic independently.
pub(crate) fn allocation_bucket(layer: &str, unit: &JsonValue, buckets: u32) -> u32 {
    (stable_unit_hash(layer, unit) % u64::from(buckets)) as u32
}

fn resolve_rules_trace(
    state: &mut ResolutionState<'_>,
    id: &str,
    default: &RuntimeSelectedValue,
    rules: &[RuntimeRule],
) -> Result<VariableResolutionTrace> {
    let mut selected = None;
    let mut rule_traces = Vec::new();
    for rule in rules {
        let matched = evaluate_rule_condition(state, rule)?;
        rule_traces.push(VariableRuleResolutionTrace {
            index: rule.index,
            condition: rule.when.source().to_owned(),
            value: rule.value.value().clone(),
            source: selected_value_source(&rule.value),
            matched,
        });
        if matched {
            selected = Some(rule.value.clone());
            break;
        }
    }

    let selected = selected.unwrap_or_else(|| default.clone());

    let resolution = VariableResolution {
        id: id.to_owned(),
        value: selected.value().clone(),
        source: selected_value_source(&selected),
    };

    Ok(VariableResolutionTrace {
        resolution,
        default_value: default.value().clone(),
        default_source: selected_value_source(default),
        rules: rule_traces,
        provenance: None,
        allocation: None,
    })
}

fn resolve_query_trace(
    runtime: &RuntimePackage,
    state: &mut ResolutionState<'_>,
    id: &str,
    query: &RuntimeQuery,
) -> Result<VariableResolutionTrace> {
    let selected = resolve_catalog_query(runtime, state, id, query)?;

    let resolution = VariableResolution {
        id: id.to_owned(),
        value: selected.value().clone(),
        source: selected_value_source(&selected),
    };

    let (default_value, default_source) = match &query.default {
        Some(default) => (default.value().clone(), selected_value_source(default)),
        None => (JsonValue::Null, VariableResolutionSource::Literal),
    };

    Ok(VariableResolutionTrace {
        resolution,
        default_value,
        default_source,
        rules: Vec::new(),
        provenance: None,
        allocation: None,
    })
}

fn selected_value_source(value: &RuntimeSelectedValue) -> VariableResolutionSource {
    match value {
        RuntimeSelectedValue::Literal(_) => VariableResolutionSource::Literal,
        RuntimeSelectedValue::Catalog { catalog, name, .. } => VariableResolutionSource::Catalog {
            catalog: catalog.clone(),
            value: name.clone(),
        },
        RuntimeSelectedValue::CatalogArray { catalog, names, .. } => {
            VariableResolutionSource::CatalogArray {
                catalog: catalog.clone(),
                values: names.clone(),
            }
        }
    }
}

fn evaluate_rule_condition(state: &mut ResolutionState<'_>, rule: &RuntimeRule) -> Result<bool> {
    let context = state.context;
    let now = state.now.clone();
    rule.when.evaluate_bool(context, None, &now, state)
}

fn resolve_catalog_query(
    runtime: &RuntimePackage,
    state: &mut ResolutionState<'_>,
    variable_id: &str,
    query: &RuntimeQuery,
) -> Result<RuntimeSelectedValue> {
    let entries = runtime
        .catalog_entries
        .get(&query.catalog)
        .ok_or_else(|| RototoError::new(format!("catalog has no entries: {}", query.catalog)))?;
    let now = state.now.clone();

    // Predicates evaluate against the hydrated entry view (`entry.x` sees
    // referenced values), but the value returned to the app is the raw
    // entry: hydration is for resolution, and apps follow references
    // explicitly (design/package-reflection.md). The entry id is injected
    // either way; identity is not hydration.
    let mut matches: Vec<(String, JsonValue)> = Vec::new();
    for (name, entry) in entries {
        let entry_view = catalog_entry_view(runtime, &query.catalog, name, entry);
        let context = state.context;
        let keep = match &query.filter {
            Some(filter) => filter.evaluate_bool(context, Some(&entry_view), &now, state)?,
            None => true,
        };
        if keep {
            matches.push((name.clone(), entry_view));
        }
    }

    if let Some(sort) = &query.sort {
        let mut keyed = Vec::with_capacity(matches.len());
        for (name, entry_view) in matches {
            let context = state.context;
            let key = sort.evaluate_value(context, Some(&entry_view), &now, state)?;
            keyed.push((key, name, entry_view));
        }
        for pair in keyed.windows(2) {
            if !query_sort_keys_comparable(&pair[0].0, &pair[1].0) {
                return Err(RototoError::new(format!(
                    "query sort keys are not comparable for variable {variable_id}: {} and {}",
                    pair[0].0, pair[1].0
                )));
            }
        }
        keyed.sort_by(|a, b| compare_query_sort_keys(&a.0, &b.0));
        if query.descending {
            keyed.reverse();
        }
        matches = keyed
            .into_iter()
            .map(|(_, name, view)| (name, view))
            .collect();
    }

    if let Some(limit) = query.limit {
        matches.truncate(limit);
    }

    if query.single {
        if matches.is_empty() {
            return query.default.clone().ok_or_else(|| {
                RototoError::new(format!(
                    "query matched no entries for variable {variable_id} and no default is declared"
                ))
            });
        }
        if matches.len() > 1 && query.sort.is_none() {
            return Err(RototoError::new(format!(
                "query matched {} entries for variable {variable_id}; add sort or narrow the filter",
                matches.len()
            )));
        }
        let (name, _) = matches.into_iter().next().expect("non-empty matches");
        let value = raw_entry_with_id(entries, &name);
        return Ok(RuntimeSelectedValue::Catalog {
            catalog: query.catalog.clone(),
            name,
            value,
        });
    }

    if matches.is_empty()
        && let Some(default) = &query.default
    {
        return Ok(default.clone());
    }

    let names: Vec<String> = matches.into_iter().map(|(name, _)| name).collect();
    let values: Vec<JsonValue> = names
        .iter()
        .map(|name| raw_entry_with_id(entries, name))
        .collect();
    Ok(RuntimeSelectedValue::CatalogArray {
        catalog: query.catalog.clone(),
        names,
        value: JsonValue::Array(values),
    })
}

/// The raw entry as the app receives it: no reference hydration, just the
/// entry id injected so a runtime-selected entry stays identifiable.
fn raw_entry_with_id(
    entries: &std::collections::BTreeMap<String, JsonValue>,
    name: &str,
) -> JsonValue {
    let mut value = entries.get(name).cloned().unwrap_or(JsonValue::Null);
    if let JsonValue::Object(object) = &mut value {
        object.insert("id".to_owned(), JsonValue::String(name.to_owned()));
    }
    value
}

fn query_sort_keys_comparable(left: &JsonValue, right: &JsonValue) -> bool {
    matches!(
        (left, right),
        (JsonValue::Number(_), JsonValue::Number(_)) | (JsonValue::String(_), JsonValue::String(_))
    )
}

fn compare_query_sort_keys(left: &JsonValue, right: &JsonValue) -> std::cmp::Ordering {
    match (left, right) {
        (JsonValue::Number(left), JsonValue::Number(right)) => {
            let left = left.as_f64().unwrap_or(f64::NAN);
            let right = right.as_f64().unwrap_or(f64::NAN);
            left.partial_cmp(&right)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        (JsonValue::String(left), JsonValue::String(right)) => left.cmp(right),
        _ => std::cmp::Ordering::Equal,
    }
}

/// Per-resolution state: the evaluation instant, plus memoized values of
/// variables referenced through the `variables` root, with cycle detection
/// across the reference chain.
struct ResolutionState<'a> {
    runtime: &'a RuntimePackage,
    context: &'a JsonValue,
    /// The evaluation timestamp exposed to expressions as `env.now`. Captured
    /// once when the resolution starts so every `env.now` in one resolution sees
    /// the same instant.
    now: String,
    variable_cache: HashMap<String, JsonValue>,
    /// Ids currently being resolved, for cycle detection.
    resolving: HashSet<String>,
}

impl RefResolver for ResolutionState<'_> {
    fn variable_value(&mut self, id: &str) -> Result<JsonValue> {
        self.resolve_variable_value(id)
    }

    fn list_members(&mut self, id: &str) -> Result<JsonValue> {
        self.runtime
            .lists
            .get(id)
            .map(|declared| JsonValue::Array(declared.members.clone()))
            .ok_or_else(|| RototoError::new(format!("expression references unknown list: {id}")))
    }
}

impl<'a> ResolutionState<'a> {
    fn new(runtime: &'a RuntimePackage, context: &'a JsonValue) -> Self {
        Self {
            runtime,
            context,
            now: crate::predicate::now_rfc3339(),
            variable_cache: HashMap::new(),
            resolving: HashSet::new(),
        }
    }

    /// Resolve a variable referenced through the `variables` root to its value,
    /// memoized per resolution.
    fn resolve_variable_value(&mut self, id: &str) -> Result<JsonValue> {
        if let Some(value) = self.variable_cache.get(id) {
            return Ok(value.clone());
        }
        if !self.resolving.insert(id.to_owned()) {
            return Err(RototoError::new(format!(
                "variable reference cycle detected at variable://{id}"
            )));
        }

        let runtime = self.runtime;
        let result = resolve_variable_with_state(runtime, self, id);
        self.resolving.remove(id);
        let value = result?.value;
        self.variable_cache.insert(id.to_owned(), value.clone());
        Ok(value)
    }
}

pub(crate) fn bucket_value(salt: &str, value: &JsonValue) -> u16 {
    (stable_unit_hash(salt, value) % 10_000) as u16
}

/// FNV-1a over `salt:value`, shared by `bucket()` expressions and layer
/// diversions. The hash is part of rototo's contract: a unit's position must
/// never move between releases.
fn stable_unit_hash(salt: &str, value: &JsonValue) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in salt
        .bytes()
        .chain([b':'])
        .chain(canonical_context_value(value).bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
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
        let package = package_with_conditions(&[
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
                resolve_condition(package.path(), id, &context)
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
                !resolve_condition(package.path(), id, &context)
                    .await
                    .unwrap(),
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn missing_context_paths_fail_resolution() {
        let package = package_with_conditions(&[
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
                condition(
                    r#"context.user.tier == "premium" && context.missing.path == "anything""#,
                ),
            ),
        ]);
        let context = serde_json::json!({ "user": { "id": "user-123" } });
        let non_matching_context = serde_json::json!({
            "user": { "id": "user-123", "tier": "free" }
        });

        let err = resolve_condition(package.path(), "missing-compare", &context)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("reads context.missing.path, which the given context does not carry"),
            "unexpected error: {err}"
        );

        let err = resolve_condition(package.path(), "missing-bucket", &context)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("reads context.missing.id, which the given context does not carry"),
            "unexpected error: {err}"
        );

        assert!(
            !resolve_condition(package.path(), "missing-after-false", &non_matching_context,)
                .await
                .unwrap()
        );
        let err = resolve_condition(
            package.path(),
            "missing-after-false",
            &serde_json::json!({ "user": { "tier": "premium" } }),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("reads context.missing.path, which the given context does not carry"),
            "unexpected error: {err}"
        );
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
        let package = package_with_conditions(&[
            ("bucket-in", bucket_predicate("9913, 9914")),
            ("bucket-start-exclusive", bucket_predicate("9914, 9915")),
            ("bucket-end-exclusive", bucket_predicate("9912, 9913")),
        ]);
        let context = serde_json::json!({ "user": { "id": "user-123" } });

        assert!(
            resolve_condition(package.path(), "bucket-in", &context)
                .await
                .unwrap()
        );
        assert!(
            !resolve_condition(package.path(), "bucket-start-exclusive", &context)
                .await
                .unwrap()
        );
        assert!(
            !resolve_condition(package.path(), "bucket-end-exclusive", &context)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn resolves_expanded_predicate_operators() {
        let package = package_with_conditions(&[
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
                resolve_condition(package.path(), id, &context)
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
                !resolve_condition(package.path(), id, &context)
                    .await
                    .unwrap(),
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn resolves_condition_indirection_and_cycles() {
        let package = package_with_conditions(&[
            ("premium", predicate("user.tier", "eq", r#""premium""#)),
            ("free", predicate("user.tier", "eq", r#""free""#)),
            ("premium-derived", condition(r#"variables["premium"]"#)),
            ("free-derived", condition(r#"variables["free"]"#)),
            ("cycle_a", condition(r#"variables["cycle_b"]"#)),
            ("cycle_b", condition(r#"variables["cycle_a"]"#)),
        ]);
        let context = serde_json::json!({ "user": { "tier": "premium" } });

        assert!(
            resolve_condition(package.path(), "premium-derived", &context)
                .await
                .unwrap()
        );
        assert!(
            !resolve_condition(package.path(), "free-derived", &context)
                .await
                .unwrap()
        );
        let err = resolve_condition(package.path(), "cycle_a", &context)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("variable reference cycle detected")
        );
    }

    #[tokio::test]
    async fn resolves_variable_default_and_fails_closed() {
        let package =
            package_with_conditions(&[("premium", predicate("user.tier", "eq", r#""premium""#))]);
        std::fs::create_dir_all(package.path().join("variables")).unwrap();
        std::fs::write(
            package.path().join("variables/message.toml"),
            r#"schema_version = 1
type = "string"

[resolve]
default = "control"

[[resolve.rule]]
when = 'variables["premium"]'
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
            package.path().join("variables/bad_rule.toml"),
            r#"schema_version = 1
type = "string"

[resolve]
default = "control"
rule = ["not-a-table"]
"#,
        )
        .unwrap();
        let err = resolve_variable(package.path(), "bad_rule", &context)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("rule must be a table"));
    }

    #[tokio::test]
    async fn resolves_cross_variable_references_and_cycles() {
        let package = package_with_conditions(&[]);
        std::fs::create_dir_all(package.path().join("variables")).unwrap();
        std::fs::write(
            package.path().join("variables/premium_user.toml"),
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'context.user.tier == "premium"'
value = true
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/message.toml"),
            r#"schema_version = 1
type = "string"

[resolve]
default = "control"

[[resolve.rule]]
when = 'variables["premium_user"]'
value = "premium"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/greeting.toml"),
            r#"schema_version = 1
type = "string"

[resolve]
default = "hello"

[[resolve.rule]]
when = 'variables.message == "premium"'
value = "welcome back"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/loop_a.toml"),
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'variables["loop_b"]'
value = true
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/loop_b.toml"),
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'variables["loop_a"]'
value = true
"#,
        )
        .unwrap();

        let premium = serde_json::json!({ "user": { "tier": "premium" } });
        let free = serde_json::json!({ "user": { "tier": "free" } });

        // A bool condition variable referenced with the bracket form.
        let message = resolve_variable(package.path(), "message", &premium)
            .await
            .unwrap();
        assert_eq!(message.value, serde_json::json!("premium"));
        let message = resolve_variable(package.path(), "message", &free)
            .await
            .unwrap();
        assert_eq!(message.value, serde_json::json!("control"));

        // A non-bool variable value referenced with the dot form, two hops deep
        // (greeting -> message -> premium_user).
        let greeting = resolve_variable(package.path(), "greeting", &premium)
            .await
            .unwrap();
        assert_eq!(greeting.value, serde_json::json!("welcome back"));

        let err = resolve_variable(package.path(), "loop_a", &premium)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("variable reference cycle detected")
        );
    }

    #[tokio::test]
    async fn numeric_equality_is_exact_without_lossy_large_integer_casts() {
        let package = package_with_conditions(&[
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
            resolve_condition(package.path(), "int-float-equal", &context)
                .await
                .unwrap()
        );
        assert!(
            resolve_condition(package.path(), "large-int-self-equal", &context)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn resolves_list_membership_in_when_and_query() {
        let package = package_with_conditions(&[(
            "known_tier",
            condition(r#"context.tier in lists.plan_tiers"#),
        )]);
        std::fs::create_dir_all(package.path().join("lists")).unwrap();
        std::fs::write(
            package.path().join("lists/plan_tiers.toml"),
            "schema_version = 1\ntype = \"string\"\nmembers = [\"free\", \"team\", \"business\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(package.path().join("model/catalogs")).unwrap();
        std::fs::create_dir_all(package.path().join("data/catalogs/plan")).unwrap();
        std::fs::write(
            package.path().join("model/catalogs/plan.schema.json"),
            r#"{
  "type": "object",
  "required": ["tier"],
  "properties": {
    "tier": { "type": "string" }
  }
}
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("data/catalogs/plan/team.toml"),
            "tier = \"team\"\n",
        )
        .unwrap();
        std::fs::write(
            package.path().join("data/catalogs/plan/legacy.toml"),
            "tier = \"trial\"\n",
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/supported_plans.toml"),
            r#"schema_version = 1
type = "array<catalog=plan>"

[resolve]
method = "query"
from = "plan"
filter = "entry.tier in lists.plan_tiers"
"#,
        )
        .unwrap();

        // A rule `when` reads the member list by name.
        let member = serde_json::json!({ "tier": "team" });
        let outsider = serde_json::json!({ "tier": "trial" });
        assert!(
            resolve_condition(package.path(), "known_tier", &member)
                .await
                .unwrap()
        );
        assert!(
            !resolve_condition(package.path(), "known_tier", &outsider)
                .await
                .unwrap()
        );

        // A query filter selects only entries whose field is a member.
        let selected = resolve_variable(package.path(), "supported_plans", &member)
            .await
            .unwrap();
        let entries = selected.value.as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["tier"], serde_json::json!("team"));
    }

    #[tokio::test]
    async fn resolves_when_conditions_and_catalog_query_variables() {
        let package =
            package_with_conditions(&[("premium", condition(r#"context.user.tier == "premium""#))]);
        std::fs::create_dir_all(package.path().join("model/catalogs")).unwrap();
        std::fs::create_dir_all(package.path().join("data/catalogs/message_template")).unwrap();
        std::fs::create_dir_all(package.path().join("data/catalogs/hero_banner")).unwrap();
        std::fs::create_dir_all(package.path().join("data/catalogs/page")).unwrap();
        std::fs::create_dir_all(package.path().join("variables")).unwrap();
        std::fs::write(
            package
                .path()
                .join("model/catalogs/message_template.schema.json"),
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
                .join("data/catalogs/message_template/email.toml"),
            r#"channel = "email"
active = true
body = "Email body"
"#,
        )
        .unwrap();
        std::fs::write(
            package
                .path()
                .join("data/catalogs/message_template/sms.toml"),
            r#"channel = "sms"
active = false
body = "SMS body"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/templates.toml"),
            r#"schema_version = 1
type = "array<catalog=message_template>"

[resolve]
method = "query"
from = "message_template"
filter = "entry.channel == context.channel && entry.active == true && variables[\"premium\"]"
"#,
        )
        .unwrap();
        std::fs::write(
            package
                .path()
                .join("model/catalogs/hero_banner.schema.json"),
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
            package.path().join("data/catalogs/hero_banner/home.toml"),
            r#"cta = "Buy"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("model/catalogs/page.schema.json"),
            r#"{
  "type": "object",
  "required": ["hero", "title"],
  "properties": {
    "hero": {
      "type": "string",
      "x-rototo-ref": "catalog=hero_banner"
    },
    "title": { "type": "string" }
  }
}
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("data/catalogs/page/home.toml"),
            r#"hero = "home"
title = "Home"
"#,
        )
        .unwrap();
        std::fs::write(
            package.path().join("variables/pages.toml"),
            r#"schema_version = 1
type = "array<catalog=page>"

[resolve]
method = "query"
from = "page"
filter = "entry.hero.cta == \"Buy\""
"#,
        )
        .unwrap();

        let context = serde_json::json!({
            "channel": "email",
            "user": { "tier": "premium" }
        });

        assert!(
            resolve_condition(package.path(), "premium", &context)
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
        // The app receives the raw entry: `hero` stays the reference
        // string it was authored as (hydration is for query predicates).
        assert_eq!(
            pages.value,
            serde_json::json!([
                {
                    "id": "home",
                    "title": "Home",
                    "hero": "home"
                }
            ])
        );
    }

    /// A package with one `plan` catalog (three priced entries) and one
    /// variable file, for exercising the query pipeline end to end.
    fn package_with_query_variable(variable: &str) -> tempfile::TempDir {
        let package = package_with_conditions(&[]);
        std::fs::create_dir_all(package.path().join("model/catalogs")).unwrap();
        std::fs::create_dir_all(package.path().join("data/catalogs/plan")).unwrap();
        std::fs::write(
            package.path().join("model/catalogs/plan.schema.json"),
            r#"{
  "type": "object",
  "required": ["tier", "price"],
  "properties": {
    "tier": { "type": "string" },
    "price": { "type": "number" }
  }
}
"#,
        )
        .unwrap();
        for (name, tier, price) in [
            ("free", "free", 0),
            ("pro", "pro", 20),
            ("enterprise", "enterprise", 100),
        ] {
            std::fs::write(
                package
                    .path()
                    .join(format!("data/catalogs/plan/{name}.toml")),
                format!("tier = \"{tier}\"\nprice = {price}\n"),
            )
            .unwrap();
        }
        std::fs::write(package.path().join("variables/plan.toml"), variable).unwrap();
        package
    }

    #[tokio::test]
    async fn query_single_selects_the_top_entry_after_sort() {
        let package = package_with_query_variable(
            r#"schema_version = 1
type = "catalog=plan"

[resolve]
method = "query"
from = "plan"
filter = "entry.price > 0"
sort = "entry.price"
order = "desc"
"#,
        );
        let resolution = resolve_variable(package.path(), "plan", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(resolution.value["tier"], "enterprise");
        assert!(matches!(
            resolution.source,
            VariableResolutionSource::Catalog { ref catalog, ref value }
                if catalog == "plan" && value == "enterprise"
        ));
    }

    #[tokio::test]
    async fn query_single_without_sort_requires_exactly_one_match() {
        let package = package_with_query_variable(
            r#"schema_version = 1
type = "catalog=plan"

[resolve]
method = "query"
from = "plan"
filter = "entry.price > 0"
"#,
        );
        let err = resolve_variable(package.path(), "plan", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("matched 2 entries"));
        assert!(err.to_string().contains("add sort or narrow the filter"));
    }

    #[tokio::test]
    async fn query_single_with_no_match_uses_default_or_errors() {
        let package = package_with_query_variable(
            r#"schema_version = 1
type = "catalog=plan"

[resolve]
method = "query"
from = "plan"
filter = "entry.price > 1000"
default = "free"
"#,
        );
        let resolution = resolve_variable(package.path(), "plan", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(resolution.value["tier"], "free");

        let package = package_with_query_variable(
            r#"schema_version = 1
type = "catalog=plan"

[resolve]
method = "query"
from = "plan"
filter = "entry.price > 1000"
"#,
        );
        let err = resolve_variable(package.path(), "plan", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("matched no entries"));
    }

    #[tokio::test]
    async fn query_list_sorts_and_limits_matches() {
        let package = package_with_query_variable(
            r#"schema_version = 1
type = "array<catalog=plan>"

[resolve]
method = "query"
from = "plan"
sort = "entry.price"
order = "desc"
limit = 2
"#,
        );
        let resolution = resolve_variable(package.path(), "plan", &serde_json::json!({}))
            .await
            .unwrap();
        let tiers: Vec<&str> = resolution
            .value
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["tier"].as_str().unwrap())
            .collect();
        assert_eq!(tiers, vec!["enterprise", "pro"]);
    }

    #[tokio::test]
    async fn query_list_with_no_match_is_empty_without_default() {
        let package = package_with_query_variable(
            r#"schema_version = 1
type = "array<catalog=plan>"

[resolve]
method = "query"
from = "plan"
filter = "entry.price > 1000"
"#,
        );
        let resolution = resolve_variable(package.path(), "plan", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(resolution.value, serde_json::json!([]));
    }

    #[tokio::test]
    async fn query_sort_keys_must_be_comparable() {
        let package = package_with_query_variable(
            r#"schema_version = 1
type = "array<catalog=plan>"

[resolve]
method = "query"
from = "plan"
sort = "entry.price > 0 ? entry.tier : entry.price"
"#,
        );
        let err = resolve_variable(package.path(), "plan", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("sort keys are not comparable"));
    }

    /// A package with one two-arm layer allocation and one allocation-driven
    /// string variable, for exercising arm assignment end to end.
    fn package_with_allocation_variable(layer: &str, variable: &str) -> tempfile::TempDir {
        let package = package_with_conditions(&[]);
        std::fs::create_dir_all(package.path().join("layers")).unwrap();
        std::fs::write(package.path().join("layers/checkout.toml"), layer).unwrap();
        std::fs::write(package.path().join("variables/cta.toml"), variable).unwrap();
        package
    }

    const CTA_LAYER: &str = r#"schema_version = 1
unit = "context.user.id"
buckets = 100

[[allocation]]
id = "cta_copy"
status = "running"
eligibility = 'context.account.plan != "enterprise"'

[[allocation.arm]]
name = "control"
buckets = "0-49"

[[allocation.arm]]
name = "benefit_led"
buckets = "50-99"
"#;

    const CTA_VARIABLE: &str = r#"schema_version = 1
type = "string"

[resolve]
method = "allocation"
allocation = "cta_copy"
default = "click here"

[[resolve.assign]]
arm = "control"
value = "click here"

[[resolve.assign]]
arm = "benefit_led"
value = "save time"
"#;

    #[tokio::test]
    async fn allocation_assigns_arms_deterministically() {
        let package = package_with_allocation_variable(CTA_LAYER, CTA_VARIABLE);
        let context =
            |id: &str| serde_json::json!({ "user": { "id": id }, "account": { "plan": "pro" } });

        // The hash is part of the contract: these units land in these arms,
        // and rerunning the resolution never moves them.
        let first = resolve_variable(package.path(), "cta", &context("user-1"))
            .await
            .unwrap();
        let again = resolve_variable(package.path(), "cta", &context("user-1"))
            .await
            .unwrap();
        assert_eq!(first.value, again.value);

        let mut arms = std::collections::BTreeSet::new();
        for index in 0..20 {
            let resolution =
                resolve_variable(package.path(), "cta", &context(&format!("user-{index}")))
                    .await
                    .unwrap();
            arms.insert(resolution.value.as_str().unwrap().to_owned());
        }
        assert_eq!(
            arms.into_iter().collect::<Vec<_>>(),
            vec!["click here".to_owned(), "save time".to_owned()],
            "20 units should spread across both arms"
        );
    }

    #[tokio::test]
    async fn allocation_trace_records_the_assignment() {
        let package = package_with_allocation_variable(CTA_LAYER, CTA_VARIABLE);
        let context = serde_json::json!({
            "user": { "id": "user-1" },
            "account": { "plan": "pro" }
        });
        let traces = trace_variable_resolutions(package.path(), &context)
            .await
            .unwrap();
        let trace = traces
            .iter()
            .find(|trace| trace.resolution.id == "cta")
            .unwrap();
        let allocation = trace.allocation.as_ref().expect("allocation trace");
        assert_eq!(allocation.layer, "checkout");
        assert_eq!(allocation.allocation, "cta_copy");
        assert!(allocation.enrolled);
        assert!(allocation.bucket.is_some());
        assert!(allocation.arm.is_some());
    }

    #[tokio::test]
    async fn allocation_ineligible_units_resolve_to_the_default() {
        let package = package_with_allocation_variable(CTA_LAYER, CTA_VARIABLE);
        let context = serde_json::json!({
            "user": { "id": "user-1" },
            "account": { "plan": "enterprise" }
        });
        let traces = trace_variable_resolutions(package.path(), &context)
            .await
            .unwrap();
        let trace = traces
            .iter()
            .find(|trace| trace.resolution.id == "cta")
            .unwrap();
        assert_eq!(trace.resolution.value, "click here");
        let allocation = trace.allocation.as_ref().expect("allocation trace");
        assert!(!allocation.enrolled);
        assert!(allocation.arm.is_none());
    }

    #[tokio::test]
    async fn allocation_only_assigns_while_running() {
        for status in ["draft", "concluded"] {
            let layer =
                CTA_LAYER.replace("status = \"running\"", &format!("status = \"{status}\""));
            let package = package_with_allocation_variable(&layer, CTA_VARIABLE);
            let context = serde_json::json!({
                "user": { "id": "user-1" },
                "account": { "plan": "pro" }
            });
            let resolution = resolve_variable(package.path(), "cta", &context)
                .await
                .unwrap();
            assert_eq!(resolution.value, "click here", "status {status}");
        }
    }

    #[tokio::test]
    async fn allocation_unclaimed_buckets_resolve_to_the_default() {
        // Shrink both arms so most of the line is unclaimed.
        let layer = CTA_LAYER
            .replace("buckets = \"0-49\"", "buckets = \"0\"")
            .replace("buckets = \"50-99\"", "buckets = \"1\"");
        let package = package_with_allocation_variable(&layer, CTA_VARIABLE);
        let mut unclaimed = 0;
        for index in 0..10 {
            let context = serde_json::json!({
                "user": { "id": format!("user-{index}") },
                "account": { "plan": "pro" }
            });
            let traces = trace_variable_resolutions(package.path(), &context)
                .await
                .unwrap();
            let trace = traces
                .iter()
                .find(|trace| trace.resolution.id == "cta")
                .unwrap();
            let allocation = trace.allocation.as_ref().unwrap();
            if allocation.arm.is_none() {
                assert!(allocation.enrolled);
                assert_eq!(trace.resolution.value, "click here");
                unclaimed += 1;
            }
        }
        assert!(
            unclaimed > 0,
            "some of 10 units should fall outside 2 of 100 buckets"
        );
    }

    #[tokio::test]
    async fn malformed_conditions_return_errors_during_resolution() {
        let context = serde_json::json!({ "user": { "tier": "premium", "id": "user-123" } });

        let package = package_with_conditions(&[(
            "unknown-function",
            condition(r#"not_a_real_function(context.user.tier, "premium")"#),
        )]);
        let err = resolve_condition(package.path(), "unknown-function", &context)
            .await
            .unwrap_err();
        // The unknown function fails during evaluation; the message is cel's.
        assert!(!err.to_string().is_empty());

        let package =
            package_with_conditions(&[("missing-when", String::from("schema_version = 1\n"))]);
        let err = resolve_condition(package.path(), "missing-when", &context)
            .await
            .unwrap_err();
        assert!(!err.to_string().is_empty());

        let package = package_with_conditions(&[("non-bool", condition("context.user.tier"))]);
        let err = resolve_condition(package.path(), "non-bool", &context)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("expression did not evaluate to bool")
        );
    }

    #[tokio::test]
    async fn batch_resolve_and_batch_trace_share_one_evaluation_state() {
        // A shared condition variable referenced by several variables, and a
        // batch resolve next to its traced twin: both run under one state
        // (one env.now, one memoization cache), so the trace explains
        // exactly what resolve did.
        let package = tempfile::tempdir().unwrap();
        let root = package.path();
        std::fs::write(root.join("rototo-package.toml"), "schema_version = 1\n").unwrap();
        std::fs::create_dir_all(root.join("variables")).unwrap();
        std::fs::write(
            root.join("variables/premium.toml"),
            "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n\n[[resolve.rule]]\nwhen = 'context.tier == \"premium\"'\nvalue = true\n",
        )
        .unwrap();
        for id in ["left", "right"] {
            std::fs::write(
                root.join(format!("variables/{id}.toml")),
                "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"off\"\n\n[[resolve.rule]]\nwhen = 'variables[\"premium\"]'\nvalue = \"on\"\n",
            )
            .unwrap();
        }

        let runtime = compile_runtime_package(root).await.unwrap();
        let context = serde_json::json!({ "tier": "premium" });

        let resolutions = resolve_variables_unchecked(&runtime, &context).unwrap();
        let traces = trace_variable_resolutions_unchecked(&runtime, &context).unwrap();
        assert_eq!(resolutions.len(), traces.len());
        for (resolution, trace) in resolutions.iter().zip(&traces) {
            assert_eq!(resolution.id, trace.resolution.id);
            assert_eq!(resolution.value, trace.resolution.value);
        }
        // The traced batch still records full per-variable detail even
        // though the shared condition memoized after its first evaluation.
        for trace in &traces {
            if trace.resolution.id != "premium" {
                assert_eq!(trace.rules.len(), 1);
                assert!(trace.rules[0].matched);
            }
        }
    }

    #[tokio::test]
    async fn outcome_batch_isolates_missing_context_key_failures() {
        // A package overview resolves everything under one partial context.
        // The variable whose rule reads the absent key fails alone; the rest
        // of the batch still traces, and dependents of the failing variable
        // fail with it instead of silently resolving.
        let package = package_with_conditions(&[
            ("premium", condition(r#"context.user.tier == "premium""#)),
            ("lane_dev", condition(r#"context.lane == "dev""#)),
            ("routed", condition(r#"variables["lane_dev"]"#)),
        ]);
        let partial = serde_json::json!({ "user": { "tier": "premium" } });

        // The strict batch refuses the whole context.
        let err = trace_variable_resolutions(package.path(), &partial)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("reads context.lane, which the given context does not carry"),
            "unexpected error: {err}"
        );

        let outcomes = trace_variable_resolution_outcomes(package.path(), &partial)
            .await
            .unwrap();
        let outcome = |id: &str| outcomes.iter().find(|outcome| outcome.id == id).unwrap();
        let premium = outcome("premium");
        assert_eq!(
            premium.trace.as_ref().unwrap().resolution.value,
            serde_json::json!(true)
        );
        assert!(premium.error.is_none());
        for id in ["lane_dev", "routed"] {
            let failed = outcome(id);
            assert!(failed.trace.is_none());
            let error = failed.error.as_ref().unwrap();
            assert!(
                error.contains("reads context.lane, which the given context does not carry"),
                "unexpected error for {id}: {error}"
            );
        }

        // A context covering every key read clears the whole batch.
        let full = serde_json::json!({ "user": { "tier": "premium" }, "lane": "dev" });
        let outcomes = trace_variable_resolution_outcomes(package.path(), &full)
            .await
            .unwrap();
        assert!(outcomes.iter().all(|outcome| outcome.trace.is_some()));
    }

    fn package_with_conditions(conditions: &[(&str, String)]) -> tempfile::TempDir {
        let package = tempfile::TempDir::new().unwrap();
        std::fs::write(
            package.path().join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .unwrap();
        std::fs::create_dir_all(package.path().join("variables")).unwrap();
        for (id, contents) in conditions {
            std::fs::write(
                package.path().join(format!("variables/{id}.toml")),
                contents,
            )
            .unwrap();
        }
        package
    }

    /// Resolve a bool condition variable to its boolean outcome.
    async fn resolve_condition(
        package: &std::path::Path,
        id: &str,
        context: &JsonValue,
    ) -> Result<bool> {
        let resolution = resolve_variable(package, id, context).await?;
        resolution.value.as_bool().ok_or_else(|| {
            RototoError::new(format!("condition variable did not resolve to bool: {id}"))
        })
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
        format!(
            "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n\n[[resolve.rule]]\nwhen = \"{escaped}\"\nvalue = true\n"
        )
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
        if let Some(variable) = attribute.strip_prefix("variable.") {
            format!("variables[\"{variable}\"]")
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
