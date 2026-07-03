use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;
use serde_json::Value as JsonValue;

use rototo::model::{
    InspectSelection, PackageInspectRequest, PackageInspection, VariableResolutionTrace,
};
use rototo::{
    Result, RototoError, SourceOptions, diagnostics_catalog_for_package, inspect_package,
    inspect_package_report, trace_variable_resolution,
};

use crate::style;
use crate::{
    ResolveArgs, SelectedIds, Selection, TargetSelectors, package_source_or_current, parse_context,
    selected_variable_ids, validate_package_selectors,
};

pub(crate) async fn run_resolve(
    args: ResolveArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let selectors = TargetSelectors::from_resolve_args(&args.selectors);
    if !selectors.has_resolvable_targets() {
        return Err(RototoError::new(
            "resolve requires at least one --variable or --variables selector",
        ));
    }
    let package = package_source_or_current(args.package, source_options).await?;
    let inspection = inspect_package(package.path()).await?;
    let catalog = diagnostics_catalog_for_package(package.path()).await?;
    validate_package_selectors(&selectors, &inspection, &catalog)?;

    if args.context.is_empty() {
        let model = rototo::lint::package_semantic_model(package.path()).await?;
        let contexts =
            trace_sample_resolutions(package.path(), &inspection, &selectors, &model).await?;
        print_resolutions(package.path(), &[], &contexts, &[], json)?;
        return Ok(ExitCode::SUCCESS);
    }

    let context = parse_context(&args.context).await?;
    let context_gaps =
        resolve_context_gaps(package.path(), &inspection, &selectors, &context).await?;
    match trace_selected_resolutions(package.path(), &inspection, &selectors, &context).await {
        Ok(variables) => {
            print_resolutions(package.path(), &variables, &[], &context_gaps, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            // Resolution evaluates strictly and fails on the first missing or
            // mistyped attribute. Surface the full set of invocation gaps so the
            // caller can fix the context in one pass rather than one path at a time.
            if !context_gaps.is_empty() {
                print_resolutions(package.path(), &[], &[], &context_gaps, json)?;
            }
            Err(err)
        }
    }
}

async fn trace_selected_resolutions(
    package: &Path,
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    context: &JsonValue,
) -> Result<Vec<VariableResolutionTrace>> {
    let mut variables = Vec::new();
    for id in selected_variable_id_list(inspection, &selectors.variables) {
        variables.push(trace_variable_resolution(package, &id, context).await?);
    }
    Ok(variables)
}

async fn trace_sample_resolutions(
    package: &Path,
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    model: &rototo::lint::PackageSemanticModel,
) -> Result<Vec<ContextResolveOutput>> {
    let variable_ids = selected_variable_id_list(inspection, &selectors.variables);
    let variable_contexts = variable_evaluation_contexts(model);
    let variable_has_rules = variable_rule_presence(model);
    let samples = stored_evaluation_contexts(model);

    let mut requested_contexts = BTreeSet::new();
    let mut context_independent_variables = BTreeSet::new();
    for variable in &variable_ids {
        let contexts = variable_contexts.get(variable).cloned().unwrap_or_default();
        if contexts.is_empty() && !variable_has_rules.get(variable).copied().unwrap_or(false) {
            context_independent_variables.insert(variable.clone());
        } else {
            requested_contexts.extend(contexts);
        }
    }

    let mut runs = Vec::new();
    let mut resolved_variables = BTreeSet::new();
    for sample in samples
        .iter()
        .filter(|sample| requested_contexts.contains(&sample.evaluation_context))
    {
        let mut variables = Vec::new();
        for variable in &variable_ids {
            let contexts = variable_contexts.get(variable).cloned().unwrap_or_default();
            if contexts.contains(&sample.evaluation_context)
                || context_independent_variables.contains(variable)
            {
                variables.push(trace_variable_resolution(package, variable, &sample.value).await?);
                resolved_variables.insert(variable.clone());
            }
        }

        if !variables.is_empty() {
            runs.push(ContextResolveOutput {
                evaluation_context: Some(sample.evaluation_context.clone()),
                sample: Some(sample.key.clone()),
                variables,
            });
        }
    }

    let unresolved_context_independent = context_independent_variables
        .iter()
        .filter(|variable| !resolved_variables.contains(*variable))
        .cloned()
        .collect::<Vec<_>>();
    if !unresolved_context_independent.is_empty() {
        let empty_context = JsonValue::Object(serde_json::Map::new());
        let mut variables = Vec::new();
        for variable in unresolved_context_independent {
            variables.push(trace_variable_resolution(package, &variable, &empty_context).await?);
            resolved_variables.insert(variable);
        }
        runs.push(ContextResolveOutput {
            evaluation_context: None,
            sample: None,
            variables,
        });
    }

    let unresolved = variable_ids
        .iter()
        .filter(|variable| !resolved_variables.contains(*variable))
        .map(|variable| format!("variable://{variable}"))
        .collect::<Vec<_>>();
    if !unresolved.is_empty() {
        return Err(RototoError::new(format!(
            "no stored evaluation context sample matched selected target(s): {}",
            unresolved.join(", ")
        )));
    }

    Ok(runs)
}

#[derive(Debug)]
struct StoredEvaluationContext {
    evaluation_context: String,
    key: String,
    value: JsonValue,
}

fn stored_evaluation_contexts(
    model: &rototo::lint::PackageSemanticModel,
) -> Vec<StoredEvaluationContext> {
    model
        .evaluation_context_samples
        .iter()
        .filter_map(|entry| {
            entry.value.as_ref().map(|value| StoredEvaluationContext {
                evaluation_context: entry.evaluation_context.clone(),
                key: entry.key.clone(),
                value: value.clone(),
            })
        })
        .collect()
}

fn variable_evaluation_contexts(
    model: &rototo::lint::PackageSemanticModel,
) -> BTreeMap<String, BTreeSet<String>> {
    model
        .variable_evaluation_contexts
        .iter()
        .map(|compatibility| {
            (
                compatibility.variable.clone(),
                compatibility.evaluation_contexts.iter().cloned().collect(),
            )
        })
        .collect()
}

fn variable_rule_presence(model: &rototo::lint::PackageSemanticModel) -> BTreeMap<String, bool> {
    model
        .variables
        .iter()
        .map(|variable| {
            (
                variable.id.clone(),
                variable
                    .resolve
                    .as_ref()
                    .is_some_and(|resolve| !resolve.rules.is_empty()),
            )
        })
        .collect()
}

fn selected_variable_id_list(
    inspection: &PackageInspection,
    selection: &Selection<String>,
) -> Vec<String> {
    match selected_variable_ids(inspection, selection) {
        SelectedIds::None => Vec::new(),
        SelectedIds::Some(ids) => ids,
        SelectedIds::All => inspection
            .variables
            .iter()
            .map(|variable| variable.id.clone())
            .collect(),
    }
}

#[derive(Debug, Serialize)]
struct ResolveOutput<'a> {
    package: String,
    variables: &'a [VariableResolutionTrace],
    #[serde(skip_serializing_if = "is_empty_slice")]
    contexts: &'a [ContextResolveOutput],
    #[serde(skip_serializing_if = "is_empty_slice")]
    context_gaps: &'a [ContextResolveGap],
}

