use serde::Serialize;

use rototo::diagnostics::{DiagnosticCatalogEntry, DiagnosticEntity, LintDiagnostic, Severity};
use rototo::error::{Result, RototoError};
use rototo::model::{QualifierInspection, VariableInspection, WorkspaceInspection, WorkspaceLint};
use rototo::workspace::{qualifier_for_id, read_toml, read_variable_toml, variable_for_id};

#[derive(Debug, Serialize)]
struct WorkspaceInspectionJson<'a> {
    workspace: String,
    environments: &'a [String],
    qualifiers: Vec<WorkspaceFileJson<'a>>,
    variables: Vec<WorkspaceFileJson<'a>>,
}

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

pub(crate) fn print_inspection(inspection: &WorkspaceInspection, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&inspection_json(inspection))
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("workspace: {}", inspection.root.display());
    println!("environments:");
    for environment in &inspection.environments {
        println!("  {}", environment);
    }
    println!("qualifiers:");
    for qualifier in &inspection.qualifiers {
        println!("  {}  {}", qualifier.uri, qualifier.path.display());
    }
    println!("variables:");
    for variable in &inspection.variables {
        println!("  {}  {}", variable.uri, variable.path.display());
    }
    Ok(())
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
    let path = inspection.root.join(&variable.path);

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

    print_workspace_file(&path).await
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
    println!("  entity: {}", diagnostic_entity_label(&diagnostic.entity));
    println!("  severity: {}", severity_label(&diagnostic.severity));
    println!("  title: {}", diagnostic.title);
    println!("  help: {}", diagnostic.help);
    Ok(())
}

fn inspection_json(inspection: &WorkspaceInspection) -> WorkspaceInspectionJson<'_> {
    WorkspaceInspectionJson {
        workspace: inspection.root.display().to_string(),
        environments: &inspection.environments,
        qualifiers: inspection.qualifiers.iter().map(qualifier_json).collect(),
        variables: inspection.variables.iter().map(variable_json).collect(),
    }
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
    }
}

fn diagnostic_location_label(diagnostic: &LintDiagnostic) -> String {
    let Some(range) = diagnostic.primary.range else {
        return diagnostic.primary.path.clone();
    };
    format!(
        "{}:{}:{}",
        diagnostic.primary.path,
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
        DiagnosticEntity::Value => "value",
        DiagnosticEntity::Rule => "rule",
        DiagnosticEntity::Schema => "schema",
    }
}
