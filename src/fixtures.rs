use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Number as JsonNumber, Value as JsonValue};

use crate::error::{Result, RototoError};
use crate::expression::simple_rule_qualifier;
use crate::model::{
    PackageInspectReport, PackageInspectRequest, PredicateInspectReport, QualifierInspectReport,
    QualifierResolutionTrace, RulePathwayInspectReport, VariableInspectReport,
    VariableResolutionTrace,
};
use crate::resolve::{bucket_value, trace_qualifier_resolution, trace_variable_resolution};
use crate::sdk::{EvaluationContext, Package};
use crate::source::{SourceOptions, stage_package_source};

const SUITE_FILE: &str = "rototo-fixtures.toml";
const MAX_BUCKET_CANDIDATES: usize = 100_000;

#[derive(Clone, Debug, Default)]
pub struct FixtureGenerateSelection {
    pub variables: FixtureTargetSelection,
    pub qualifiers: FixtureTargetSelection,
}

impl FixtureGenerateSelection {
    pub fn all() -> Self {
        Self {
            variables: FixtureTargetSelection::All,
            qualifiers: FixtureTargetSelection::All,
        }
    }

    fn normalized(self) -> Self {
        if self.variables.is_none() && self.qualifiers.is_none() {
            Self::all()
        } else {
            self
        }
    }
}

#[derive(Clone, Debug, Default)]
pub enum FixtureTargetSelection {
    #[default]
    None,
    Some(BTreeSet<String>),
    All,
}

impl FixtureTargetSelection {
    pub fn some(values: impl IntoIterator<Item = String>) -> Self {
        Self::Some(values.into_iter().collect())
    }

    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Debug)]
pub struct GeneratedFixtureSuite {
    pub package: String,
    pub manifest: FixtureSuite,
    files: Vec<GeneratedFixtureFile>,
}

#[derive(Debug)]
struct GeneratedFixtureFile {
    path: PathBuf,
    fixture: FixtureFile,
}

#[derive(Debug, Serialize)]
pub struct FixtureWriteReport {
    pub out: String,
    pub files: Vec<String>,
}

impl GeneratedFixtureSuite {
    pub async fn write_to(&self, out: impl AsRef<Path>) -> Result<FixtureWriteReport> {
        let out = out.as_ref();
        tokio::fs::create_dir_all(out).await.map_err(|err| {
            RototoError::new(format!(
                "failed to create fixture directory {}: {err}",
                out.display()
            ))
        })?;

        let mut written = Vec::new();
        let manifest_path = out.join(SUITE_FILE);
        write_toml_file(&manifest_path, &self.manifest).await?;
        written.push(SUITE_FILE.to_owned());

        for file in &self.files {
            let path = out.join(&file.path);
            write_toml_file(&path, &file.fixture).await?;
            written.push(file.path.display().to_string());
        }

        Ok(FixtureWriteReport {
            out: out.display().to_string(),
            files: written,
        })
    }
}