/// What a supplied `--context` is missing relative to what a resolved target's
/// expressions actually read. This is an invocation-time observation, distinct
/// from the package-static gaps that lint reports.
#[derive(Debug, Serialize)]
struct ContextResolveGap {
    target: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mismatched_paths: Vec<ContextResolveMismatch>,
}

#[derive(Debug, Serialize)]
struct ContextResolveMismatch {
    path: String,
    expected_types: Vec<String>,
    actual_type: String,
}

#[derive(Debug, Serialize)]
struct ContextResolveOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    evaluation_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample: Option<String>,
    variables: Vec<VariableResolutionTrace>,
}

fn is_empty_slice<T>(value: &&[T]) -> bool {
    value.is_empty()
}

async fn resolve_context_gaps(
    package: &Path,
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    context: &JsonValue,
) -> Result<Vec<ContextResolveGap>> {
    let variable_ids = selected_variable_id_list(inspection, &selectors.variables);
    let report = inspect_package_report(
        package,
        PackageInspectRequest {
            variables: id_selection(variable_ids),
            ..PackageInspectRequest::default()
        },
    )
    .await?;

    let mut gaps = Vec::new();
    for variable in &report.variables {
        if let Some(gap) = target_context_gap(
            &format!("variable://{}", variable.id),
            &variable.context_attributes,
            context,
        ) {
            gaps.push(gap);
        }
    }
    Ok(gaps)
}

fn id_selection(ids: Vec<String>) -> InspectSelection {
    if ids.is_empty() {
        InspectSelection::None
    } else {
        InspectSelection::Some(ids)
    }
}

fn target_context_gap(
    target: &str,
    attributes: &[rototo::model::ContextAttributeInspectReport],
    context: &JsonValue,
) -> Option<ContextResolveGap> {
    let mut missing_paths = Vec::new();
    let mut mismatched_paths = Vec::new();
    for attribute in attributes {
        let pointer = format!("/{}", attribute.path.replace('.', "/"));
        match context.pointer(&pointer) {
            None => missing_paths.push(attribute.path.clone()),
            Some(value) => {
                let actual = json_value_type_label(value);
                if !attribute.expected_types.is_empty()
                    && !attribute
                        .expected_types
                        .iter()
                        .any(|expected| expected == actual)
                {
                    mismatched_paths.push(ContextResolveMismatch {
                        path: attribute.path.clone(),
                        expected_types: attribute.expected_types.clone(),
                        actual_type: actual.to_owned(),
                    });
                }
            }
        }
    }
    if missing_paths.is_empty() && mismatched_paths.is_empty() {
        None
    } else {
        Some(ContextResolveGap {
            target: target.to_owned(),
            missing_paths,
            mismatched_paths,
        })
    }
}

