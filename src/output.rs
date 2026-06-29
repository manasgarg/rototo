use std::path::Path;

use serde::Serialize;
use toml::Value as TomlValue;

use crate::style;

use rototo::diagnostics::{
    DiagnosticCatalogEntry, DiagnosticEntity, DiagnosticLocation, LintDiagnostic, SemanticEntity,
    SemanticField, SemanticTarget, Severity,
};
use rototo::error::{Result, RototoError};
use rototo::model::{InspectRuntimeStatus, PackageDiff, PackageInspectReport};
use rototo::model::{PackageInspection, PackageLint};
use rototo::package::{
    catalog_for_id, qualifier_for_id, read_catalog_json, read_toml, read_variable_toml,
    variable_for_id,
};

#[derive(Debug, Serialize)]
struct PackageFileJson<'a> {
    id: &'a str,
    uri: &'a str,
    path: String,
}

#[derive(Debug, Serialize)]
struct PackageLintJson<'a> {
    package: String,
    documents: &'a [rototo::model::SourceDocumentSummary],
    diagnostics: &'a [LintDiagnostic],
}

#[derive(Debug, Serialize)]
struct QualifierListJson<'a> {
    package: String,
    qualifiers: Vec<PackageFileJson<'a>>,
}

#[derive(Debug, Serialize)]
struct VariableListJson<'a> {
    package: String,
    variables: Vec<PackageFileJson<'a>>,
}

#[derive(Debug, Serialize)]
struct CatalogListJson<'a> {
    package: String,
    catalogs: Vec<PackageFileJson<'a>>,
}