async fn write_toml_file(path: &Path, value: impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| {
            RototoError::new(format!(
                "failed to create fixture directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    let mut text = toml::to_string_pretty(&value)
        .map_err(|err| RototoError::new(format!("failed to render fixture TOML: {err}")))?;
    text.push('\n');
    tokio::fs::write(path, text).await.map_err(|err| {
        RototoError::new(format!("failed to write fixture {}: {err}", path.display()))
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureSuite {
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, rename = "fixture")]
    pub fixtures: Vec<FixtureEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureEntry {
    pub path: String,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureFile {
    pub schema_version: u32,
    pub target: String,
    pub description: String,
    #[serde(default, rename = "case")]
    pub cases: Vec<FixtureCase>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureCase {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub because: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub generated: bool,
    #[serde(default = "empty_toml_table")]
    pub context: toml::Value,
    pub expect: FixtureExpectation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureExpectation {
    pub value: toml::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_condition: Option<String>,
    #[serde(default, rename = "bucket", skip_serializing_if = "Vec::is_empty")]
    pub buckets: Vec<FixtureBucketExpectation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureBucketExpectation {
    pub attribute: String,
    pub salt: String,
    pub range: Vec<i64>,
    pub value: u16,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn empty_toml_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

pub async fn generate_fixture_suite(
    package_source: impl AsRef<str>,
    source_options: &SourceOptions,
    selection: FixtureGenerateSelection,
) -> Result<GeneratedFixtureSuite> {
    let package_source = package_source.as_ref();
    let selection = selection.normalized();
    let staged = stage_package_source(package_source.to_owned(), source_options).await?;
    let report =
        crate::inspect_package_report(staged.path(), PackageInspectRequest::default()).await?;

    let variable_ids = selected_variable_ids(&report, &selection.variables)?;
    let qualifier_ids = selected_qualifier_ids(&report, &selection.qualifiers)?;
    let factory = ContextFactory::new(&report);

    let mut manifest = FixtureSuite {
        schema_version: 1,
        package: Some(package_source.to_owned()),
        fixtures: Vec::new(),
    };
    let mut files = Vec::new();

    for id in qualifier_ids {
        let qualifier = report
            .qualifiers
            .iter()
            .find(|qualifier| qualifier.id == id)
            .expect("selected qualifier id was validated");
        let fixture = generate_qualifier_fixture(staged.path(), qualifier, &factory).await?;
        let path = PathBuf::from("qualifiers").join(format!("{id}.toml"));
        manifest.fixtures.push(FixtureEntry {
            target: fixture.target.clone(),
            path: path.display().to_string(),
        });
        files.push(GeneratedFixtureFile { path, fixture });
    }

    for id in variable_ids {
        let variable = report
            .variables
            .iter()
            .find(|variable| variable.id == id)
            .expect("selected variable id was validated");
        let fixture = generate_variable_fixture(staged.path(), variable, &factory).await?;
        let path = PathBuf::from("variables").join(format!("{id}.toml"));
        manifest.fixtures.push(FixtureEntry {
            target: fixture.target.clone(),
            path: path.display().to_string(),
        });
        files.push(GeneratedFixtureFile { path, fixture });
    }

    Ok(GeneratedFixtureSuite {
        package: package_source.to_owned(),
        manifest,
        files,
    })
}

fn selected_variable_ids(
    report: &PackageInspectReport,
    selection: &FixtureTargetSelection,
) -> Result<Vec<String>> {
    selected_ids(
        selection,
        report.variables.iter().map(|variable| variable.id.as_str()),
        "variable",
    )
}

fn selected_qualifier_ids(
    report: &PackageInspectReport,
    selection: &FixtureTargetSelection,
) -> Result<Vec<String>> {
    selected_ids(
        selection,
        report
            .qualifiers
            .iter()
            .map(|qualifier| qualifier.id.as_str()),
        "qualifier",
    )
}

fn selected_ids<'a>(
    selection: &FixtureTargetSelection,
    available: impl Iterator<Item = &'a str>,
    kind: &str,
) -> Result<Vec<String>> {
    let available = available.map(str::to_owned).collect::<Vec<_>>();
    match selection {
        FixtureTargetSelection::None => Ok(Vec::new()),
        FixtureTargetSelection::All => Ok(available),
        FixtureTargetSelection::Some(ids) => {
            for id in ids {
                if !available.iter().any(|available| available == id) {
                    return Err(RototoError::new(format!("{kind} not found: {kind}://{id}")));
                }
            }
            Ok(available
                .into_iter()
                .filter(|id| ids.contains(id))
                .collect())
        }
    }
}

async fn generate_qualifier_fixture(
    package: &Path,
    qualifier: &QualifierInspectReport,
    factory: &ContextFactory<'_>,
) -> Result<FixtureFile> {
    let mut cases = Vec::new();

    if let Some((context, trace)) =
        sampled_qualifier_context(package, &qualifier.id, true, factory).await?
    {
        cases.push(qualifier_case(
            "matches",
            "Matches when the qualifier condition is true",
            Some("An evaluation context sample satisfies the qualifier condition."),
            true,
            context,
            &trace,
        )?);
    }

    if let Some((context, trace)) =
        sampled_qualifier_context(package, &qualifier.id, false, factory).await?
    {
        cases.push(qualifier_case(
            "does-not-match",
            "Does not match when the qualifier condition is false",
            Some("An evaluation context sample does not satisfy the qualifier condition."),
            true,
            context,
            &trace,
        )?);
    }

    Ok(FixtureFile {
        schema_version: 1,
        target: format!("qualifier:{}", qualifier.id),
        description: format!("Runtime behavior fixtures for qualifier://{}", qualifier.id),
        cases,
    })
}

fn qualifier_case(
    id: &str,
    title: &str,
    because: Option<&str>,
    generated: bool,
    context: JsonValue,
    trace: &QualifierResolutionTrace,
) -> Result<FixtureCase> {
    Ok(FixtureCase {
        id: id.to_owned(),
        title: title.to_owned(),
        because: because.map(str::to_owned),
        generated,
        context: json_to_toml(&context)?,
        expect: FixtureExpectation {
            value: toml::Value::Boolean(trace.value),
            matched: None,
            matched_rule: None,
            matched_condition: None,
            buckets: buckets_from_qualifier_trace(trace),
        },
    })
}

async fn generate_variable_fixture(
    package: &Path,
    variable: &VariableInspectReport,
    factory: &ContextFactory<'_>,
) -> Result<FixtureFile> {
    let mut cases = Vec::new();

    if let Some(context) = variable_default_context(package, variable, factory).await? {
        let trace = trace_variable_resolution(package, &variable.id, &context).await?;
        if trace.rules.iter().any(|rule| rule.matched) {
            return Err(RototoError::new(format!(
                "generated default fixture matched a rule for variable: {}",
                variable.id
            )));
        }
        cases.push(variable_case(
            "default",
            "Uses the default value when no rule matches",
            Some("Every rule condition is false."),
            true,
            context,
            &trace,
        )?);
    }

    for rule in &variable.resolve.rules {
        let Some(context) = variable_rule_context(package, variable, rule, factory).await? else {
            continue;
        };
        let trace = trace_variable_resolution(package, &variable.id, &context).await?;
        if !trace
            .rules
            .iter()
            .any(|trace_rule| trace_rule.index == rule.index && trace_rule.matched)
        {
            continue;
        }
        let condition = rule_condition_label(rule);
        let id = format!("rule-{}-{}", rule.index, sanitize_id(&condition));
        let title = format!(
            "Rule {} selects {} when {} matches",
            rule.index,
            rule.value
                .as_ref()
                .map(serde_json::Value::to_string)
                .unwrap_or_else(|| "<missing>".to_owned()),
            condition
        );
        cases.push(variable_case(
            &id,
            &title,
            Some("Earlier rule conditions are kept false when possible."),
            true,
            context,
            &trace,
        )?);
    }

    Ok(FixtureFile {
        schema_version: 1,
        target: format!("variable:{}", variable.id),
        description: format!("Runtime behavior fixtures for variable://{}", variable.id),
        cases,
    })
}

fn variable_case(
    id: &str,
    title: &str,
    because: Option<&str>,
    generated: bool,
    context: JsonValue,
    trace: &VariableResolutionTrace,
) -> Result<FixtureCase> {
    let matched_rule = trace.rules.iter().find(|rule| rule.matched);
    Ok(FixtureCase {
        id: id.to_owned(),
        title: title.to_owned(),
        because: because.map(str::to_owned),
        generated,
        context: json_to_toml(&context)?,
        expect: FixtureExpectation {
            value: json_to_toml(&trace.resolution.value)?,
            matched: matched_rule.is_none().then(|| "default".to_owned()),
            matched_rule: matched_rule.map(|rule| rule.index),
            matched_condition: matched_rule.map(|rule| rule.condition.clone()),
            buckets: trace
                .qualifier_traces
                .iter()
                .flat_map(buckets_from_qualifier_trace)
                .collect(),
        },
    })
}

fn rule_condition_label(rule: &RulePathwayInspectReport) -> String {
    rule.when
        .as_deref()
        .or(rule.query.as_deref())
        .unwrap_or("<missing>")
        .to_owned()
}

async fn variable_rule_context(
    package: &Path,
    variable: &VariableInspectReport,
    rule: &RulePathwayInspectReport,
    factory: &ContextFactory<'_>,
) -> Result<Option<JsonValue>> {
    for context in factory.sample_contexts() {
        if let Ok(trace) = trace_variable_resolution(package, &variable.id, context).await
            && trace
                .rules
                .iter()
                .any(|trace_rule| trace_rule.index == rule.index && trace_rule.matched)
        {
            return Ok(Some(context.clone()));
        }
    }

    if let Some(qualifier) = rule.when.as_deref().and_then(simple_rule_qualifier)
        && let Some(context) = factory.qualifier_context(&qualifier, true)
        && let Ok(trace) = trace_variable_resolution(package, &variable.id, &context).await
        && trace
            .rules
            .iter()
            .any(|trace_rule| trace_rule.index == rule.index && trace_rule.matched)
    {
        return Ok(Some(context));
    }

    Ok(factory.variable_rule_context(variable, rule))
}

async fn variable_default_context(
    package: &Path,
    variable: &VariableInspectReport,
    factory: &ContextFactory<'_>,
) -> Result<Option<JsonValue>> {
    for context in factory.sample_contexts() {
        if let Ok(trace) = trace_variable_resolution(package, &variable.id, context).await
            && trace.rules.iter().all(|rule| !rule.matched)
        {
            return Ok(Some(context.clone()));
        }
    }

    Ok(factory.variable_default_context(variable))
}

async fn sampled_qualifier_context(
    package: &Path,
    qualifier: &str,
    desired: bool,
    factory: &ContextFactory<'_>,
) -> Result<Option<(JsonValue, QualifierResolutionTrace)>> {
    for context in factory.sample_contexts() {
        if let Ok(trace) = trace_qualifier_resolution(package, qualifier, context).await
            && trace.value == desired
        {
            return Ok(Some((context.clone(), trace)));
        }
    }

    if let Some(context) = factory.qualifier_context(qualifier, desired)
        && let Ok(trace) = trace_qualifier_resolution(package, qualifier, &context).await
        && trace.value == desired
    {
        return Ok(Some((context, trace)));
    }

    Ok(None)
}

fn buckets_from_qualifier_trace(trace: &QualifierResolutionTrace) -> Vec<FixtureBucketExpectation> {
    let _ = trace;
    Vec::new()
}

struct ContextFactory<'a> {
    qualifiers: BTreeMap<String, &'a QualifierInspectReport>,
    samples: Vec<JsonValue>,
}

impl<'a> ContextFactory<'a> {
    fn new(report: &'a PackageInspectReport) -> Self {
        Self {
            qualifiers: report
                .qualifiers
                .iter()
                .map(|qualifier| (qualifier.id.clone(), qualifier))
                .collect(),
            samples: report
                .evaluation_contexts
                .iter()
                .flat_map(|evaluation_context| evaluation_context.samples.iter())
                .map(|sample| sample.value.clone())
                .collect(),
        }
    }

    fn sample_contexts(&self) -> &[JsonValue] {
        &self.samples
    }

    fn variable_default_context(&self, variable: &VariableInspectReport) -> Option<JsonValue> {
        let mut context = empty_json_object();
        for rule in &variable.resolve.rules {
            let qualifier = simple_rule_qualifier(rule.when.as_deref()?)?;
            merge_context(&mut context, self.qualifier_context(&qualifier, false)?)?;
        }
        Some(context)
    }

    fn variable_rule_context(
        &self,
        variable: &VariableInspectReport,
        rule: &RulePathwayInspectReport,
    ) -> Option<JsonValue> {
        let mut context = empty_json_object();
        for earlier in variable
            .resolve
            .rules
            .iter()
            .filter(|earlier| earlier.index < rule.index)
        {
            let qualifier = simple_rule_qualifier(earlier.when.as_deref()?)?;
            merge_context(&mut context, self.qualifier_context(&qualifier, false)?)?;
        }
        let qualifier = simple_rule_qualifier(rule.when.as_deref()?)?;
        merge_context(&mut context, self.qualifier_context(&qualifier, true)?)?;
        Some(context)
    }

    fn qualifier_context(&self, id: &str, desired: bool) -> Option<JsonValue> {
        self.qualifier_context_inner(id, desired, &mut BTreeSet::new())
    }

    fn qualifier_context_inner(
        &self,
        id: &str,
        desired: bool,
        stack: &mut BTreeSet<String>,
    ) -> Option<JsonValue> {
        if !desired {
            return None;
        }

        if !stack.insert(id.to_owned()) {
            return None;
        }
        let qualifier = self.qualifiers.get(id)?;
        if qualifier.predicates.is_empty() {
            stack.remove(id);
            return None;
        }
        let mut context = empty_json_object();
        for predicate in &qualifier.predicates {
            merge_context(
                &mut context,
                self.predicate_context(predicate, true, stack)?,
            )?;
        }
        stack.remove(id);
        Some(context)
    }

    fn predicate_context(
        &self,
        predicate: &PredicateInspectReport,
        should_match: bool,
        stack: &mut BTreeSet<String>,
    ) -> Option<JsonValue> {
        let attribute = predicate.attribute.as_deref()?;
        if let Some(qualifier_id) = attribute.strip_prefix("qualifier.") {
            let expected = predicate.value.as_ref()?;
            let desired =
                qualifier_result_for_predicate(predicate.op.as_deref()?, expected, should_match)?;
            return self.qualifier_context_inner(qualifier_id, desired, stack);
        }

        let value = if predicate.op.as_deref() == Some("bucket") {
            bucket_candidate(predicate, should_match)?
        } else {
            context_value_for_predicate(predicate, should_match)?
        };
        context_with_path(attribute, value)
    }
}

fn qualifier_result_for_predicate(
    op: &str,
    expected: &JsonValue,
    should_match: bool,
) -> Option<bool> {
    for candidate in [false, true] {
        let actual = JsonValue::Bool(candidate);
        if compare_predicate_values(op, &actual, expected) == should_match {
            return Some(candidate);
        }
    }
    None
}

fn context_value_for_predicate(
    predicate: &PredicateInspectReport,
    should_match: bool,
) -> Option<JsonValue> {
    let op = predicate.op.as_deref()?;
    let expected = predicate.value.as_ref()?;
    candidate_values(op, expected, should_match)
        .into_iter()
        .find(|candidate| compare_predicate_values(op, candidate, expected) == should_match)
}

fn candidate_values(op: &str, expected: &JsonValue, should_match: bool) -> Vec<JsonValue> {
    let mut values = match op {
        "gt" | "gte" | "lt" | "lte" => numeric_candidate_values(expected),
        "in" | "not_in" => {
            let mut values = Vec::new();
            if let Some(items) = expected.as_array() {
                values.extend(items.iter().cloned());
                values.push(array_alternative_value(items));
            }
            values
        }
        _ => {
            let mut values = vec![expected.clone(), alternative_value(expected)];
            values.extend([
                JsonValue::String("fixture-other".to_owned()),
                JsonValue::String("prod".to_owned()),
                JsonValue::String("standard".to_owned()),
                JsonValue::Bool(true),
                JsonValue::Bool(false),
                JsonValue::Number(JsonNumber::from(0)),
                JsonValue::Number(JsonNumber::from(1)),
                JsonValue::Number(JsonNumber::from(100)),
            ]);
            values
        }
    };

    if (!should_match && matches!(op, "eq" | "in"))
        || (should_match && matches!(op, "neq" | "not_in"))
    {
        let mut preferred = vec![
            JsonValue::String("fixture-other".to_owned()),
            JsonValue::String("prod".to_owned()),
            JsonValue::String("standard".to_owned()),
            JsonValue::Number(JsonNumber::from(0)),
            JsonValue::Bool(false),
        ];
        preferred.extend(values);
        values = preferred;
    }

    values
}

fn numeric_candidate_values(expected: &JsonValue) -> Vec<JsonValue> {
    let Some(number) = expected.as_f64() else {
        return Vec::new();
    };
    if let Some(integer) = expected.as_i64() {
        return [integer - 1, integer, integer + 1]
            .into_iter()
            .map(|value| JsonValue::Number(JsonNumber::from(value)))
            .collect();
    }
    [number - 1.0, number, number + 1.0]
        .into_iter()
        .filter_map(JsonNumber::from_f64)
        .map(JsonValue::Number)
        .collect()
}

fn alternative_value(expected: &JsonValue) -> JsonValue {
    match expected {
        JsonValue::String(value) => JsonValue::String(format!("fixture-not-{value}")),
        JsonValue::Bool(value) => JsonValue::Bool(!value),
        JsonValue::Number(number) => {
            if let Some(value) = number.as_i64() {
                JsonValue::Number(JsonNumber::from(value + 1))
            } else if let Some(value) = number.as_u64() {
                JsonValue::Number(JsonNumber::from(value + 1))
            } else {
                JsonNumber::from_f64(number.as_f64().unwrap_or(0.0) + 1.0)
                    .map(JsonValue::Number)
                    .unwrap_or_else(|| JsonValue::String("fixture-other".to_owned()))
            }
        }
        JsonValue::Array(_) | JsonValue::Object(_) | JsonValue::Null => {
            JsonValue::String("fixture-other".to_owned())
        }
    }
}

fn array_alternative_value(values: &[JsonValue]) -> JsonValue {
    for candidate in [
        JsonValue::String("fixture-other".to_owned()),
        JsonValue::String("prod".to_owned()),
        JsonValue::Bool(false),
        JsonValue::Number(JsonNumber::from(0)),
    ] {
        if values
            .iter()
            .all(|value| !json_values_equal(value, &candidate))
        {
            return candidate;
        }
    }
    JsonValue::String("fixture-other".to_owned())
}

fn compare_predicate_values(op: &str, actual: &JsonValue, expected: &JsonValue) -> bool {
    match op {
        "eq" => json_values_equal(actual, expected),
        "neq" => !json_values_equal(actual, expected),
        "in" => expected
            .as_array()
            .is_some_and(|values| values.iter().any(|value| json_values_equal(value, actual))),
        "not_in" => expected
            .as_array()
            .is_some_and(|values| values.iter().all(|value| !json_values_equal(value, actual))),
        "gt" => numeric_ordering(actual, expected).is_some_and(|ordering| ordering.is_gt()),
        "gte" => numeric_ordering(actual, expected)
            .is_some_and(|ordering| ordering.is_gt() || ordering.is_eq()),
        "lt" => numeric_ordering(actual, expected).is_some_and(|ordering| ordering.is_lt()),
        "lte" => numeric_ordering(actual, expected)
            .is_some_and(|ordering| ordering.is_lt() || ordering.is_eq()),
        _ => false,
    }
}

fn json_values_equal(left: &JsonValue, right: &JsonValue) -> bool {
    match (left.as_f64(), right.as_f64()) {
        (Some(left), Some(right)) => left == right,
        _ => left == right,
    }
}

fn numeric_ordering(left: &JsonValue, right: &JsonValue) -> Option<std::cmp::Ordering> {
    left.as_f64()?.partial_cmp(&right.as_f64()?)
}

fn bucket_candidate(predicate: &PredicateInspectReport, should_match: bool) -> Option<JsonValue> {
    let salt = predicate.salt.as_deref()?;
    let range = predicate.range.as_ref()?;
    let [start, end] = range.as_slice() else {
        return None;
    };
    for index in 0..MAX_BUCKET_CANDIDATES {
        let candidate = JsonValue::String(format!("fixture-{}-{index:05}", sanitize_id(salt)));
        let bucket = bucket_value(salt, &candidate);
        let matched = i64::from(bucket) >= *start && i64::from(bucket) < *end;
        if matched == should_match {
            return Some(candidate);
        }
    }
    None
}

fn context_with_path(path: &str, value: JsonValue) -> Option<JsonValue> {
    let mut root = serde_json::Map::new();
    insert_context_path(&mut root, path, value)?;
    Some(JsonValue::Object(root))
}

fn insert_context_path(
    object: &mut serde_json::Map<String, JsonValue>,
    path: &str,
    value: JsonValue,
) -> Option<()> {
    let mut segments = path.split('.').peekable();
    let mut current = object;
    while let Some(segment) = segments.next() {
        if segment.is_empty() {
            return None;
        }
        if segments.peek().is_none() {
            current.insert(segment.to_owned(), value);
            return Some(());
        }
        let entry = current
            .entry(segment.to_owned())
            .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
        current = entry.as_object_mut()?;
    }
    None
}

fn merge_context(target: &mut JsonValue, source: JsonValue) -> Option<()> {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return None;
    };
    merge_context_objects(target, source)
}

fn merge_context_objects(
    target: &mut serde_json::Map<String, JsonValue>,
    source: &serde_json::Map<String, JsonValue>,
) -> Option<()> {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(existing), JsonValue::Object(source_object)) if existing.is_object() => {
                merge_context_objects(existing.as_object_mut()?, source_object)?;
            }
            (Some(existing), value) if existing != value => return None,
            (Some(_), _) => {}
            (None, value) => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    Some(())
}

fn empty_json_object() -> JsonValue {
    JsonValue::Object(serde_json::Map::new())
}

fn sanitize_id(value: &str) -> String {
    let mut sanitized = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | '.') {
            sanitized.push('-');
        }
    }
    let sanitized = sanitized.trim_matches('-').to_owned();
    if sanitized.is_empty() {
        "fixture".to_owned()
    } else {
        sanitized
    }
}

fn json_to_toml(value: &JsonValue) -> Result<toml::Value> {
    Ok(match value {
        JsonValue::Null => {
            return Err(RototoError::new(
                "fixture TOML cannot represent null values",
            ));
        }
        JsonValue::Bool(value) => toml::Value::Boolean(*value),
        JsonValue::Number(number) => {
            if let Some(value) = number.as_i64() {
                toml::Value::Integer(value)
            } else if let Some(value) = number.as_u64() {
                let value = i64::try_from(value).map_err(|_| {
                    RototoError::new("fixture TOML cannot represent integer larger than i64")
                })?;
                toml::Value::Integer(value)
            } else {
                toml::Value::Float(number.as_f64().ok_or_else(|| {
                    RototoError::new("fixture TOML cannot represent non-finite number")
                })?)
            }
        }
        JsonValue::String(value) => toml::Value::String(value.clone()),
        JsonValue::Array(values) => {
            let values = values
                .iter()
                .map(json_to_toml)
                .collect::<Result<Vec<_>>>()?;
            toml::Value::Array(values)
        }
        JsonValue::Object(values) => {
            let mut table = toml::map::Map::new();
            for (key, value) in values {
                table.insert(key.clone(), json_to_toml(value)?);
            }
            toml::Value::Table(table)
        }
    })
}

fn toml_to_json(value: &toml::Value) -> Result<JsonValue> {
    Ok(match value {
        toml::Value::String(value) => JsonValue::String(value.clone()),
        toml::Value::Integer(value) => JsonValue::Number(JsonNumber::from(*value)),
        toml::Value::Float(value) => JsonNumber::from_f64(*value)
            .map(JsonValue::Number)
            .ok_or_else(|| RototoError::new("fixture contains non-finite float"))?,
        toml::Value::Boolean(value) => JsonValue::Bool(*value),
        toml::Value::Datetime(value) => JsonValue::String(value.to_string()),
        toml::Value::Array(values) => JsonValue::Array(
            values
                .iter()
                .map(toml_to_json)
                .collect::<Result<Vec<_>>>()?,
        ),
        toml::Value::Table(values) => {
            let mut object = serde_json::Map::new();
            for (key, value) in values {
                object.insert(key.clone(), toml_to_json(value)?);
            }
            JsonValue::Object(object)
        }
    })
}

pub async fn load_fixture_suite(path: impl AsRef<Path>) -> Result<LoadedFixtureSuite> {
    let path = fixture_manifest_path(path.as_ref());
    let text = tokio::fs::read_to_string(&path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to read fixture suite {}: {err}",
            path.display()
        ))
    })?;
    let manifest: FixtureSuite = toml::from_str(&text).map_err(|err| {
        RototoError::new(format!(
            "failed to parse fixture suite {}: {err}",
            path.display()
        ))
    })?;
    if manifest.schema_version != 1 {
        return Err(RototoError::new(format!(
            "unsupported fixture suite schema_version: {}",
            manifest.schema_version
        )));
    }
    let root = path
        .parent()
        .ok_or_else(|| RototoError::new("fixture suite path has no parent directory"))?
        .to_path_buf();
    let mut files = Vec::new();
    for entry in &manifest.fixtures {
        let file_path = root.join(&entry.path);
        let text = tokio::fs::read_to_string(&file_path).await.map_err(|err| {
            RototoError::new(format!(
                "failed to read fixture {}: {err}",
                file_path.display()
            ))
        })?;
        let fixture: FixtureFile = toml::from_str(&text).map_err(|err| {
            RototoError::new(format!(
                "failed to parse fixture {}: {err}",
                file_path.display()
            ))
        })?;
        if fixture.schema_version != 1 {
            return Err(RototoError::new(format!(
                "unsupported fixture file schema_version in {}: {}",
                file_path.display(),
                fixture.schema_version
            )));
        }
        if fixture.target != entry.target {
            return Err(RototoError::new(format!(
                "fixture target mismatch in {}: manifest has {}, file has {}",
                file_path.display(),
                entry.target,
                fixture.target
            )));
        }
        files.push(LoadedFixtureFile {
            path: entry.path.clone(),
            fixture,
        });
    }
    Ok(LoadedFixtureSuite {
        path,
        manifest,
        files,
    })
}

fn fixture_manifest_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(SUITE_FILE)
    } else {
        path.to_path_buf()
    }
}

