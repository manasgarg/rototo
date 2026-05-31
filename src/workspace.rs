use std::fmt;
use std::path::{Path, PathBuf};

use toml::Value;

use crate::error::{Result, RototoError};
use crate::model::{
    QualifierConfig, QualifierInspection, VariableConfig, VariableInspection, WorkspaceInspection,
};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

pub async fn inspect_workspace(workspace_root: &Path) -> Result<WorkspaceInspection> {
    let workspace_root = tokio::fs::canonicalize(workspace_root)
        .await
        .map_err(|err| RototoError::new(format!("workspace not found: {err}")))?;
    let manifest = read_toml(&workspace_root.join(WORKSPACE_MANIFEST)).await?;
    let environments = workspace_environments(&manifest)?;
    let qualifiers = discover_qualifiers(&workspace_root).await?;
    let variables = discover_variables(&workspace_root).await?;

    Ok(WorkspaceInspection {
        root: workspace_root,
        environments,
        qualifiers,
        variables,
    })
}

pub async fn find_workspace_root(start: &Path) -> Result<PathBuf> {
    let mut current = tokio::fs::canonicalize(start)
        .await
        .map_err(|err| RototoError::new(format!("failed to resolve current directory: {err}")))?;

    loop {
        if tokio::fs::metadata(current.join(WORKSPACE_MANIFEST))
            .await
            .is_ok_and(|metadata| metadata.is_file())
        {
            return Ok(current);
        }

        if !current.pop() {
            return Err(RototoError::new(
                "workspace not found: pass a workspace source or run inside a rototo workspace",
            ));
        }
    }
}

pub async fn read_toml(path: &Path) -> Result<Value> {
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|err| RototoError::new(format!("failed to read {}: {err}", path.display())))?;
    text.parse::<Value>()
        .map_err(|err| RototoError::new(format!("failed to parse {}: {err}", path.display())))
}

pub async fn read_variable_toml(
    workspace_root: &Path,
    variable: &VariableInspection,
) -> Result<Value> {
    read_variable_toml_detailed(workspace_root, variable)
        .await
        .map_err(|err| RototoError::new(err.to_string()))
}

pub(crate) async fn read_variable_toml_detailed(
    workspace_root: &Path,
    variable: &VariableInspection,
) -> std::result::Result<Value, VariableTomlReadError> {
    let path = workspace_root.join(&variable.path);
    let mut toml = read_toml_detailed(&path).await?;
    merge_external_variable_values(workspace_root, variable, &mut toml).await?;
    Ok(toml)
}

#[derive(Debug)]
pub(crate) struct VariableTomlReadError {
    message: String,
}

impl VariableTomlReadError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VariableTomlReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for VariableTomlReadError {}

async fn read_toml_detailed(path: &Path) -> std::result::Result<Value, VariableTomlReadError> {
    let text = tokio::fs::read_to_string(path).await.map_err(|err| {
        VariableTomlReadError::new(format!("failed to read {}: {err}", path.display()))
    })?;
    text.parse::<Value>().map_err(|err| {
        VariableTomlReadError::new(format!("failed to parse {}: {err}", path.display()))
    })
}

pub fn qualifier_for_id<'a>(
    inspection: &'a WorkspaceInspection,
    id: &str,
) -> Result<&'a QualifierInspection> {
    inspection
        .qualifiers
        .iter()
        .find(|qualifier| qualifier.id == id)
        .ok_or_else(|| RototoError::new(format!("qualifier not found: qualifier://{id}")))
}

pub fn variable_for_id<'a>(
    inspection: &'a WorkspaceInspection,
    id: &str,
) -> Result<&'a VariableInspection> {
    inspection
        .variables
        .iter()
        .find(|variable| variable.id == id)
        .ok_or_else(|| RototoError::new(format!("variable not found: variable://{id}")))
}

pub async fn list_qualifiers(workspace_root: &Path) -> Result<Vec<QualifierInspection>> {
    Ok(inspect_workspace(workspace_root).await?.qualifiers)
}

