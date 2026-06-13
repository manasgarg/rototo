use std::path::{Path, PathBuf};

use toml::Value;

use crate::error::{Result, RototoError};
use crate::model::{
    CatalogConfig, CatalogInspection, LinterInspection, QualifierConfig, QualifierInspection,
    SchemaInspection, VariableConfig, VariableInspection, WorkspaceInspection,
};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

pub async fn inspect_workspace(workspace_root: &Path) -> Result<WorkspaceInspection> {
    let workspace_root = tokio::fs::canonicalize(workspace_root)
        .await
        .map_err(|err| RototoError::new(format!("workspace not found: {err}")))?;
    let manifest = read_toml(&workspace_root.join(WORKSPACE_MANIFEST)).await?;
    validate_workspace_manifest(&manifest)?;
    let schemas = discover_schemas(&workspace_root).await?;
    let catalogs = discover_catalogs(&workspace_root).await?;
    let qualifiers = discover_qualifiers(&workspace_root).await?;
    let variables = discover_variables(&workspace_root).await?;
    let linters = discover_linters(&workspace_root).await?;

    Ok(WorkspaceInspection {
        root: workspace_root,
        schemas,
        catalogs,
        qualifiers,
        variables,
        linters,
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
    read_toml(&workspace_root.join(&variable.path)).await
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

pub fn catalog_for_id<'a>(
    inspection: &'a WorkspaceInspection,
    id: &str,
) -> Result<&'a CatalogInspection> {
    inspection
        .catalogs
        .iter()
        .find(|catalog| catalog.id == id)
        .ok_or_else(|| RototoError::new(format!("catalog not found: catalog://{id}")))
}

pub async fn list_qualifiers(workspace_root: &Path) -> Result<Vec<QualifierInspection>> {
    Ok(inspect_workspace(workspace_root).await?.qualifiers)
}

pub async fn list_variables(workspace_root: &Path) -> Result<Vec<VariableInspection>> {
    Ok(inspect_workspace(workspace_root).await?.variables)
}

pub async fn list_catalogs(workspace_root: &Path) -> Result<Vec<CatalogInspection>> {
    Ok(inspect_workspace(workspace_root).await?.catalogs)
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

pub async fn read_catalog(workspace_root: &Path, id: &str) -> Result<CatalogConfig> {
    let inspection = inspect_workspace(workspace_root).await?;
    let catalog = catalog_for_id(&inspection, id)?;
    catalog_config(&inspection.root, catalog).await
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

pub async fn read_catalogs(workspace_root: &Path) -> Result<Vec<CatalogConfig>> {
    let inspection = inspect_workspace(workspace_root).await?;
    let mut configs = Vec::new();
    for catalog in &inspection.catalogs {
        configs.push(catalog_config(&inspection.root, catalog).await?);
    }
    Ok(configs)
}

pub fn validate_workspace_manifest(manifest: &Value) -> Result<()> {
    let schema_version = manifest
        .get("schema_version")
        .and_then(Value::as_integer)
        .ok_or_else(|| RototoError::new("workspace manifest must declare schema_version = 1"))?;
    if schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(RototoError::new(format!(
            "unsupported workspace schema_version: {schema_version}"
        )));
    }

    workspace_extends_sources(manifest)?;
    Ok(())
}

pub fn workspace_extends_sources(manifest: &Value) -> Result<Vec<String>> {
    let Some(extends) = manifest.get("extends") else {
        return Ok(Vec::new());
    };
    let values = extends
        .as_array()
        .ok_or_else(|| RototoError::new("workspace extends must be an array of sources"))?;
    let mut sources = Vec::with_capacity(values.len());
    for source in values {
        let Some(source) = source.as_str() else {
            return Err(RototoError::new(
                "workspace extends sources must be strings",
            ));
        };
        if source.trim().is_empty() {
            return Err(RototoError::new(
                "workspace extends source must not be blank",
            ));
        }
        if source.trim() != source {
            return Err(RototoError::new(
                "workspace extends source must not contain surrounding whitespace",
            ));
        }
        sources.push(source.to_owned());
    }
    Ok(sources)
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

async fn catalog_config(
    workspace_root: &Path,
    catalog: &CatalogInspection,
) -> Result<CatalogConfig> {
    let value = serde_json::to_value(read_catalog_toml(workspace_root, catalog).await?)
        .map_err(|err| RototoError::new(err.to_string()))?;

    Ok(CatalogConfig {
        id: catalog.id.clone(),
        uri: catalog.uri.clone(),
        path: catalog.path.clone(),
        value,
    })
}

pub async fn read_catalog_toml(
    workspace_root: &Path,
    catalog: &CatalogInspection,
) -> Result<Value> {
    let mut toml = read_toml(&workspace_root.join(&catalog.path)).await?;
    let entries = read_catalog_entries_toml(workspace_root, catalog).await?;
    if entries.is_empty() {
        return Ok(toml);
    }
    let Some(root_table) = toml.as_table_mut() else {
        return Ok(toml);
    };
    root_table.insert("entries".to_owned(), Value::Table(entries));
    Ok(toml)
}

async fn read_catalog_entries_toml(
    workspace_root: &Path,
    catalog: &CatalogInspection,
) -> Result<toml::map::Map<String, Value>> {
    let entries_dir = workspace_root
        .join("catalogs")
        .join(format!("{}-entries", catalog.id));
    let mut catalog_entries = toml::map::Map::new();
    let Ok(mut entries) = tokio::fs::read_dir(&entries_dir).await else {
        return Ok(catalog_entries);
    };
    while let Some(entry) = entries.next_entry().await.map_err(|err| {
        RototoError::new(format!("failed to read {}: {err}", entries_dir.display()))
    })? {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml")
            || !tokio::fs::metadata(&path)
                .await
                .is_ok_and(|metadata| metadata.is_file())
        {
            continue;
        }
        let id = id_from_path(&path)?;
        catalog_entries.insert(id, read_toml(&path).await?);
    }
    Ok(catalog_entries)
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

async fn discover_catalogs(workspace_root: &Path) -> Result<Vec<CatalogInspection>> {
    let mut catalogs = Vec::new();
    for path in discover_named_toml_files(workspace_root, "catalogs").await? {
        let id = id_from_path(&path)?;
        let relative_path = relative_path(workspace_root, &path)?;
        catalogs.push(CatalogInspection {
            uri: format!("catalog://{id}"),
            id,
            path: relative_path,
        });
    }
    catalogs.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(catalogs)
}

async fn discover_schemas(workspace_root: &Path) -> Result<Vec<SchemaInspection>> {
    let mut schemas = Vec::new();
    for path in discover_named_files(workspace_root, "schemas", "json").await? {
        let id = id_from_path(&path)?;
        let relative_path = relative_path(workspace_root, &path)?;
        schemas.push(SchemaInspection {
            id,
            path: relative_path,
        });
    }
    schemas.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(schemas)
}

async fn discover_linters(workspace_root: &Path) -> Result<Vec<LinterInspection>> {
    let mut linters = Vec::new();
    let dir = workspace_root.join("lint");
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return Ok(linters);
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to read {}: {err}", dir.display())))?
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("lua")
            && tokio::fs::metadata(&path)
                .await
                .is_ok_and(|metadata| metadata.is_file())
        {
            let id = id_from_path(&path)?;
            let relative_path = relative_path(workspace_root, &path)?;
            linters.push(LinterInspection {
                id,
                path: relative_path,
            });
        }
    }
    linters.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(linters)
}

async fn discover_named_toml_files(workspace_root: &Path, dir: &str) -> Result<Vec<PathBuf>> {
    discover_named_files(workspace_root, dir, "toml").await
}

async fn discover_named_files(
    workspace_root: &Path,
    dir: &str,
    extension: &str,
) -> Result<Vec<PathBuf>> {
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
        if path.extension().and_then(|value| value.to_str()) == Some(extension)
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
