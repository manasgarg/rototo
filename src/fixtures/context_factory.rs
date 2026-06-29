use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;

use crate::expression::{Expression, empty_context, merge_context};
use crate::model::{PackageInspectReport, VariableInspectReport};

/// A pool of candidate `context` objects for fixture generation.
///
/// Each context is either a stored evaluation-context sample or one synthesized
/// from a qualifier or variable-rule expression to drive a specific outcome
/// (a qualifier matching, a rule firing, the default falling through). Fixture
/// generation traces every candidate through real resolution and keeps the
/// first that produces the case it is after, so an imperfect synthesis is
/// discarded rather than emitted.
pub(super) struct ContextFactory {
    contexts: Vec<JsonValue>,
}

impl ContextFactory {
    pub(super) fn new(report: &PackageInspectReport) -> Self {
        // Parse every qualifier `when` once so synthesis can compose qualifiers
        // referenced from other qualifiers and from variable rules.
        let qualifiers: BTreeMap<String, Expression> = report
            .qualifiers
            .iter()
            .filter_map(|qualifier| {
                let source = qualifier.when.as_deref()?;
                let expression = Expression::parse(source).ok()?;
                Some((qualifier.id.clone(), expression))
            })
            .collect();

        let mut contexts = Vec::new();

        // Stored samples first: they are the most realistic and the printed
        // commands read best when grounded in a real sample.
        for evaluation_context in &report.evaluation_contexts {
            for sample in &evaluation_context.samples {
                contexts.push(sample.value.clone());
            }
        }

        // A context that drives each qualifier to each outcome.
        for qualifier in &report.qualifiers {
            for want in [true, false] {
                if let Some(context) = synthesize_qualifier(&qualifiers, &qualifier.id, want) {
                    contexts.push(context);
                }
            }
        }

        // For each variable: a context that falls through to the default, and
        // one that fires each rule (with earlier rules kept false so the rule
        // under test is the one that wins).
        for variable in &report.variables {
            push_variable_contexts(&qualifiers, variable, &mut contexts);
        }

        Self { contexts }
    }

    /// The candidate contexts to trace, in preference order.
    pub(super) fn candidate_contexts(&self) -> &[JsonValue] {
        &self.contexts
    }
}

fn push_variable_contexts(
    qualifiers: &BTreeMap<String, Expression>,
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
    if let Some(context) = merge_all(rules.iter().map(|rule| synth(qualifiers, rule, false))) {
        out.push(context);
    }

    for index in 0..rules.len() {
        // The rule's own satisfying context. Often this already falsifies the
        // earlier rules (a different equality value misses them), in which case
        // trace selection accepts it directly.
        if let Some(context) = synth(qualifiers, &rules[index], true) {
            out.push(context);
        }
        // A context that additionally forces every earlier rule false, for when
        // the bare context would be shadowed by an earlier rule. A path conflict
        // between the constraints simply drops this candidate; the bare one above
        // and the stored samples remain.
        let earlier = rules[..index]
            .iter()
            .map(|rule| synth(qualifiers, rule, false));
        let this = std::iter::once(synth(qualifiers, &rules[index], true));
        if let Some(context) = merge_all(earlier.chain(this)) {
            out.push(context);
        }
    }
}

/// Synthesize a context for one optional rule expression. A rule without a
/// `when` (a `query` rule, or a malformed expression) cannot be inverted and
/// yields `None`, which fails the surrounding merge.
fn synth(
    qualifiers: &BTreeMap<String, Expression>,
    rule: &Option<Expression>,
    want: bool,
) -> Option<JsonValue> {
    rule.as_ref()?.synthesize_context(want, &mut |id, want| {
        synthesize_qualifier(qualifiers, id, want)
    })
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

fn synthesize_qualifier(
    qualifiers: &BTreeMap<String, Expression>,
    id: &str,
    want: bool,
) -> Option<JsonValue> {
    synthesize_qualifier_inner(qualifiers, id, want, &mut BTreeSet::new())
}

fn synthesize_qualifier_inner(
    qualifiers: &BTreeMap<String, Expression>,
    id: &str,
    want: bool,
    stack: &mut BTreeSet<String>,
) -> Option<JsonValue> {
    if !stack.insert(id.to_owned()) {
        // A qualifier cycle; resolution would reject it, so synthesis bails.
        return None;
    }
    let result = qualifiers.get(id).and_then(|expression| {
        expression.synthesize_context(want, &mut |nested, nested_want| {
            synthesize_qualifier_inner(qualifiers, nested, nested_want, stack)
        })
    });
    stack.remove(id);
    result
}
