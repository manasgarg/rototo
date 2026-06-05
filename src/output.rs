use serde::Serialize;

use rototo::diagnostics::{
    DiagnosticCatalogEntry, DiagnosticEntity, DiagnosticLocation, LintDiagnostic, Severity,
};
use rototo::error::{Result, RototoError};
use rototo::model::{InspectRuntimeStatus, WorkspaceInspectReport};
use rototo::model::{
    QualifierInspection, ResourceInspection, VariableInspection, WorkspaceInspection, WorkspaceLint,
};
use rototo::workspace::{
    qualifier_for_id, read_resource_toml, read_toml, read_variable_toml, resource_for_id,
    variable_for_id,
};

#[derive(Debug, Serialize)]
struct WorkspaceFileJson<'a> {
    id: &'a str,
    uri: &'a str,
    path: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceLintJson<'a> {
    workspace: String,
    documents: &'a [rototo::model::SourceDocumentSummary],
    diagnostics: &'a [LintDiagnostic],
}

#[derive(Debug, Serialize)]
struct QualifierListJson<'a> {
    workspace: String,
    qualifiers: Vec<WorkspaceFileJson<'a>>,
}

#[derive(Debug, Serialize)]
struct VariableListJson<'a> {
    workspace: String,
    variables: Vec<WorkspaceFileJson<'a>>,
}

#[derive(Debug, Serialize)]
struct ResourceListJson<'a> {
    workspace: String,
    resources: Vec<WorkspaceFileJson<'a>>,
}

#[derive(Debug, Serialize)]
struct QualifierGetJson {
    workspace: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct VariableGetJson {
    workspace: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ResourceGetJson {
    workspace: String,
    id: String,
    uri: String,
    path: String,
    value: serde_json::Value,
}

pub(crate) fn print_workspace_lint(lint: &WorkspaceLint, json: bool, quiet: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&WorkspaceLintJson {
                workspace: lint.root.display().to_string(),
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
        println!("ok: {}", lint.root.display());
        return Ok(());
    }

    print_diagnostics(&lint.diagnostics);
    Ok(())
}

pub(crate) fn print_inspect_report(report: &WorkspaceInspectReport, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("workspace: {}", report.workspace);
    match &report.runtime {
        InspectRuntimeStatus::Available => println!("runtime: available"),
        InspectRuntimeStatus::Unavailable { reason } => {
            println!("runtime: unavailable");
            println!("  reason: {reason}");
        }
    }

    if !report.diagnostics.is_empty() {
        println!("diagnostics:");
        print_diagnostics(&report.diagnostics);
    }

    if !report.schemas.is_empty() {
        println!("schemas:");
        let count = report.schemas.len();
        for (index, schema) in report.schemas.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  schema: {}", schema.id);
            println!("    path: {}", schema.path);
            println!("    status: {}", schema.status);
            if let Some(error) = &schema.error {
                println!("    error: {error}");
            }
            if !schema.consumers.is_empty() {
                println!("    consumed by:");
                for consumer in &schema.consumers {
                    println!("      {}  {}", consumer.label, consumer.location.path);
                }
            }
            if !schema.diagnostics.is_empty() {
                println!("    diagnostics:");
                print_diagnostics(&schema.diagnostics);
            }
        }
    }

    if !report.qualifiers.is_empty() {
        println!("qualifiers:");
        let count = report.qualifiers.len();
        for (index, qualifier) in report.qualifiers.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  qualifier: {}", qualifier.id);
            if !qualifier.predicates.is_empty() {
                println!("    predicates:");
                for predicate in &qualifier.predicates {
                    let attribute = predicate.attribute.as_deref().unwrap_or("<missing>");
                    let op = predicate.op.as_deref().unwrap_or("<missing>");
                    let predicate_value = predicate_value_label(predicate)?;
                    println!(
                        "      [{}] {} {}{}",
                        predicate.index, attribute, op, predicate_value
                    );
                }
            }
            print_dependencies(&qualifier.dependencies, "    ");
            if !qualifier.consumers.is_empty() {
                println!("    consumed by:");
                for consumer in &qualifier.consumers {
                    println!("      {}  {}", consumer.label, consumer.location.path);
                }
            }
            if !qualifier.diagnostics.is_empty() {
                println!("    diagnostics:");
                print_diagnostics(&qualifier.diagnostics);
            }
            if let Some(trace) = &qualifier.trace {
                println!("    trace: {}", trace.value);
                for predicate in &trace.predicates {
                    println!(
                        "      [{}] {} -> {}",
                        predicate.index, predicate.attribute, predicate.result
                    );
                }
            }
        }
    }

