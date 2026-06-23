use std::path::Path;

use serde::Serialize;

use crate::style;

use rototo::diagnostics::{
    DiagnosticCatalogEntry, DiagnosticEntity, DiagnosticLocation, LintDiagnostic, SemanticEntity,
    SemanticField, SemanticTarget, Severity,
};
use rototo::error::{Result, RototoError};
use rototo::model::{InspectRuntimeStatus, WorkspaceDiff, WorkspaceInspectReport};
use rototo::model::{WorkspaceInspection, WorkspaceLint};
use rototo::workspace::{
    catalog_for_id, qualifier_for_id, read_catalog_json, read_toml, read_variable_toml,
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
struct CatalogListJson<'a> {
    workspace: String,
    catalogs: Vec<WorkspaceFileJson<'a>>,
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
struct CatalogGetJson {
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
        println!("{}", style::ok_line(&lint.root.display().to_string()));
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

    println!(
        "{} {}",
        style::label("workspace"),
        style::bold(&report.workspace)
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

    if !report.request_contexts.is_empty() {
        println!("{}", style::label("request contexts"));
        let count = report.request_contexts.len();
        for (index, request_context) in report.request_contexts.iter().enumerate() {
            print_entity_separator(index, count);
            println!("  request context: {}", style::sea(&request_context.id));
            println!(
                "    {} {}",
                style::dim("path:"),
                style::dim(&request_context.path)
            );
            println!(
                "    {} {}",
                style::dim("status:"),
                if request_context.status == "valid" {
                    style::ok(&request_context.status)
                } else {
                    style::err(&request_context.status)
                }
            );
            if let Some(title) = &request_context.title {
                println!("    title: {title}");
            }
            if let Some(description) = &request_context.description {
                println!("    description: {description}");
            }
            if !request_context.entries.is_empty() {
                println!("    {}", style::subhead("samples"));
                for entry in &request_context.entries {
                    println!(
                        "      {} = {}",
                        style::sea(&entry.key),
                        compact_json(&entry.value)?
                    );
                }
            }
            if !request_context.diagnostics.is_empty() {
                println!("    {}", style::subhead("diagnostics"));
                print_diagnostics(&request_context.diagnostics);
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
            print_compatible_request_contexts(&qualifier.request_contexts, "    ");
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
            print_compatible_request_contexts(&variable.request_contexts, "    ");
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

fn print_compatible_request_contexts(request_contexts: &[String], indent: &str) {
    if request_contexts.is_empty() {
        return;
    }
    println!("{indent}{}", style::subhead("request contexts"));
    for request_context in request_contexts {
        println!("{indent}  {}", style::sea(request_context));
    }
}

pub(crate) fn print_workspace_diff(diff: &WorkspaceDiff, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(diff).map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("before: {}", diff.before);
    println!("after: {}", diff.after);
    if diff.changes.is_empty() {
        println!("semantic changes: none");
    } else {
        println!("semantic changes:");
        for change in &diff.changes {
            println!(
                "  {}  {}",
                change.kind,
                semantic_target_label(&change.target)
            );
            if let Some(location) = &change.before_location {
                println!(
                    "    before location: {}",
                    diagnostic_location_label_for_location(location)
                );
            }
            if let Some(location) = &change.after_location {
                println!(
                    "    after location: {}",
                    diagnostic_location_label_for_location(location)
                );
            }
            if change.before.is_some() || change.after.is_some() {
                println!("    before: {}", compact_json_option(&change.before)?);
                println!("    after: {}", compact_json_option(&change.after)?);
            }
        }
    }
    if !diff.resolution_impacts.is_empty() {
        println!("resolution impact:");
        for impact in &diff.resolution_impacts {
            println!("  variable: {}", impact.variable);
            println!(
                "    before: {} {}",
                resolution_source_label(&impact.before.source),
                compact_json(&impact.before.value)?
            );
            println!(
                "    after: {} {}",
                resolution_source_label(&impact.after.source),
                compact_json(&impact.after.value)?
            );
        }
    }
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

pub(crate) fn print_qualifier_list(inspection: &WorkspaceInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&QualifierListJson {
                workspace: inspection.root.display().to_string(),
                qualifiers: inspection
                    .qualifiers
                    .iter()
                    .map(|qualifier| {
                        workspace_file_json(&qualifier.id, &qualifier.uri, &qualifier.path)
                    })
                    .collect(),
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
                variables: inspection
                    .variables
                    .iter()
                    .map(|variable| workspace_file_json(
                        &variable.id,
                        &variable.uri,
                        &variable.path
                    ))
                    .collect(),
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

pub(crate) fn print_catalog_list(inspection: &WorkspaceInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&CatalogListJson {
                workspace: inspection.root.display().to_string(),
                catalogs: inspection
                    .catalogs
                    .iter()
                    .map(|catalog| workspace_file_json(&catalog.id, &catalog.uri, &catalog.path))
                    .collect(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    for catalog in &inspection.catalogs {
        println!("{}", catalog.id);
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

pub(crate) async fn print_catalog_get(
    inspection: &WorkspaceInspection,
    id: &str,
    json: bool,
) -> Result<()> {
    let catalog = catalog_for_id(inspection, id)?;

    if json {
        let value = read_catalog_json(&inspection.root, catalog).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&CatalogGetJson {
                workspace: inspection.root.display().to_string(),
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
    print!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|err| RototoError::new(err.to_string()))?
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

fn workspace_file_json<'a>(id: &'a str, uri: &'a str, path: &Path) -> WorkspaceFileJson<'a> {
    WorkspaceFileJson {
        id,
        uri,
        path: path.display().to_string(),
    }
}

async fn print_workspace_file(path: &Path) -> Result<()> {
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
        SemanticEntity::Workspace => "workspace".to_owned(),
        SemanticEntity::Manifest => "manifest".to_owned(),
        SemanticEntity::Qualifier { id } => format!("qualifier:{id}"),
        SemanticEntity::Predicate { qualifier, index } => {
            format!("qualifier:{qualifier}.predicate[{index}]")
        }
        SemanticEntity::Variable { id } => format!("variable:{id}"),
        SemanticEntity::RequestContext { id } => format!("request-context:{id}"),
        SemanticEntity::RequestContextEntry {
            request_context,
            key,
        } => {
            format!("request-context:{request_context}.entry:{key}")
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
        SemanticField::WorkspaceExtends => "extends".to_owned(),
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
        SemanticField::RequestContextEntry => "entry".to_owned(),
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
        DiagnosticEntity::Workspace => "workspace",
        DiagnosticEntity::Qualifier => "qualifier",
        DiagnosticEntity::Variable => "variable",
        DiagnosticEntity::RequestContext => "request_context",
        DiagnosticEntity::RequestContextEntry => "request_context_entry",
        DiagnosticEntity::Catalog => "catalog",
        DiagnosticEntity::CatalogEntry => "catalog_entry",
        DiagnosticEntity::Value => "value",
        DiagnosticEntity::Rule => "rule",
    }
}