#[derive(Debug)]
pub struct LoadedFixtureSuite {
    pub path: PathBuf,
    pub manifest: FixtureSuite,
    pub files: Vec<LoadedFixtureFile>,
}

#[derive(Debug)]
pub struct LoadedFixtureFile {
    pub path: String,
    pub fixture: FixtureFile,
}

#[derive(Debug, Serialize)]
pub struct FixtureAssertionReport {
    pub cases: usize,
}

pub async fn assert_fixtures(
    package: &Package,
    suite_path: impl AsRef<Path>,
) -> Result<FixtureAssertionReport> {
    let suite = load_fixture_suite(suite_path).await?;
    let mut count = 0;
    for file in &suite.files {
        let target = FixtureTarget::parse(&file.fixture.target)?;
        for case in &file.fixture.cases {
            assert_fixture_case(package, &target, case)
                .await
                .map_err(|err| {
                    RototoError::new(format!(
                        "fixture failed: {} case {}: {err}",
                        file.fixture.target, case.id
                    ))
                })?;
            count += 1;
        }
    }
    Ok(FixtureAssertionReport { cases: count })
}

enum FixtureTarget {
    Qualifier(String),
    Variable(String),
}

impl FixtureTarget {
    fn parse(value: &str) -> Result<Self> {
        if let Some(id) = value.strip_prefix("qualifier:") {
            return Ok(Self::Qualifier(id.to_owned()));
        }
        if let Some(id) = value.strip_prefix("variable:") {
            return Ok(Self::Variable(id.to_owned()));
        }
        Err(RototoError::new(format!(
            "unsupported fixture target: {value}"
        )))
    }
}

