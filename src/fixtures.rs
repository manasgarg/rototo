use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};
use crate::lint::{RuntimePackage, compile_runtime_package, parse_arm_buckets};
use crate::model::{
    AllocationInspectReport, PackageInspectReport, PackageInspectRequest, RulePathwayInspectReport,
    VariableInspectReport, VariableResolutionTrace,
};
use crate::resolve::{allocation_bucket, trace_variable_unchecked};
use crate::source::{SourceOptions, stage_package_source};

mod context_factory;
mod render;

use context_factory::ContextFactory;

pub use render::{ContextForm, render_command, render_comment};

#[derive(Clone, Debug, Default)]
pub struct FixtureGenerateSelection {
    pub variables: FixtureTargetSelection,
}

impl FixtureGenerateSelection {
    pub fn all() -> Self {
        Self {
            variables: FixtureTargetSelection::All,
        }
    }

    fn normalized(self) -> Self {
        if self.variables.is_none() {
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

/// A single `rototo resolve` invocation that exercises one behavior case of a
/// variable. The CLI renders these into runnable command lines;
/// nothing is persisted to disk.
#[derive(Clone, Debug)]
pub struct ResolveInvocation {
    pub target: ResolveTarget,
    pub case_id: String,
    pub title: String,
    pub because: Option<String>,
    pub context: JsonValue,
    pub expect: ResolveExpectation,
}

#[derive(Clone, Debug)]
pub enum ResolveTarget {
    Variable(String),
}

impl ResolveTarget {
    /// The `kind:id` label used in headers and JSON output.
    pub fn label(&self) -> String {
        match self {
            Self::Variable(id) => format!("variable:{id}"),
        }
    }

    /// The resolve selector flag that targets this entity.
    pub fn selector_flag(&self) -> &'static str {
        match self {
            Self::Variable(_) => "--variable",
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Variable(id) => id,
        }
    }
}

/// The expected result of a printed invocation, used to annotate each command
/// with its resolution outcome.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResolveExpectation {
    Variable {
        value: JsonValue,
        matched: MatchedBy,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MatchedBy {
    Default,
    Rule { index: usize, condition: String },
    Arm { allocation: String, arm: String },
}

pub async fn generate_resolve_invocations(
    package_source: impl AsRef<str>,
    source_options: &SourceOptions,
    selection: FixtureGenerateSelection,
) -> Result<Vec<ResolveInvocation>> {
    let package_source = package_source.as_ref();
    let selection = selection.normalized();
    let staged = stage_package_source(package_source.to_owned(), source_options).await?;
    let report =
        crate::inspect_package_report(staged.path(), PackageInspectRequest::default()).await?;

    let variable_ids = selected_variable_ids(&report, &selection.variables)?;
    let factory = ContextFactory::new(&report);
    // The fixture hunt traces many candidate contexts per variable; one
    // compiled runtime serves them all instead of recompiling per trace.
    let runtime = compile_runtime_package(staged.path()).await?;

    let mut invocations = Vec::new();

    for id in variable_ids {
        let variable = report
            .variables
            .iter()
            .find(|variable| variable.id == id)
            .expect("selected variable id was validated");
        generate_variable_invocations(&runtime, variable, &factory, &mut invocations)?;
    }

    Ok(invocations)
}

/// One traced resolution against the shared compiled runtime, with the same
/// per-variable context validation `trace_variable_resolution` performs.
fn trace_candidate(
    runtime: &RuntimePackage,
    id: &str,
    context: &JsonValue,
) -> Result<VariableResolutionTrace> {
    runtime.validate_context_for_variable(id, context)?;
    trace_variable_unchecked(runtime, id, context)
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

fn generate_variable_invocations(
    runtime: &RuntimePackage,
    variable: &VariableInspectReport,
    factory: &ContextFactory,
    out: &mut Vec<ResolveInvocation>,
) -> Result<()> {
    let target = ResolveTarget::Variable(variable.id.clone());

    if variable.resolve.method == "allocation" {
        if let Some(allocation) = &variable.resolve.allocation {
            generate_allocation_invocations(runtime, variable, allocation, factory, out)?;
        }
        return Ok(());
    }

    if let Some(context) = variable_default_context(runtime, variable, factory)? {
        let trace = trace_candidate(runtime, &variable.id, &context)?;
        if trace.rules.iter().any(|rule| rule.matched) {
            return Err(RototoError::new(format!(
                "generated default fixture matched a rule for variable: {}",
                variable.id
            )));
        }
        out.push(ResolveInvocation {
            target: target.clone(),
            case_id: "default".to_owned(),
            title: "Uses the default value when no rule matches".to_owned(),
            because: Some("Every rule condition is false.".to_owned()),
            context,
            expect: variable_expectation(&trace),
        });
    }

    for rule in &variable.resolve.rules {
        let Some(context) = variable_rule_context(runtime, variable, rule, factory)? else {
            continue;
        };
        let trace = trace_candidate(runtime, &variable.id, &context)?;
        if !trace
            .rules
            .iter()
            .any(|trace_rule| trace_rule.index == rule.index && trace_rule.matched)
        {
            continue;
        }
        let condition = rule_condition_label(rule);
        let case_id = format!("rule-{}-{}", rule.index, sanitize_id(&condition));
        let title = format!(
            "Rule {} selects {} when {} matches",
            rule.index,
            rule.value
                .as_ref()
                .map(serde_json::Value::to_string)
                .unwrap_or_else(|| "<missing>".to_owned()),
            condition
        );
        out.push(ResolveInvocation {
            target: target.clone(),
            case_id,
            title,
            because: Some("Earlier rule conditions are kept false when possible.".to_owned()),
            context,
            expect: variable_expectation(&trace),
        });
    }

    Ok(())
}

/// How many candidate unit ids to hash while hunting for one that lands in an
/// arm's bucket range. Arms claiming at least ~1/1000 of the line are found
/// comfortably; narrower arms are skipped rather than searched forever.
const MAX_UNIT_CANDIDATES: u32 = 8192;

/// Fixture cases for a `method = "allocation"` variable: one unit id per arm
/// (found by hashing candidates against the layer's diversion, exactly the way
/// resolution will) plus a no-arm case that lands on the default.
fn generate_allocation_invocations(
    runtime: &RuntimePackage,
    variable: &VariableInspectReport,
    allocation: &AllocationInspectReport,
    factory: &ContextFactory,
    out: &mut Vec<ResolveInvocation>,
) -> Result<()> {
    let target = ResolveTarget::Variable(variable.id.clone());

    // The no-arm case: the unit is not enrolled or lands in unclaimed buckets.
    for context in factory.candidates_for(&variable.id) {
        if let Ok(trace) = trace_candidate(runtime, &variable.id, context)
            && trace
                .allocation
                .as_ref()
                .is_none_or(|allocation| allocation.arm.is_none())
        {
            out.push(ResolveInvocation {
                target: target.clone(),
                case_id: "default".to_owned(),
                title: "Uses the default value when the unit is in no arm".to_owned(),
                because: Some("The unit is not enrolled or lands in unclaimed buckets.".to_owned()),
                context: context.clone(),
                expect: variable_expectation(&trace),
            });
            break;
        }
    }

    let (Some(layer), Some(unit), Some(buckets)) =
        (&allocation.layer, &allocation.unit, allocation.buckets)
    else {
        return Ok(());
    };
    if buckets < 1 {
        return Ok(());
    }
    let buckets = buckets as u32;
    // Synthesis places the unit value at the diversion's context path, so it
    // only works when `unit` is a plain path like `context.user.id`.
    let Some(unit_path) = plain_context_path(unit) else {
        return Ok(());
    };

    for arm in &allocation.arms {
        let (Some(name), Some(range)) = (&arm.name, &arm.buckets) else {
            continue;
        };
        let Some((start, end)) = parse_arm_buckets(range) else {
            continue;
        };
        let Some(unit_value) = (0..MAX_UNIT_CANDIDATES).find_map(|index| {
            let candidate = format!("{name}-unit-{index:04}");
            let bucket = allocation_bucket(layer, &JsonValue::String(candidate.clone()), buckets);
            (bucket >= start && bucket <= end).then_some(candidate)
        }) else {
            continue;
        };

        // Verify by real resolution against each candidate base context; the
        // first base that enrolls the unit (eligibility can depend on other
        // context facts) wins.
        for base in factory.candidates_for(&variable.id) {
            let mut context = base.clone();
            set_context_path(
                &mut context,
                &unit_path,
                JsonValue::String(unit_value.clone()),
            );
            let Ok(trace) = trace_candidate(runtime, &variable.id, &context) else {
                continue;
            };
            let assigned = trace
                .allocation
                .as_ref()
                .and_then(|allocation| allocation.arm.as_deref());
            if assigned != Some(name.as_str()) {
                continue;
            }
            let value = trace.resolution.value.clone();
            out.push(ResolveInvocation {
                target: target.clone(),
                case_id: format!("arm-{}", sanitize_id(name)),
                title: format!("Arm {name} assigns {value} when the unit lands in its buckets"),
                because: Some(format!(
                    "The unit id hashes into buckets {range} of layer {layer}."
                )),
                context,
                expect: variable_expectation(&trace),
            });
            break;
        }
    }

    Ok(())
}

/// `context.a.b` as its path segments, or `None` when the expression is not a
/// plain dotted context path.
fn plain_context_path(source: &str) -> Option<Vec<String>> {
    let path = source.trim().strip_prefix("context.")?;
    let segments: Vec<String> = path.split('.').map(str::to_owned).collect();
    segments
        .iter()
        .all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
        .then_some(segments)
}

/// Force-set a dotted path in a context object, replacing whatever value the
/// base context carried there.
fn set_context_path(context: &mut JsonValue, path: &[String], value: JsonValue) {
    if !context.is_object() {
        *context = JsonValue::Object(serde_json::Map::new());
    }
    let mut current = context;
    for segment in &path[..path.len() - 1] {
        let object = current.as_object_mut().expect("object ensured above");
        let entry = object
            .entry(segment.clone())
            .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = JsonValue::Object(serde_json::Map::new());
        }
        current = entry;
    }
    current
        .as_object_mut()
        .expect("object ensured above")
        .insert(path[path.len() - 1].clone(), value);
}

fn variable_expectation(trace: &VariableResolutionTrace) -> ResolveExpectation {
    if let Some(allocation) = &trace.allocation
        && let Some(arm) = &allocation.arm
    {
        return ResolveExpectation::Variable {
            value: trace.resolution.value.clone(),
            matched: MatchedBy::Arm {
                allocation: allocation.allocation.clone(),
                arm: arm.clone(),
            },
        };
    }
    let matched = trace.rules.iter().find(|rule| rule.matched);
    ResolveExpectation::Variable {
        value: trace.resolution.value.clone(),
        matched: match matched {
            Some(rule) => MatchedBy::Rule {
                index: rule.index,
                condition: rule.condition.clone(),
            },
            None => MatchedBy::Default,
        },
    }
}

fn rule_condition_label(rule: &RulePathwayInspectReport) -> String {
    rule.when.as_deref().unwrap_or("<missing>").to_owned()
}

/// The first candidate context that drives `rule` to win for `variable`.
/// Selection is by real resolution, so the rule under test must be the one that
/// actually matches (earlier rules kept false), not merely have a true `when`.
fn variable_rule_context(
    runtime: &RuntimePackage,
    variable: &VariableInspectReport,
    rule: &RulePathwayInspectReport,
    factory: &ContextFactory,
) -> Result<Option<JsonValue>> {
    for context in factory.candidates_for(&variable.id) {
        if let Ok(trace) = trace_candidate(runtime, &variable.id, context)
            && trace
                .rules
                .iter()
                .any(|trace_rule| trace_rule.index == rule.index && trace_rule.matched)
        {
            return Ok(Some(context.clone()));
        }
    }

    Ok(None)
}

/// The first candidate context under which no rule matches, so `variable`
/// resolves to its default.
fn variable_default_context(
    runtime: &RuntimePackage,
    variable: &VariableInspectReport,
    factory: &ContextFactory,
) -> Result<Option<JsonValue>> {
    for context in factory.candidates_for(&variable.id) {
        if let Ok(trace) = trace_candidate(runtime, &variable.id, context)
            && trace.rules.iter().all(|rule| !rule.matched)
        {
            return Ok(Some(context.clone()));
        }
    }

    Ok(None)
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