#[derive(Debug, Serialize)]
struct QualifierGetJson {
    package: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct VariableGetJson {
    package: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct CatalogGetJson {
    package: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
}

pub(crate) fn print_package_lint(lint: &PackageLint, json: bool, quiet: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&PackageLintJson {
                package: lint.root.display().to_string(),
                documents: &lint.documents,
                diagnostics: &lint.diagnostics,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    if lint.diagnostics.is_empty() {
        if quiet {
            return Ok(());
        }
        println!("{}", style::ok_line(&lint.root.display().to_string()));
        return Ok(());
    }

    print_diagnostics(&lint.diagnostics);
    Ok(())
}

pub(crate) fn print_inspect_report(report: &PackageInspectReport, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!(
        "{} {}",
        style::label("package"),
        style::bold(&report.package)
    );
    match &report.runtime {
        InspectRuntimeStatus::Available => {
            println!("{} {}", style::label("runtime"), style::ok("available"))
        }
        InspectRuntimeStatus::Unavailable { reason } => {
            println!("{} {}", style::label("runtime"), style::warn("unavailable"));
            println!("  reason: {reason}");
        }
    }

    if !report.diagnostics.is_empty() {
        println!("{}", style::label("diagnostics"));
        print_diagnostics(&report.diagnostics);
    }

    if !report.evaluation_contexts.is_empty() {
        println!("{}", style::label("evaluation contexts"));
        let count = report.evaluation_contexts.len();
        for (index, evaluation_context) in report.evaluation_contexts.iter().enumerate() {
            print_entity_separator(index, count);
            println!(
                "  evaluation context: {}",
                style::sea(&evaluation_context.id)
            );
            println!(
                "    {} {}",
                style::dim("path:"),
                style::dim(&evaluation_context.path)
            );
            println!(
                "    {} {}",
                style::dim("status:"),
                if evaluation_context.status == "valid" {
                    style::ok(&evaluation_context.status)
                } else {
                    style::err(&evaluation_context.status)
                }
            );
            if let Some(title) = &evaluation_context.title {
                println!("    title: {title}");
            }
            if let Some(description) = &evaluation_context.description {
                println!("    description: {description}");
            }
            if !evaluation_context.samples.is_empty() {
                println!("    {}", style::subhead("samples"));
                for sample in &evaluation_context.samples {
                    println!(
                        "      {} = {}",
                        style::sea(&sample.key),
                        compact_json(&sample.value)?
                    );
                }
            }
            if !evaluation_context.diagnostics.is_empty() {
                println!("    {}", style::subhead("diagnostics"));
                print_diagnostics(&evaluation_context.diagnostics);
            }
        }
    }

    if !report.qualifiers.is_empty() {
        println!("{}", style::label("qualifiers"));
        let count = report.qualifiers.len();
        for (index, qualifier) in report.qualifiers.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  qualifier: {}", style::sea(&qualifier.id));
            if let Some(description) = &qualifier.description {
                println!("    description: {description}");
            }
            if let Some(when) = &qualifier.when {
                println!("    {} {}", style::subhead("when"), style::info(when));
            }
            print_compatible_evaluation_contexts(&qualifier.evaluation_contexts, "    ");
            print_context_attributes(&qualifier.context_attributes, "    ");
            print_qualifier_sample_coverage(qualifier.sample_coverage.as_ref(), "    ");
            print_dependencies(&qualifier.dependencies, "    ");
            if !qualifier.consumers.is_empty() {
                println!("    {}", style::subhead("consumed by"));
                for consumer in &qualifier.consumers {
                    println!(
                        "      {}  {}",
                        style::sea(&consumer.label),
                        style::dim(&consumer.location.path)
                    );
                }
            }
            if !qualifier.diagnostics.is_empty() {
                println!("    {}", style::subhead("diagnostics"));
                print_diagnostics(&qualifier.diagnostics);
            }
            if let Some(trace) = &qualifier.trace {
                println!(
                    "    {} {}",
                    style::dim("trace:"),
                    if trace.value {
                        style::ok("true")
                    } else {
                        style::dim("false")
                    }
                );
                println!(
                    "      {} {}",
                    style::subhead("when"),
                    style::info(&trace.when)
                );
            }
        }
    }

    if !report.catalogs.is_empty() {
        println!("{}", style::label("catalogs"));
        let count = report.catalogs.len();
        for (index, catalog) in report.catalogs.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  catalog: {}", style::sea(&catalog.id));
            println!("    {} {}", style::dim("path:"), style::dim(&catalog.path));
            if let Some(description) = &catalog.description {
                println!("    description: {description}");
            }
            if let Some(schema) = &catalog.schema {
                println!("    schema: {}", style::info(schema));
            }
            if !catalog.entries.is_empty() {
                println!("    {}", style::subhead("values"));
                for entry in &catalog.entries {
                    println!(
                        "      {} = {}",
                        style::sea(&entry.key),
                        compact_json(&entry.value)?
                    );
                }
            }
            print_dependencies(&catalog.dependencies, "    ");
            if !catalog.consumers.is_empty() {
                println!("    {}", style::subhead("consumed by"));
                for consumer in &catalog.consumers {
                    println!(
                        "      {}  {}",
                        style::sea(&consumer.label),
                        style::dim(&consumer.location.path)
                    );
                }
            }
            if !catalog.diagnostics.is_empty() {
                println!("    {}", style::subhead("diagnostics"));
                print_diagnostics(&catalog.diagnostics);
            }
        }
    }

    if !report.variables.is_empty() {
        println!("{}", style::label("variables"));
        let count = report.variables.len();
        for (index, variable) in report.variables.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  variable: {}", style::sea(&variable.id));
            if let Some(description) = &variable.description {
                println!("    description: {description}");
            }
            println!(
                "    {} {}",
                style::dim("type:"),
                style::info(&variable.type_source)
            );
            if let Some(schema) = &variable.schema {
                println!("    schema: {}", style::info(schema));
            }
            if !variable.values.is_empty() {
                println!("    {}", style::subhead("values"));
                for value in &variable.values {
                    println!(
                        "      {} {} = {}",
                        style::sea(&value.key),
                        style::dim(&format!("({})", value.origin)),
                        compact_json(&value.value)?
                    );
                }
            }
            if variable.resolve.default_value.is_some() || !variable.resolve.rules.is_empty() {
                println!("    {}", style::subhead("resolve"));
                for rule in &variable.resolve.rules {
                    let condition = variable_rule_condition(rule);
                    let value = compact_json_option(&rule.value)?;
                    println!(
                        "      {} if {} {} {}",
                        style::dim(&format!("rule[{}]", rule.index)),
                        style::sea(condition),
                        style::arrow(),
                        value
                    );
                }
                let default = compact_json_option(&variable.resolve.default_value)?;
                println!(
                    "      {} {} {default}",
                    style::dim("default"),
                    style::arrow()
                );
            }
            print_compatible_evaluation_contexts(&variable.evaluation_contexts, "    ");
            print_context_attributes(&variable.context_attributes, "    ");
            print_variable_sample_coverage(variable.sample_coverage.as_ref(), "    ");
            print_dependencies(&variable.dependencies, "    ");
            if !variable.diagnostics.is_empty() {
                println!("    {}", style::subhead("diagnostics"));
                print_diagnostics(&variable.diagnostics);
            }
            if let Some(trace) = &variable.trace {
                println!(
                    "    trace: {}",
                    resolution_source_label(&trace.resolution.source)
                );
                for rule in &trace.rules {
                    println!(
                        "      {} if {} {} {} ({})",
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
            }
        }
    }

    if !report.lint_rules.is_empty() {
        println!("{}", style::label("lint rules"));
        let count = report.lint_rules.len();
        for (index, rule) in report.lint_rules.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  lint rule: {}", style::sea(&rule.rule));
            println!("    severity: {}", severity_label(&rule.severity));
            println!("    title: {}", rule.title);
            if !rule.diagnostics.is_empty() {
                print_diagnostics(&rule.diagnostics);
            }
        }
    }

    if !report.lint_authorities.is_empty() {
        println!("{}", style::label("lint authorities"));
        let count = report.lint_authorities.len();
        for (index, authority) in report.lint_authorities.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  lint authority: {}", style::sea(&authority.authority));
            for rule in &authority.rules {
                println!("    {}  {}", rule.rule, rule.title);
            }
        }
    }