pub async fn list_variables(workspace_root: &Path) -> Result<Vec<VariableInspection>> {
    Ok(inspect_workspace(workspace_root).await?.variables)
}

pub async fn read_qualifier(workspace_root: &Path, id: &str) -> Result<QualifierConfig> {
    let inspection = inspect_workspace(workspace_root).await?;
    let qualifier = qualifier_for_id(&inspection, id)?;
    qualifier_config(&inspection.root, qualifier).await
}

pub async fn read_variable(workspace_root: &Path, id: &str) -> Result<VariableConfig> {
    let inspection = inspect_workspace(workspace_root).await?;
    let variable = variable_for_id(&inspection, id)?;
    variable_config(&inspection.root, variable).await
}

pub async fn read_qualifiers(workspace_root: &Path) -> Result<Vec<QualifierConfig>> {
    let inspection = inspect_workspace(workspace_root).await?;
    let mut configs = Vec::new();
    for qualifier in &inspection.qualifiers {
        configs.push(qualifier_config(&inspection.root, qualifier).await?);
    }
    Ok(configs)
}

pub async fn read_variables(workspace_root: &Path) -> Result<Vec<VariableConfig>> {
    let inspection = inspect_workspace(workspace_root).await?;
    let mut configs = Vec::new();
    for variable in &inspection.variables {
        configs.push(variable_config(&inspection.root, variable).await?);
    }
    Ok(configs)
}

pub fn workspace_environments(manifest: &Value) -> Result<Vec<String>> {
    let schema_version = manifest
        .get("schema_version")
        .and_then(Value::as_integer)
        .ok_or_else(|| RototoError::new("workspace manifest must declare schema_version = 1"))?;
    if schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(RototoError::new(format!(
            "unsupported workspace schema_version: {schema_version}"
        )));
    }

    let values = manifest
        .get("environments")
        .and_then(|environments| environments.get("values"))
        .and_then(Value::as_array)
        .ok_or_else(|| RototoError::new("workspace manifest must declare [environments].values"))?;

    let mut environments = Vec::new();
    for value in values {
        let environment = value
            .as_str()
            .ok_or_else(|| RototoError::new("environment names must be strings"))?;
        if environment == "_" {
            return Err(RototoError::new(
                "_ is reserved as the catch-all environment",
            ));
        }
        if environments.iter().any(|existing| existing == environment) {
            return Err(RototoError::new(format!(
                "duplicate environment: {environment}"
            )));
        }
        environments.push(environment.to_owned());
    }

    if environments.is_empty() {
        return Err(RototoError::new(
            "workspace must declare at least one environment",
        ));
    }
    Ok(environments)
}

async fn qualifier_config(
    workspace_root: &Path,
    qualifier: &QualifierInspection,
) -> Result<QualifierConfig> {
    let value = serde_json::to_value(read_toml(&workspace_root.join(&qualifier.path)).await?)
        .map_err(|err| RototoError::new(err.to_string()))?;

    Ok(QualifierConfig {
        id: qualifier.id.clone(),
        uri: qualifier.uri.clone(),
        path: qualifier.path.clone(),
        value,
    })
}

async fn variable_config(
    workspace_root: &Path,
    variable: &VariableInspection,
) -> Result<VariableConfig> {
    let value = serde_json::to_value(read_variable_toml(workspace_root, variable).await?)
        .map_err(|err| RototoError::new(err.to_string()))?;

    Ok(VariableConfig {
        id: variable.id.clone(),
        uri: variable.uri.clone(),
        path: variable.path.clone(),
        value,
    })
}

async fn discover_qualifiers(workspace_root: &Path) -> Result<Vec<QualifierInspection>> {
    let mut qualifiers = Vec::new();
    for path in discover_named_toml_files(workspace_root, "qualifiers").await? {
        let id = id_from_path(&path)?;
        let relative_path = relative_path(workspace_root, &path)?;
        qualifiers.push(QualifierInspection {
            uri: format!("qualifier://{id}"),
            id,
            path: relative_path,
        });
    }
    qualifiers.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(qualifiers)
}