fn json_value_type_label(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

fn print_resolutions(
    package: &Path,
    variables: &[VariableResolutionTrace],
    contexts: &[ContextResolveOutput],
    context_gaps: &[ContextResolveGap],
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ResolveOutput {
                package: package.display().to_string(),
                variables,
                contexts,
                context_gaps,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!(
        "{} {}",
        style::label("package"),
        style::bold(&package.display().to_string())
    );
    let count = variables.len();
    for (index, trace) in variables.iter().enumerate() {
        print_resolve_separator(index, count);
        print_variable_resolution_trace(trace)?;
    }
    if !contexts.is_empty() {
        let count = contexts.len();
        for (index, context) in contexts.iter().enumerate() {
            print_resolve_separator(index, count);
            print_context_resolution_trace(context)?;
        }
    }
    if !context_gaps.is_empty() {
        println!("{}", style::label("context gaps"));
        for gap in context_gaps {
            println!("  {}", style::sea(&gap.target));
            for path in &gap.missing_paths {
                println!(
                    "    {} {}",
                    style::warn("missing"),
                    style::info(&format!("context.{path}"))
                );
            }
            for mismatch in &gap.mismatched_paths {
                println!(
                    "    {} {} {}",
                    style::warn("type"),
                    style::info(&format!("context.{}", mismatch.path)),
                    style::dim(&format!(
                        "expected {}, got {}",
                        mismatch.expected_types.join(" or "),
                        mismatch.actual_type
                    ))
                );
            }
        }
    }
    Ok(())
}

fn print_resolve_separator(index: usize, count: usize) {
    if count > 1 && index > 0 {
        println!("{}", style::hairline());
    }
}

fn print_variable_resolution_trace(trace: &VariableResolutionTrace) -> Result<()> {
    println!("variable: {}", style::sea(&trace.resolution.id));
    println!("  {}", style::subhead("pathway"));
    for rule in &trace.rules {
        println!(
            "    {} if {} {} {} ({})",
            style::dim(&format!("rule[{}]", rule.index)),
            style::sea(&rule.condition),
            style::arrow(),
            compact_json(&rule.value)?,
            if rule.matched {
                style::ok("matched")
            } else {
                style::dim("skipped")
            }
        );
    }
    if let Some(allocation) = &trace.allocation {
        let assignment = match (&allocation.arm, allocation.enrolled) {
            (Some(arm), _) => format!(
                "bucket {} {} arm {}",
                allocation.bucket.unwrap_or_default(),
                style::arrow(),
                style::sea_bold(arm)
            ),
            (None, true) => match allocation.bucket {
                Some(bucket) => format!("bucket {bucket} {} no arm", style::arrow()),
                None => "no arm".to_owned(),
            },
            (None, false) => style::dim("not enrolled").to_string(),
        };
        println!(
            "    {} {}/{} {} {}",
            style::dim("allocation"),
            style::sea(&allocation.layer),
            style::sea(&allocation.allocation),
            style::arrow(),
            assignment
        );
    }
    println!(
        "    {} {} {}",
        style::dim("default"),
        style::arrow(),
        compact_json(&trace.default_value)?
    );
    println!("  {}", style::subhead("result"));
    println!(
        "    source: {}",
        style::sea_bold(&resolution_source_label(&trace.resolution.source))
    );
    println!("    value: {}", compact_json(&trace.resolution.value)?);
    Ok(())
}

fn resolution_source_label(source: &rototo::model::VariableResolutionSource) -> String {
    match source {
        rototo::model::VariableResolutionSource::Literal => "literal".to_owned(),
        rototo::model::VariableResolutionSource::Catalog { catalog, value } => {
            format!("{catalog}:{value}")
        }
        rototo::model::VariableResolutionSource::CatalogList { catalog, values } => {
            format!("{catalog}:[{}]", values.join(","))
        }
    }
}

fn print_context_resolution_trace(context: &ContextResolveOutput) -> Result<()> {
    match (&context.evaluation_context, &context.sample) {
        (Some(evaluation_context), Some(sample)) => {
            println!("evaluation context: {}", style::sea(evaluation_context));
            println!("sample: {}", style::info(sample));
        }
        _ => {
            println!("evaluation context: {}", style::dim("<none>"));
        }
    }

    let count = context.variables.len();
    for (index, trace) in context.variables.iter().enumerate() {
        print_resolve_separator(index, count);
        print_variable_resolution_trace(trace)?;
    }
    Ok(())
}

fn compact_json(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string(value).map_err(|err| RototoError::new(err.to_string()))
}