    if !report.linters.is_empty() {
        println!("{}", style::label("linters"));
        let count = report.linters.len();
        for (index, linter) in report.linters.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  linter: {}", style::sea(&linter.id));
            println!("    path: {}", linter.path);
            if !linter.registrations.is_empty() {
                println!("    {}", style::subhead("registrations"));
            }
            for (registration_index, registration) in linter.registrations.iter().enumerate() {
                println!("      [{}] {}", registration_index, registration.rule);
                println!(
                    "        target: {}",
                    linter_registration_target(registration)
                );
                println!("        runs during: {} lint stage", registration.stage);
                println!("        handler: {}", registration.handler);
            }
            if !linter.diagnostics.is_empty() {
                print_diagnostics(&linter.diagnostics);
            }
        }
    }
    Ok(())
}

fn print_compatible_evaluation_contexts(evaluation_contexts: &[String], indent: &str) {
    if evaluation_contexts.is_empty() {
        return;
    }
    println!("{indent}{}", style::subhead("evaluation contexts"));
    for evaluation_context in evaluation_contexts {
        println!("{indent}  {}", style::sea(evaluation_context));
    }
}

fn print_qualifier_sample_coverage(
    coverage: Option<&rototo::model::QualifierSampleCoverageReport>,
    indent: &str,
) {
    let Some(coverage) = coverage else {
        return;
    };
    let mark = |covered: bool, label: &str| {
        if covered {
            style::ok(label)
        } else {
            style::warn(&format!("{label} (no sample)"))
        }
    };
    println!(
        "{indent}{} {}  {}  {}",
        style::subhead("sample coverage"),
        style::dim(&format!("{} sample(s)", coverage.sample_count)),
        mark(coverage.evaluated_true, "true"),
        mark(coverage.evaluated_false, "false"),
    );
}

fn print_variable_sample_coverage(
    coverage: Option<&rototo::model::VariableSampleCoverageReport>,
    indent: &str,
) {
    let Some(coverage) = coverage else {
        return;
    };
    println!(
        "{indent}{} {}",
        style::subhead("sample coverage"),
        style::dim(&format!("{} sample(s)", coverage.sample_count)),
    );
    let default_mark = if coverage.default_covered {
        style::ok("covered")
    } else {
        style::warn("no sample")
    };
    println!("{indent}  {} {}", style::dim("default"), default_mark);
    for rule in &coverage.rules {
        let mark = if rule.covered {
            style::ok("covered")
        } else {
            style::warn("no sample")
        };
        println!(
            "{indent}  {} {}",
            style::dim(&format!("rule {}", rule.index)),
            mark
        );
    }
}

