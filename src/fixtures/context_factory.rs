use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;

use crate::expression::{Expression, empty_context, merge_context};
use crate::model::{PackageInspectReport, VariableInspectReport};

/// A pool of candidate `context` objects for fixture generation.
///
/// Each context is either a stored evaluation-context sample or one synthesized
/// from a variable-rule expression to drive a specific outcome (a condition
/// variable matching, a rule firing, the default falling through). Fixture
/// generation traces every candidate through real resolution and keeps the
/// first that produces the case it is after, so an imperfect synthesis is
/// discarded rather than emitted.
pub(super) struct ContextFactory {
    contexts: Vec<JsonValue>,
}

/// The invertible conditions of a package: for each bool "condition" variable,
/// the `when` expressions of its literal-`true` rules. Synthesis composes them
/// when an expression references a variable (`variables.id`).
struct Conditions {
    /// Bool variables in the simple condition shape (rules carrying literal
    /// `true` values over a `false` default). Driving one true satisfies any
    /// rule's `when`; driving it false falsifies every rule's `when` so the
    /// default falls through. Variables outside that shape synthesize `None`
    /// and the surrounding candidate is dropped by trace verification.
    variables: BTreeMap<String, Vec<Expression>>,
}

impl ContextFactory {
    pub(super) fn new(report: &PackageInspectReport) -> Self {
        let conditions = Conditions::new(report);

        let mut contexts = Vec::new();

        // Stored samples first: they are the most realistic and the printed
        // commands read best when grounded in a real sample.
        for evaluation_context in &report.evaluation_contexts {
            for sample in &evaluation_context.samples {
                contexts.push(sample.value.clone());
            }
        }

        // For each variable: a context that falls through to the default, and
        // one that fires each rule (with earlier rules kept false so the rule
        // under test is the one that wins).
        for variable in &report.variables {
            push_variable_contexts(&conditions, variable, &mut contexts);
        }

        Self { contexts }
    }

    /// The candidate contexts to trace, in preference order.
    pub(super) fn candidate_contexts(&self) -> &[JsonValue] {
        &self.contexts
    }
}

impl Conditions {
    fn new(report: &PackageInspectReport) -> Self {
        let variables: BTreeMap<String, Vec<Expression>> = report
            .variables
            .iter()
            .filter_map(|variable| {
                if variable.resolve.default_value != Some(JsonValue::Bool(false)) {
                    return None;
                }
                let rules: Option<Vec<Expression>> = variable
                    .resolve
                    .rules
                    .iter()
                    .map(|rule| {
                        (rule.value == Some(JsonValue::Bool(true)))
                            .then_some(rule.when.as_deref())
                            .flatten()
                            .and_then(|source| Expression::parse(source).ok())
                    })
                    .collect();
                Some((variable.id.clone(), rules?))
            })
            .collect();

        Self { variables }
    }

    fn synthesize(&self, id: &str, want: bool, stack: &mut BTreeSet<String>) -> Option<JsonValue> {
        if !stack.insert(id.to_owned()) {
            // A reference cycle; resolution would reject it, so synthesis bails.
            return None;
        }
        let result = self.synthesize_condition_variable(id, want, stack);
        stack.remove(id);
        result
    }

    /// Drive a condition-shaped bool variable to `want`: true means one of its
    /// rules fires, false means every rule stays quiet and the `false` default
    /// falls through.
    fn synthesize_condition_variable(
        &self,
        id: &str,
        want: bool,
        stack: &mut BTreeSet<String>,
    ) -> Option<JsonValue> {
        let rules = self.variables.get(id)?;
        let mut synthesize = |rule: &Expression, want: bool| -> Option<JsonValue> {
            rule.synthesize_context(
                want,
                &mut |nested, nested_want| self.synthesize(nested, nested_want, stack),
                // The inspect report does not carry enum members, so enum
                // memberships stay uninvertible here; the candidate is simply
                // dropped by trace verification.
                &mut |_| None,
            )
        };
        if want {
            rules.iter().find_map(|rule| synthesize(rule, true))
        } else {
            let mut merged = empty_context();
            for rule in rules {
                merge_context(&mut merged, synthesize(rule, false)?)?;
            }
            Some(merged)
        }
    }
}

fn push_variable_contexts(
    conditions: &Conditions,
    variable: &VariableInspectReport,
    out: &mut Vec<JsonValue>,
) {
    let rules: Vec<Option<Expression>> = variable
        .resolve
        .rules
        .iter()
        .map(|rule| {
            rule.when
                .as_deref()
                .and_then(|source| Expression::parse(source).ok())
        })
        .collect();

    // Default: every rule false at once.
    if let Some(context) = merge_all(rules.iter().map(|rule| synth(conditions, rule, false))) {
        out.push(context);
    }

    for index in 0..rules.len() {
        // The rule's own satisfying context. Often this already falsifies the
        // earlier rules (a different equality value misses them), in which case
        // trace selection accepts it directly.
        if let Some(context) = synth(conditions, &rules[index], true) {
            out.push(context);
        }
        // A context that additionally forces every earlier rule false, for when
        // the bare context would be shadowed by an earlier rule. A path conflict
        // between the constraints simply drops this candidate; the bare one above
        // and the stored samples remain.
        let earlier = rules[..index]
            .iter()
            .map(|rule| synth(conditions, rule, false));
        let this = std::iter::once(synth(conditions, &rules[index], true));
        if let Some(context) = merge_all(earlier.chain(this)) {
            out.push(context);
        }
    }
}

/// Synthesize a context for one optional rule expression. A rule without a
/// `when` (a `query` rule, or a malformed expression) cannot be inverted and
/// yields `None`, which fails the surrounding merge.
fn synth(conditions: &Conditions, rule: &Option<Expression>, want: bool) -> Option<JsonValue> {
    rule.as_ref()?.synthesize_context(
        want,
        &mut |id, want| conditions.synthesize(id, want, &mut BTreeSet::new()),
        &mut |_| None,
    )
}

/// Merge a sequence of optional contexts into one, failing if any is `None` or
/// if two disagree on a path.
fn merge_all(contexts: impl IntoIterator<Item = Option<JsonValue>>) -> Option<JsonValue> {
    let mut merged = empty_context();
    for context in contexts {
        merge_context(&mut merged, context?)?;
    }
    Some(merged)
}
