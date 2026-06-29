use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Number as JsonNumber, Value as JsonValue};

use crate::expression::simple_rule_qualifier;
use crate::model::{
    PackageInspectReport, PredicateInspectReport, QualifierInspectReport, RulePathwayInspectReport,
    VariableInspectReport,
};
use crate::resolve::bucket_value;

use super::sanitize_id;

const MAX_BUCKET_CANDIDATES: usize = 100_000;

pub(super) struct ContextFactory<'a> {
    qualifiers: BTreeMap<String, &'a QualifierInspectReport>,
    samples: Vec<JsonValue>,
}

impl<'a> ContextFactory<'a> {
    pub(super) fn new(report: &'a PackageInspectReport) -> Self {
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

    pub(super) fn sample_contexts(&self) -> &[JsonValue] {
        &self.samples
    }

    pub(super) fn variable_default_context(
        &self,
        variable: &VariableInspectReport,
    ) -> Option<JsonValue> {
        let mut context = empty_json_object();
        for rule in &variable.resolve.rules {
            let qualifier = simple_rule_qualifier(rule.when.as_deref()?)?;
            merge_context(&mut context, self.qualifier_context(&qualifier, false)?)?;
        }
        Some(context)
    }

    pub(super) fn variable_rule_context(
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

    pub(super) fn qualifier_context(&self, id: &str, desired: bool) -> Option<JsonValue> {
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
