use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};
use crate::expression::simple_rule_qualifier;
use crate::model::{
    PackageInspectReport, PackageInspectRequest, QualifierInspectReport, QualifierResolutionTrace,
    RulePathwayInspectReport, VariableInspectReport, VariableResolutionTrace,
};
use crate::resolve::{trace_qualifier_resolution, trace_variable_resolution};
use crate::source::{SourceOptions, stage_package_source};

mod context_factory;
mod render;

use context_factory::ContextFactory;

pub use render::{ContextForm, render_command, render_comment};

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

/// A single `rototo resolve` invocation that exercises one behavior case of a
/// variable or qualifier. The CLI renders these into runnable command lines;
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
    Qualifier(String),
}

impl ResolveTarget {
    /// The `kind:id` label used in headers and JSON output.
    pub fn label(&self) -> String {
        match self {
            Self::Variable(id) => format!("variable:{id}"),
            Self::Qualifier(id) => format!("qualifier:{id}"),
        }
    }

    /// The resolve selector flag that targets this entity.
    pub fn selector_flag(&self) -> &'static str {
        match self {
            Self::Variable(_) => "--variable",
            Self::Qualifier(_) => "--qualifier",
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Variable(id) | Self::Qualifier(id) => id,
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
    Qualifier {
        value: bool,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MatchedBy {
    Default,
    Rule { index: usize, condition: String },
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
    let qualifier_ids = selected_qualifier_ids(&report, &selection.qualifiers)?;
    let factory = ContextFactory::new(&report);

    let mut invocations = Vec::new();

    for id in qualifier_ids {
        let qualifier = report
            .qualifiers
            .iter()
            .find(|qualifier| qualifier.id == id)
            .expect("selected qualifier id was validated");
        generate_qualifier_invocations(staged.path(), qualifier, &factory, &mut invocations)
            .await?;
    }

    for id in variable_ids {
        let variable = report
            .variables
            .iter()
            .find(|variable| variable.id == id)
            .expect("selected variable id was validated");
        generate_variable_invocations(staged.path(), variable, &factory, &mut invocations).await?;
    }

    Ok(invocations)
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

async fn generate_qualifier_invocations(
    package: &Path,
    qualifier: &QualifierInspectReport,
    factory: &ContextFactory<'_>,
    out: &mut Vec<ResolveInvocation>,
) -> Result<()> {
    let target = ResolveTarget::Qualifier(qualifier.id.clone());

    if let Some((context, trace)) =
        sampled_qualifier_context(package, &qualifier.id, true, factory).await?
    {
        out.push(ResolveInvocation {
            target: target.clone(),
            case_id: "matches".to_owned(),
            title: "Matches when the qualifier condition is true".to_owned(),
            because: Some(
                "An evaluation context sample satisfies the qualifier condition.".to_owned(),
            ),
            context,
            expect: ResolveExpectation::Qualifier { value: trace.value },
        });
    }

    if let Some((context, trace)) =
        sampled_qualifier_context(package, &qualifier.id, false, factory).await?
    {
        out.push(ResolveInvocation {
            target,
            case_id: "does-not-match".to_owned(),
            title: "Does not match when the qualifier condition is false".to_owned(),
            because: Some(
                "An evaluation context sample does not satisfy the qualifier condition.".to_owned(),
            ),
            context,
            expect: ResolveExpectation::Qualifier { value: trace.value },
        });
    }

    Ok(())
}

async fn generate_variable_invocations(
    package: &Path,
    variable: &VariableInspectReport,
    factory: &ContextFactory<'_>,
    out: &mut Vec<ResolveInvocation>,
) -> Result<()> {
    let target = ResolveTarget::Variable(variable.id.clone());

    if let Some(context) = variable_default_context(package, variable, factory).await? {
        let trace = trace_variable_resolution(package, &variable.id, &context).await?;
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

fn variable_expectation(trace: &VariableResolutionTrace) -> ResolveExpectation {
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