async fn discover_variables(workspace_root: &Path) -> Result<Vec<VariableInspection>> {
    let mut variables = Vec::new();
    for path in discover_named_toml_files(workspace_root, "variables").await? {
        let id = id_from_path(&path)?;
        let relative_path = relative_path(workspace_root, &path)?;
        variables.push(VariableInspection {
            uri: format!("variable://{id}"),
            id,
            path: relative_path,
        });
    }
    variables.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(variables)
}

async fn discover_named_toml_files(workspace_root: &Path, dir: &str) -> Result<Vec<PathBuf>> {
    let dir = workspace_root.join(dir);
    let mut paths = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return Ok(paths);
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to read {}: {err}", dir.display())))?
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("toml")
            && tokio::fs::metadata(&path)
                .await
                .is_ok_and(|metadata| metadata.is_file())
        {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn id_from_path(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .ok_or_else(|| RototoError::new(format!("path has no valid id: {path:?}")))
}

fn relative_path(workspace_root: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(workspace_root)
        .map(Path::to_path_buf)
        .map_err(|err| RototoError::new(err.to_string()))
}

async fn merge_external_variable_values(
    workspace_root: &Path,
    variable: &VariableInspection,
    toml: &mut Value,
) -> std::result::Result<(), VariableTomlReadError> {
    let values_dir = external_values_dir(workspace_root, variable);
    if !tokio::fs::metadata(&values_dir)
        .await
        .is_ok_and(|metadata| metadata.is_dir())
    {
        return Ok(());
    }

    let mut external_values = toml::map::Map::new();
    let mut entries = tokio::fs::read_dir(&values_dir).await.map_err(|err| {
        VariableTomlReadError::new(format!(
            "failed to read variable values directory {}: {err}",
            values_dir.display()
        ))
    })?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| VariableTomlReadError::new(err.to_string()))?
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml")
            || !tokio::fs::metadata(&path)
                .await
                .is_ok_and(|metadata| metadata.is_file())
        {
            continue;
        }
        let id = id_from_path(&path).map_err(|err| VariableTomlReadError::new(err.to_string()))?;
        let value = value_from_external_value_toml(&path).await?;
        if external_values.insert(id.clone(), value).is_some() {
            return Err(VariableTomlReadError::new(format!(
                "duplicate external variable value: {id}"
            )));
        }
    }

    if external_values.is_empty() {
        return Ok(());
    }

    let Some(root_table) = toml.as_table_mut() else {
        return Ok(());
    };
    let values = root_table
        .entry("values".to_owned())
        .or_insert_with(|| Value::Table(toml::map::Map::new()));
    let Some(values) = values.as_table_mut() else {
        return Err(VariableTomlReadError::new(format!(
            "variable values must be a table: {}",
            variable.path.display()
        )));
    };

    for (id, value) in external_values {
        if values.insert(id.clone(), value).is_some() {
            return Err(VariableTomlReadError::new(format!(
                "variable value is declared more than once: {id}"
            )));
        }
    }
    Ok(())
}

fn external_values_dir(workspace_root: &Path, variable: &VariableInspection) -> PathBuf {
    workspace_root
        .join(&variable.path)
        .parent()
        .unwrap_or(workspace_root)
        .join(format!("{}-values", variable.id))
}

async fn value_from_external_value_toml(
    path: &Path,
) -> std::result::Result<Value, VariableTomlReadError> {
    let text = tokio::fs::read_to_string(path).await.map_err(|err| {
        VariableTomlReadError::new(format!("failed to read {}: {err}", path.display()))
    })?;
    let value = text.parse::<Value>().map_err(|err| {
        VariableTomlReadError::new(format!("failed to parse {}: {err}", path.display()))
    })?;
    let Some(table) = value.as_table() else {
        return Ok(value);
    };
    if table.len() == 1 && table.contains_key("value") {
        return Ok(table.get("value").expect("value key checked").clone());
    }
    Ok(value)
}