async fn assert_fixture_case(
    package: &Package,
    target: &FixtureTarget,
    case: &FixtureCase,
) -> Result<()> {
    let context = toml_to_json(&case.context)?;
    let context = EvaluationContext::from_json(context)?;
    match target {
        FixtureTarget::Qualifier(id) => {
            let trace = trace_qualifier_resolution(package.root(), id, context.value()).await?;
            let expected = case.expect.value.as_bool().ok_or_else(|| {
                RototoError::new("qualifier fixture expect.value must be a boolean")
            })?;
            if trace.value != expected {
                return Err(RototoError::new(format!(
                    "expected qualifier value {expected}, got {}",
                    trace.value
                )));
            }
            assert_bucket_expectations(&case.expect.buckets, &[trace])?;
        }
        FixtureTarget::Variable(id) => {
            let trace = trace_variable_resolution(package.root(), id, context.value()).await?;
            let expected_value = toml_to_json(&case.expect.value)?;
            if trace.resolution.value != expected_value {
                return Err(RototoError::new(format!(
                    "expected value {}, got {}",
                    expected_value, trace.resolution.value
                )));
            }
            if case.expect.matched.as_deref() == Some("default")
                && trace.rules.iter().any(|rule| rule.matched)
            {
                return Err(RototoError::new(
                    "expected default resolution, but a rule matched",
                ));
            }
            if let Some(rule_index) = case.expect.matched_rule {
                let Some(rule) = trace
                    .rules
                    .iter()
                    .find(|rule| rule.index == rule_index && rule.matched)
                else {
                    return Err(RototoError::new(format!(
                        "expected matched rule {rule_index}, but it did not match"
                    )));
                };
                if let Some(expected_condition) = &case.expect.matched_condition
                    && &rule.condition != expected_condition
                {
                    return Err(RototoError::new(format!(
                        "expected matched condition {expected_condition}, got {}",
                        rule.condition
                    )));
                }
            }
            assert_bucket_expectations(&case.expect.buckets, &trace.qualifier_traces)?;
        }
    }
    Ok(())
}

fn assert_bucket_expectations(
    expected: &[FixtureBucketExpectation],
    traces: &[QualifierResolutionTrace],
) -> Result<()> {
    let _ = (expected, traces);
    Ok(())
}