fn print_context_attributes(
    attributes: &[rototo::model::ContextAttributeInspectReport],
    indent: &str,
) {
    if attributes.is_empty() {
        return;
    }
    println!("{indent}{}", style::subhead("context attributes"));
    for attribute in attributes {
        let expected = if attribute.expected_types.is_empty() {
            String::new()
        } else {
            format!(
                " {}",
                style::dim(&format!(
                    "used as {}",
                    attribute.expected_types.join(" or ")
                ))
            )
        };
        let declared = attribute
            .declarations
            .iter()
            .map(|declaration| {
                let types = if declaration.declared_types.is_empty() {
                    "untyped".to_owned()
                } else {
                    declaration.declared_types.join("|")
                };
                format!("{}:{}", declaration.evaluation_context, types)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = match attribute.status.as_str() {
            "undeclared" => format!("  {}", style::err("undeclared")),
            "type_mismatch" => format!(
                "  {} {}",
                style::err("type mismatch"),
                style::dim(&declared)
            ),
            _ => format!("  {}", style::dim(&declared)),
        };
        println!(
            "{indent}  {} {}{}{}",
            style::dim("context"),
            style::info(&attribute.path),
            expected,
            suffix
        );
    }
}

pub(crate) fn print_package_diff(diff: &PackageDiff, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(diff).map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("{} {}", style::label("before"), style::bold(&diff.before));
    println!("{} {}", style::label("after"), style::bold(&diff.after));
    if diff.changes.is_empty() {
        println!(
            "{} {}",
            style::label("semantic changes"),
            style::dim("none")
        );
    } else {
        println!("{}", style::label("semantic changes"));
        for change in &diff.changes {
            print_semantic_change_header(change);
            if let Some(location) = &change.before_location {
                println!(
                    "    {} {}",
                    style::dim("before location:"),
                    style::info(&diagnostic_location_label_for_location(location))
                );
            }
            if let Some(location) = &change.after_location {
                println!(
                    "    {} {}",
                    style::dim("after location:"),
                    style::info(&diagnostic_location_label_for_location(location))
                );
            }
            if change.before.is_some() || change.after.is_some() {
                print_diff_value_line("before", &change.before, DiffSide::Before)?;
                print_diff_value_line("after", &change.after, DiffSide::After)?;
            }
        }
    }
    if !diff.resolution_impacts.is_empty() {
        println!("{}", style::label("resolution impact"));
        for impact in &diff.resolution_impacts {
            println!(
                "  {} {}",
                style::dim("variable:"),
                style::sea(&impact.variable)
            );
            print_resolution_impact_line("before", &impact.before, DiffSide::Before)?;
            print_resolution_impact_line("after", &impact.after, DiffSide::After)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum DiffSide {
    Before,
    After,
}

fn print_semantic_change_header(change: &rototo::model::SemanticChange) {
    let target = semantic_target_label(&change.target);
    let description = semantic_change_description(&change.kind);
    if !style::enabled() {
        println!("  {}  {}", change.kind, target);
        println!("    change: {description}");
        return;
    }

    let marker = semantic_change_marker(change);
    println!(
        "  {} {}  {}",
        style_diff_marker(marker),
        style_diff_kind(description, marker),
        style::sea(&target)
    );
    println!("    {} {}", style::dim("kind:"), style::dim(&change.kind));
}

fn semantic_change_marker(change: &rototo::model::SemanticChange) -> DiffChangeMarker {
    if change.kind.ends_with("_added") || (change.before.is_none() && change.after.is_some()) {
        DiffChangeMarker::Added
    } else if change.kind.ends_with("_removed")
        || (change.before.is_some() && change.after.is_none())
    {
        DiffChangeMarker::Removed
    } else {
        DiffChangeMarker::Changed
    }
}

#[derive(Clone, Copy)]
enum DiffChangeMarker {
    Added,
    Removed,
    Changed,
}

fn style_diff_marker(marker: DiffChangeMarker) -> String {
    match marker {
        DiffChangeMarker::Added => style::ok("+"),
        DiffChangeMarker::Removed => style::err("−"),
        DiffChangeMarker::Changed => style::warn("~"),
    }
}

fn style_diff_kind(kind: &str, marker: DiffChangeMarker) -> String {
    match marker {
        DiffChangeMarker::Added => style::ok(kind),
        DiffChangeMarker::Removed => style::err(kind),
        DiffChangeMarker::Changed => style::warn(kind),
    }
}

fn semantic_change_description(kind: &str) -> &'static str {
    match kind {
        "variable_added" => "variable added",
        "variable_removed" => "variable removed",
        "variable_type_changed" => "variable type changed",
        "variable_value_added" => "variable value added",
        "variable_value_removed" => "variable value removed",
        "variable_value_changed" => "variable value changed",
        "variable_resolve_default_changed" => "variable resolve default changed",
        "variable_rule_added" => "variable resolve rule added",
        "variable_rule_removed" => "variable resolve rule removed",
        "variable_rule_when_changed" => "variable rule condition changed",
        "variable_rule_query_changed" => "variable rule catalog query changed",
        "variable_rule_value_changed" => "variable rule value changed",
        "qualifier_added" => "qualifier added",
        "qualifier_removed" => "qualifier removed",
        "qualifier_when_changed" => "qualifier condition changed",
        "catalog_added" => "catalog added",
        "catalog_removed" => "catalog removed",
        "catalog_schema_changed" => "catalog schema changed",
        "catalog_entry_added" => "catalog value added",
        "catalog_entry_removed" => "catalog value removed",
        "catalog_entry_changed" => "catalog value changed",
        _ => "semantic change",
    }
}

fn print_diff_value_line(
    label: &str,
    value: &Option<serde_json::Value>,
    side: DiffSide,
) -> Result<()> {
    let value = compact_json_option(value)?;
    if !style::enabled() {
        println!("    {label}: {value}");
        return Ok(());
    }

    println!(
        "    {} {}",
        style_diff_side_label(label, side),
        style_diff_side_value(&value, side)
    );
    Ok(())
}

fn print_resolution_impact_line(
    label: &str,
    resolution: &rototo::model::VariableResolution,
    side: DiffSide,
) -> Result<()> {
    let source = resolution_source_label(&resolution.source);
    let value = compact_json(&resolution.value)?;
    if !style::enabled() {
        println!("    {label}: {source} {value}");
        return Ok(());
    }

    println!(
        "    {} {} {}",
        style_diff_side_label(label, side),
        style::dim(&source),
        style_diff_side_value(&value, side)
    );
    Ok(())
}

fn style_diff_side_label(label: &str, side: DiffSide) -> String {
    let label = format!("{label}:");
    match side {
        DiffSide::Before => style::err(&label),
        DiffSide::After => style::ok(&label),
    }
}

fn style_diff_side_value(value: &str, side: DiffSide) -> String {
    if value == "<none>" {
        return style::dim(value);
    }
    match side {
        DiffSide::Before => style::err(value),
        DiffSide::After => style::ok(value),
    }
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

fn print_entity_separator(index: usize, count: usize) {
    if count > 1 && index > 0 {
        println!("{}", style::hairline());
    }
}

fn compact_json(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string(value).map_err(|err| RototoError::new(err.to_string()))
}

fn linter_registration_target(
    registration: &rototo::model::LinterRegistrationInspectReport,
) -> String {
    registration.target.clone()
}

fn print_dependencies(dependencies: &rototo::model::DependencyInspectReport, indent: &str) {
    if dependencies.qualifiers.is_empty()
        && dependencies.context_paths.is_empty()
        && dependencies.catalogs.is_empty()
    {
        return;
    }
    println!("{indent}{}", style::subhead("depends on"));
    for qualifier in &dependencies.qualifiers {
        println!(
            "{indent}  {} {}",
            style::dim("qualifier"),
            style::sea(qualifier)
        );
    }
    for context_path in &dependencies.context_paths {
        println!(
            "{indent}  {} {}",
            style::dim("context"),
            style::info(context_path)
        );
    }
    for catalog in &dependencies.catalogs {
        println!(
            "{indent}  {} {}",
            style::dim("catalog"),
            style::sea(catalog)
        );
    }
}

pub(crate) async fn print_qualifier_list(inspection: &PackageInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&QualifierListJson {
                package: inspection.root.display().to_string(),
                qualifiers: inspection
                    .qualifiers
                    .iter()
                    .map(|qualifier| {
                        package_file_json(&qualifier.id, &qualifier.uri, &qualifier.path)
                    })
                    .collect(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!(
        "{} {}",
        style::label("qualifiers"),
        style::bold(&inspection.qualifiers.len().to_string())
    );
    for qualifier in &inspection.qualifiers {
        match read_toml(&inspection.root.join(&qualifier.path)).await {
            Ok(value) => print_qualifier_summary(qualifier.id.as_str(), &qualifier.path, &value),
            Err(err) => print_unavailable_summary(
                qualifier.id.as_str(),
                "path",
                &qualifier.path,
                &err.to_string(),
            ),
        }
    }
    Ok(())
}

pub(crate) async fn print_variable_list(inspection: &PackageInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&VariableListJson {
                package: inspection.root.display().to_string(),
                variables: inspection
                    .variables
                    .iter()
                    .map(|variable| package_file_json(&variable.id, &variable.uri, &variable.path))
                    .collect(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!(
        "{} {}",
        style::label("variables"),
        style::bold(&inspection.variables.len().to_string())
    );
    for variable in &inspection.variables {
        match read_variable_toml(&inspection.root, variable).await {
            Ok(value) => print_variable_summary(variable.id.as_str(), &variable.path, &value)?,
            Err(err) => print_unavailable_summary(
                variable.id.as_str(),
                "path",
                &variable.path,
                &err.to_string(),
            ),
        }
    }
    Ok(())
}

pub(crate) async fn print_catalog_list(inspection: &PackageInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&CatalogListJson {
                package: inspection.root.display().to_string(),
                catalogs: inspection
                    .catalogs
                    .iter()
                    .map(|catalog| package_file_json(&catalog.id, &catalog.uri, &catalog.path))
                    .collect(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!(
        "{} {}",
        style::label("catalogs"),
        style::bold(&inspection.catalogs.len().to_string())
    );
    for catalog in &inspection.catalogs {
        match read_catalog_json(&inspection.root, catalog).await {
            Ok(value) => print_catalog_summary(catalog.id.as_str(), &catalog.path, &value),
            Err(err) => print_unavailable_summary(
                catalog.id.as_str(),
                "schema",
                &catalog.path,
                &err.to_string(),
            ),
        }
    }
    Ok(())
}

pub(crate) async fn print_qualifier_get(
    inspection: &PackageInspection,
    id: &str,
    json: bool,
) -> Result<()> {
    let qualifier = qualifier_for_id(inspection, id)?;
    let path = inspection.root.join(&qualifier.path);

    if json {
        let value = serde_json::to_value(read_toml(&path).await?)
            .map_err(|err| RototoError::new(err.to_string()))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&QualifierGetJson {
                package: inspection.root.display().to_string(),
                id: qualifier.id.clone(),
                uri: qualifier.uri.clone(),
                path: qualifier.path.display().to_string(),
                value,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    let value = read_toml(&path).await?;
    print_qualifier_detail(qualifier.id.as_str(), &qualifier.path, &value);
    print_source_header();
    print_package_file(&path).await
}

pub(crate) async fn print_variable_get(
    inspection: &PackageInspection,
    id: &str,
    json: bool,
) -> Result<()> {
    let variable = variable_for_id(inspection, id)?;

    if json {
        let value = serde_json::to_value(read_variable_toml(&inspection.root, variable).await?)
            .map_err(|err| RototoError::new(err.to_string()))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&VariableGetJson {
                package: inspection.root.display().to_string(),
                id: variable.id.clone(),
                uri: variable.uri.clone(),
                path: variable.path.display().to_string(),
                value,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    let value = read_variable_toml(&inspection.root, variable).await?;
    print_variable_detail(variable.id.as_str(), &variable.path, &value)?;
    print_source_header();
    print!(
        "{}",
        toml::to_string_pretty(&value).map_err(|err| RototoError::new(err.to_string()))?
    );
    Ok(())
}

pub(crate) async fn print_catalog_get(
    inspection: &PackageInspection,
    id: &str,
    json: bool,
) -> Result<()> {
    let catalog = catalog_for_id(inspection, id)?;

    if json {
        let value = read_catalog_json(&inspection.root, catalog).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&CatalogGetJson {
                package: inspection.root.display().to_string(),
                id: catalog.id.clone(),
                uri: catalog.uri.clone(),
                path: catalog.path.display().to_string(),
                value,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    let value = read_catalog_json(&inspection.root, catalog).await?;
    print_catalog_detail(catalog.id.as_str(), &catalog.path, &value);
    print_source_header();
    print!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|err| RototoError::new(err.to_string()))?
    );
    Ok(())
}

fn print_qualifier_summary(id: &str, path: &Path, value: &TomlValue) {
    println!("  {}", style::sea(id));
    println!(
        "    {} {}",
        style::dim("path:"),
        style::dim(&path.display().to_string())
    );
    if let Some(description) = toml_string(value, "description") {
        println!("    description: {description}");
    }
    if let Some(when) = toml_string(value, "when") {
        println!("    {} {}", style::subhead("when"), style::info(when));
    }
}

fn print_variable_summary(id: &str, path: &Path, value: &TomlValue) -> Result<()> {
    let type_label = toml_string(value, "type").unwrap_or("<missing>");
    println!("  {}", style::sea(id));
    println!("    {} {}", style::dim("type:"), style::info(type_label));
    if let Some(description) = toml_string(value, "description") {
        println!("    description: {description}");
    }
    println!(
        "    {} {}",
        style::dim("path:"),
        style::dim(&path.display().to_string())
    );
    print_variable_resolve_summary(value, "    ")?;
    Ok(())
}

fn print_catalog_summary(id: &str, path: &Path, value: &serde_json::Value) {
    println!("  {}", style::sea(id));
    println!(
        "    {} {}",
        style::dim("schema:"),
        style::dim(&path.display().to_string())
    );
    if let Some(description) = value.get("description").and_then(serde_json::Value::as_str) {
        println!("    description: {description}");
    }
    if let Some(schema_type) = value.get("type").and_then(serde_json::Value::as_str) {
        println!("    {} {}", style::dim("type:"), style::info(schema_type));
    }
    if let Some(entries) = value.get("entries").and_then(serde_json::Value::as_object) {
        println!(
            "    {} {}",
            style::dim("values:"),
            plural_count(entries.len(), "entry", "entries")
        );
    }
}

fn print_unavailable_summary(id: &str, path_label: &str, path: &Path, reason: &str) {
    println!("  {}", style::sea(id));
    println!(
        "    {} {}",
        style::dim(&format!("{path_label}:")),
        style::dim(&path.display().to_string())
    );
    println!("    status: {}", style::warn("unavailable"));
    println!("    reason: {reason}");
}

fn print_qualifier_detail(id: &str, path: &Path, value: &TomlValue) {
    println!("qualifier: {}", style::sea(id));
    println!(
        "  {} {}",
        style::dim("path:"),
        style::dim(&path.display().to_string())
    );
    if let Some(description) = toml_string(value, "description") {
        println!("  description: {description}");
    }
    if let Some(when) = toml_string(value, "when") {
        println!("  {} {}", style::subhead("when"), style::info(when));
    }
}

fn print_variable_detail(id: &str, path: &Path, value: &TomlValue) -> Result<()> {
    println!("variable: {}", style::sea(id));
    println!(
        "  {} {}",
        style::dim("path:"),
        style::dim(&path.display().to_string())
    );
    if let Some(description) = toml_string(value, "description") {
        println!("  description: {description}");
    }
    let type_label = toml_string(value, "type").unwrap_or("<missing>");
    println!("  {} {}", style::dim("type:"), style::info(type_label));
    print_variable_resolve_detail(value)?;
    Ok(())
}

fn print_catalog_detail(id: &str, path: &Path, value: &serde_json::Value) {
    println!("catalog: {}", style::sea(id));
    println!(
        "  {} {}",
        style::dim("schema:"),
        style::dim(&path.display().to_string())
    );
    if let Some(description) = value.get("description").and_then(serde_json::Value::as_str) {
        println!("  description: {description}");
    }
    if let Some(schema_type) = value.get("type").and_then(serde_json::Value::as_str) {
        println!("  {} {}", style::dim("type:"), style::info(schema_type));
    }
    if let Some(entries) = value.get("entries").and_then(serde_json::Value::as_object) {
        println!(
            "  {} {}",
            style::dim("values:"),
            plural_count(entries.len(), "entry", "entries")
        );
    }
}

fn print_variable_resolve_summary(value: &TomlValue, indent: &str) -> Result<()> {
    let Some(resolve) = value.get("resolve").and_then(TomlValue::as_table) else {
        return Ok(());
    };
    let default = resolve
        .get("default")
        .map(compact_toml_value)
        .transpose()?
        .unwrap_or_else(|| "<none>".to_owned());
    let rules = resolve
        .get("rule")
        .and_then(TomlValue::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    println!(
        "{indent}{} default {} / {}",
        style::dim("resolve:"),
        default,
        plural_count(rules, "rule", "rules")
    );
    Ok(())
}

fn print_variable_resolve_detail(value: &TomlValue) -> Result<()> {
    let Some(resolve) = value.get("resolve").and_then(TomlValue::as_table) else {
        return Ok(());
    };
    println!("  {}", style::subhead("resolve"));
    if let Some(rules) = resolve.get("rule").and_then(TomlValue::as_array) {
        for (index, rule) in rules.iter().enumerate() {
            let condition = rule
                .get("when")
                .and_then(TomlValue::as_str)
                .or_else(|| rule.get("query").and_then(TomlValue::as_str))
                .unwrap_or("<missing>");
            let rule_value = rule
                .get("value")
                .map(compact_toml_value)
                .transpose()?
                .unwrap_or_else(|| "<none>".to_owned());
            println!(
                "    {} if {} {} {}",
                style::dim(&format!("rule[{index}]")),
                style::sea(condition),
                style::arrow(),
                rule_value
            );
        }
    }
    let default = resolve
        .get("default")
        .map(compact_toml_value)
        .transpose()?
        .unwrap_or_else(|| "<none>".to_owned());
    println!("    {} {} {default}", style::dim("default"), style::arrow());
    Ok(())
}

fn print_source_header() {
    println!();
    println!("{}", style::subhead("source"));
}

fn toml_string<'a>(value: &'a TomlValue, key: &str) -> Option<&'a str> {
    value.get(key).and_then(TomlValue::as_str)
}

fn compact_toml_value(value: &TomlValue) -> Result<String> {
    let value = serde_json::to_value(value).map_err(|err| RototoError::new(err.to_string()))?;
    compact_json(&value)
}

fn plural_count(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

pub(crate) fn print_diagnostic_catalog_entry(
    diagnostic: &DiagnosticCatalogEntry,
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(diagnostic)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("{}", diagnostic.rule);
    if let Some(entity) = &diagnostic.entity {
        println!("  entity: {}", diagnostic_entity_label(entity));
    }
    println!("  severity: {}", severity_label(&diagnostic.severity));
    println!("  title: {}", diagnostic.title);
    println!("  help: {}", diagnostic.help);
    Ok(())
}

fn package_file_json<'a>(id: &'a str, uri: &'a str, path: &Path) -> PackageFileJson<'a> {
    PackageFileJson {
        id,
        uri,
        path: path.display().to_string(),
    }
}

async fn print_package_file(path: &Path) -> Result<()> {
    print!(
        "{}",
        tokio::fs::read_to_string(path)
            .await
            .map_err(|err| RototoError::new(format!("failed to read {}: {err}", path.display())))?
    );
    Ok(())
}

fn print_diagnostics(diagnostics: &[LintDiagnostic]) {
    for diagnostic in diagnostics {
        println!(
            "{}: {}: {}",
            style::severity_prefix(&diagnostic.severity, &diagnostic.rule.as_string()),
            style::info(&diagnostic_location_label(diagnostic)),
            diagnostic.message
        );
        println!("  {} {}", style::dim("help:"), style::dim(&diagnostic.help));
        for related in &diagnostic.related {
            println!(
                "  {} {}: {}",
                style::dim("note:"),
                style::info(&diagnostic_location_label_for_location(&related.location)),
                related.message
            );
        }
    }
}

fn diagnostic_location_label(diagnostic: &LintDiagnostic) -> String {
    diagnostic_location_label_for_location(&diagnostic.primary)
}

fn diagnostic_location_label_for_location(location: &DiagnosticLocation) -> String {
    let Some(range) = location.range else {
        return location.path.clone();
    };
    format!(
        "{}:{}:{}",
        location.path,
        range.start.line + 1,
        range.start.character + 1
    )
}

fn semantic_target_label(target: &SemanticTarget) -> String {
    match &target.field {
        Some(field) => format!(
            "{}.{}",
            semantic_entity_label(&target.entity),
            semantic_field_label(field)
        ),
        None => semantic_entity_label(&target.entity),
    }
}

fn semantic_entity_label(entity: &SemanticEntity) -> String {
    match entity {
        SemanticEntity::Package => "package".to_owned(),
        SemanticEntity::Manifest => "manifest".to_owned(),
        SemanticEntity::Qualifier { id } => format!("qualifier:{id}"),
        SemanticEntity::Predicate { qualifier, index } => {
            format!("qualifier:{qualifier}.predicate[{index}]")
        }
        SemanticEntity::Variable { id } => format!("variable:{id}"),
        SemanticEntity::EvaluationContext { id } => format!("evaluation-context:{id}"),
        SemanticEntity::EvaluationContextSample {
            evaluation_context,
            key,
        } => {
            format!("evaluation-context:{evaluation_context}.entry:{key}")
        }
        SemanticEntity::Catalog { id } => format!("catalog:{id}"),
        SemanticEntity::CatalogEntry { catalog, key } => format!("catalog:{catalog}.value:{key}"),
        SemanticEntity::Value { variable, key } => format!("variable:{variable}.value:{key}"),
        SemanticEntity::Rule { variable, index } => {
            format!("variable:{variable}.rule[{index}]")
        }
        SemanticEntity::CustomLint { path } => format!("lint:{path}"),
    }
}

fn semantic_field_label(field: &SemanticField) -> String {
    match field {
        SemanticField::PackageExtends => "extends".to_owned(),
        SemanticField::SchemaVersion => "schema_version".to_owned(),
        SemanticField::Description => "description".to_owned(),
        SemanticField::QualifierWhen => "when".to_owned(),
        SemanticField::QualifierPredicates => "predicates".to_owned(),
        SemanticField::PredicateAttribute => "attribute".to_owned(),
        SemanticField::PredicateOp => "op".to_owned(),
        SemanticField::PredicateNot => "not".to_owned(),
        SemanticField::PredicateValue => "value".to_owned(),
        SemanticField::PredicateSalt => "salt".to_owned(),
        SemanticField::PredicateRange => "range".to_owned(),
        SemanticField::VariableType => "type".to_owned(),
        SemanticField::VariableSchema => "schema".to_owned(),
        SemanticField::VariableValues => "values".to_owned(),
        SemanticField::VariableResolve => "resolve".to_owned(),
        SemanticField::VariableResolveDefault => "resolve.default".to_owned(),
        SemanticField::VariableRuleWhen => "when".to_owned(),
        SemanticField::VariableRuleQuery => "query".to_owned(),
        SemanticField::VariableRuleValue => "value".to_owned(),
        SemanticField::Value => "value".to_owned(),
        SemanticField::ValueJsonPath { path } => format!("value.{}", path.join(".")),
        SemanticField::SchemaJson => "json".to_owned(),
        SemanticField::SchemaJsonPath { path } => format!("json.{}", path.join(".")),
        SemanticField::EvaluationContextSample => "entry".to_owned(),
        SemanticField::CatalogEntry => "value".to_owned(),
    }
}

fn compact_json_option(value: &Option<serde_json::Value>) -> Result<String> {
    match value {
        Some(value) => compact_json(value),
        None => Ok("<none>".to_owned()),
    }
}

fn variable_rule_condition(rule: &rototo::model::RulePathwayInspectReport) -> &str {
    rule.when
        .as_deref()
        .or(rule.query.as_deref())
        .unwrap_or("<missing>")
}

fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn diagnostic_entity_label(entity: &DiagnosticEntity) -> &'static str {
    match entity {
        DiagnosticEntity::Package => "package",
        DiagnosticEntity::Qualifier => "qualifier",
        DiagnosticEntity::Variable => "variable",
        DiagnosticEntity::EvaluationContext => "evaluation_context",
        DiagnosticEntity::EvaluationContextSample => "evaluation_context_sample",
        DiagnosticEntity::Catalog => "catalog",
        DiagnosticEntity::CatalogEntry => "catalog_entry",
        DiagnosticEntity::Value => "value",
        DiagnosticEntity::Rule => "rule",
    }
}