    if !report.resources.is_empty() {
        println!("resources:");
        let count = report.resources.len();
        for (index, resource) in report.resources.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  resource: {}", resource.id);
            println!("    path: {}", resource.path);
            if let Some(schema) = &resource.schema {
                println!("    schema: {schema}");
            }
            if !resource.objects.is_empty() {
                println!("    objects:");
                for object in &resource.objects {
                    println!("      {} = {}", object.key, compact_json(&object.value)?);
                }
            }
            print_dependencies(&resource.dependencies, "    ");
            if !resource.consumers.is_empty() {
                println!("    consumed by:");
                for consumer in &resource.consumers {
                    println!("      {}  {}", consumer.label, consumer.location.path);
                }
            }
            if !resource.diagnostics.is_empty() {
                println!("    diagnostics:");
                print_diagnostics(&resource.diagnostics);
            }
        }
    }

    if !report.variables.is_empty() {
        println!("variables:");
        let count = report.variables.len();
        for (index, variable) in report.variables.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  variable: {}", variable.id);
            println!("    type: {}", variable.type_source);
            if let Some(schema) = &variable.schema {
                println!("    schema: {schema}");
            }
            if !variable.values.is_empty() {
                println!("    values:");
                for value in &variable.values {
                    println!(
                        "      {} ({}) = {}",
                        value.key,
                        value.origin,
                        compact_json(&value.value)?
                    );
                }
            }
            if variable.resolve.default_value.is_some() || !variable.resolve.rules.is_empty() {
                println!("    resolve:");
                for rule in &variable.resolve.rules {
                    let qualifier = rule.qualifier.as_deref().unwrap_or("<missing>");
                    let value = rule.value.as_deref().unwrap_or("<missing>");
                    println!("      rule[{}] if {} -> {}", rule.index, qualifier, value);
                }
                let default = variable
                    .resolve
                    .default_value
                    .as_deref()
                    .unwrap_or("<missing>");
                println!("      default -> {default}");
            }
            print_dependencies(&variable.dependencies, "    ");
            if !variable.diagnostics.is_empty() {
                println!("    diagnostics:");
                print_diagnostics(&variable.diagnostics);
            }
            if let Some(trace) = &variable.trace {
                println!("    trace: {}", trace.resolution.value_key);
                for rule in &trace.rules {
                    println!(
                        "      rule[{}] if {} -> {} ({})",
                        rule.index,
                        rule.qualifier,
                        rule.value,
                        if rule.matched { "matched" } else { "skipped" }
                    );
                }
            }
        }
    }

    if !report.lint_rules.is_empty() {
        println!("lint rules:");
        let count = report.lint_rules.len();
        for (index, rule) in report.lint_rules.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  lint rule: {}", rule.rule);
            println!("    severity: {}", severity_label(&rule.severity));
            println!("    title: {}", rule.title);
            if !rule.diagnostics.is_empty() {
                print_diagnostics(&rule.diagnostics);
            }
        }
    }

    if !report.lint_authorities.is_empty() {
        println!("lint authorities:");
        let count = report.lint_authorities.len();
        for (index, authority) in report.lint_authorities.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  lint authority: {}", authority.authority);
            for rule in &authority.rules {
                println!("    {}  {}", rule.rule, rule.title);
            }
        }
    }

    if !report.linters.is_empty() {
        println!("linters:");
        let count = report.linters.len();
        for (index, linter) in report.linters.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  linter: {}", linter.id);
            println!("    path: {}", linter.path);
            if !linter.registrations.is_empty() {
                println!("    registrations:");
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

fn print_entity_separator(index: usize, count: usize) {
    if count > 1 && index > 0 {
        println!("  ----------------------------------------");
    }
}

fn compact_json(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string(value).map_err(|err| RototoError::new(err.to_string()))
}

fn predicate_value_label(predicate: &rototo::model::PredicateInspectReport) -> Result<String> {
    if let Some(value) = &predicate.value {
        return Ok(format!(" {}", compact_json(value)?));
    }
    match (&predicate.salt, &predicate.range) {
        (Some(salt), Some(range)) => Ok(format!(
            " salt={} range={}",
            salt,
            compact_json(&serde_json::json!(range))?
        )),
        (Some(salt), None) => Ok(format!(" salt={salt}")),
        (None, Some(range)) => Ok(format!(
            " range={}",
            compact_json(&serde_json::json!(range))?
        )),
        (None, None) => Ok(String::new()),
    }
}

fn linter_registration_target(
    registration: &rototo::model::LinterRegistrationInspectReport,
) -> String {
    let Some(field) = registration.field.as_deref() else {
        return registration.entity.clone();
    };
    if field.starts_with("value.") || field.starts_with("json.") {
        field.to_owned()
    } else {
        format!("{}.{}", registration.entity, field)
    }
}

fn print_dependencies(dependencies: &rototo::model::DependencyInspectReport, indent: &str) {
    if dependencies.qualifiers.is_empty()
        && dependencies.context_paths.is_empty()
        && dependencies.schemas.is_empty()
        && dependencies.resources.is_empty()
    {
        return;
    }
    println!("{indent}depends on:");
    for qualifier in &dependencies.qualifiers {
        println!("{indent}  qualifier {qualifier}");
    }
    for context_path in &dependencies.context_paths {
        println!("{indent}  context {context_path}");
    }
    for schema in &dependencies.schemas {
        println!("{indent}  schema {schema}");
    }
    for resource in &dependencies.resources {
        println!("{indent}  resource {resource}");
    }
}

pub(crate) fn print_qualifier_list(inspection: &WorkspaceInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&QualifierListJson {
                workspace: inspection.root.display().to_string(),
                qualifiers: inspection.qualifiers.iter().map(qualifier_json).collect(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    for qualifier in &inspection.qualifiers {
        println!("{}", qualifier.id);
    }
    Ok(())
}

pub(crate) fn print_variable_list(inspection: &WorkspaceInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&VariableListJson {
                workspace: inspection.root.display().to_string(),
                variables: inspection.variables.iter().map(variable_json).collect(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    for variable in &inspection.variables {
        println!("{}", variable.id);
    }
    Ok(())
}

pub(crate) fn print_resource_list(inspection: &WorkspaceInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ResourceListJson {
                workspace: inspection.root.display().to_string(),
                resources: inspection.resources.iter().map(resource_json).collect(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    for resource in &inspection.resources {
        println!("{}", resource.id);
    }
    Ok(())
}

pub(crate) async fn print_qualifier_get(
    inspection: &WorkspaceInspection,
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
                workspace: inspection.root.display().to_string(),
                id: qualifier.id.clone(),
                uri: qualifier.uri.clone(),
                path: qualifier.path.display().to_string(),
                value,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    print_workspace_file(&path).await
}

pub(crate) async fn print_variable_get(
    inspection: &WorkspaceInspection,
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
                workspace: inspection.root.display().to_string(),
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
    print!(
        "{}",
        toml::to_string_pretty(&value).map_err(|err| RototoError::new(err.to_string()))?
    );
    Ok(())
}

pub(crate) async fn print_resource_get(
    inspection: &WorkspaceInspection,
    id: &str,
    json: bool,
) -> Result<()> {
    let resource = resource_for_id(inspection, id)?;

    if json {
        let value = serde_json::to_value(read_resource_toml(&inspection.root, resource).await?)
            .map_err(|err| RototoError::new(err.to_string()))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&ResourceGetJson {
                workspace: inspection.root.display().to_string(),
                id: resource.id.clone(),
                uri: resource.uri.clone(),
                path: resource.path.display().to_string(),
                value,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    let value = read_resource_toml(&inspection.root, resource).await?;
    print!(
        "{}",
        toml::to_string_pretty(&value).map_err(|err| RototoError::new(err.to_string()))?
    );
    Ok(())
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

fn qualifier_json(qualifier: &QualifierInspection) -> WorkspaceFileJson<'_> {
    WorkspaceFileJson {
        id: &qualifier.id,
        uri: &qualifier.uri,
        path: qualifier.path.display().to_string(),
    }
}

fn variable_json(variable: &VariableInspection) -> WorkspaceFileJson<'_> {
    WorkspaceFileJson {
        id: &variable.id,
        uri: &variable.uri,
        path: variable.path.display().to_string(),
    }
}

fn resource_json(resource: &ResourceInspection) -> WorkspaceFileJson<'_> {
    WorkspaceFileJson {
        id: &resource.id,
        uri: &resource.uri,
        path: resource.path.display().to_string(),
    }
}

async fn print_workspace_file(path: &std::path::Path) -> Result<()> {
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
            "{}[{}]: {}: {}",
            severity_label(&diagnostic.severity),
            diagnostic.rule.as_string(),
            diagnostic_location_label(diagnostic),
            diagnostic.message
        );
        println!("  help: {}", diagnostic.help);
        for related in &diagnostic.related {
            println!(
                "  note: {}: {}",
                diagnostic_location_label_for_location(&related.location),
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

fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn diagnostic_entity_label(entity: &DiagnosticEntity) -> &'static str {
    match entity {
        DiagnosticEntity::Workspace => "workspace",
        DiagnosticEntity::Qualifier => "qualifier",
        DiagnosticEntity::Variable => "variable",
        DiagnosticEntity::Resource => "resource",
        DiagnosticEntity::ResourceObject => "resource_object",
        DiagnosticEntity::Value => "value",
        DiagnosticEntity::Rule => "rule",
        DiagnosticEntity::Schema => "schema",
    }
}
