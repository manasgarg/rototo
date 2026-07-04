use super::*;

use super::diff::resolution_source_label;
use super::lint::{compact_json_option, print_diagnostics, variable_rule_condition};

use crate::severity_label;

#[derive(Debug, Serialize)]
pub(super) struct PackageFileJson<'a> {
    id: &'a str,
    uri: &'a str,
    path: String,
}

#[derive(Debug, Serialize)]
pub(super) struct VariableListJson<'a> {
    package: String,
    variables: Vec<PackageFileJson<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct CatalogListJson<'a> {
    package: String,
    catalogs: Vec<PackageFileJson<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct VariableGetJson {
    package: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(super) struct CatalogGetJson {
    package: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
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

pub(super) fn print_compatible_evaluation_contexts(evaluation_contexts: &[String], indent: &str) {
    if evaluation_contexts.is_empty() {
        return;
    }
    println!("{indent}{}", style::subhead("evaluation contexts"));
    for evaluation_context in evaluation_contexts {
        println!("{indent}  {}", style::sea(evaluation_context));
    }
}

pub(super) fn print_variable_sample_coverage(
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

pub(super) fn print_context_attributes(
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

pub(super) fn linter_registration_target(
    registration: &rototo::model::LinterRegistrationInspectReport,
) -> String {
    registration.target.clone()
}

pub(super) fn print_dependencies(
    dependencies: &rototo::model::DependencyInspectReport,
    indent: &str,
) {
    if dependencies.variables.is_empty()
        && dependencies.context_paths.is_empty()
        && dependencies.catalogs.is_empty()
    {
        return;
    }
    println!("{indent}{}", style::subhead("depends on"));
    for variable in &dependencies.variables {
        println!(
            "{indent}  {} {}",
            style::dim("variable"),
            style::sea(variable)
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

pub(super) fn print_variable_summary(id: &str, path: &Path, value: &TomlValue) -> Result<()> {
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

pub(super) fn print_catalog_summary(id: &str, path: &Path, value: &serde_json::Value) {
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

pub(super) fn print_unavailable_summary(id: &str, path_label: &str, path: &Path, reason: &str) {
    println!("  {}", style::sea(id));
    println!(
        "    {} {}",
        style::dim(&format!("{path_label}:")),
        style::dim(&path.display().to_string())
    );
    println!("    status: {}", style::warn("unavailable"));
    println!("    reason: {reason}");
}

pub(super) fn print_variable_detail(id: &str, path: &Path, value: &TomlValue) -> Result<()> {
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

pub(super) fn print_catalog_detail(id: &str, path: &Path, value: &serde_json::Value) {
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

pub(super) fn print_variable_resolve_summary(value: &TomlValue, indent: &str) -> Result<()> {
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

pub(super) fn print_variable_resolve_detail(value: &TomlValue) -> Result<()> {
    let Some(resolve) = value.get("resolve").and_then(TomlValue::as_table) else {
        return Ok(());
    };
    println!("  {}", style::subhead("resolve"));
    if let Some(rules) = resolve.get("rule").and_then(TomlValue::as_array) {
        for (index, rule) in rules.iter().enumerate() {
            let condition = rule
                .get("when")
                .and_then(TomlValue::as_str)
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

pub(super) fn print_source_header() {
    println!();
    println!("{}", style::subhead("source"));
}

pub(super) fn toml_string<'a>(value: &'a TomlValue, key: &str) -> Option<&'a str> {
    value.get(key).and_then(TomlValue::as_str)
}

pub(super) fn compact_toml_value(value: &TomlValue) -> Result<String> {
    let value = serde_json::to_value(value).map_err(|err| RototoError::new(err.to_string()))?;
    compact_json(&value)
}

pub(super) fn package_file_json<'a>(id: &'a str, uri: &'a str, path: &Path) -> PackageFileJson<'a> {
    PackageFileJson {
        id,
        uri,
        path: path.display().to_string(),
    }
}
