use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use toml::Value;

use crate::error::{Result, RototoError};
use crate::model::{
    CatalogConfig, CatalogInspection, EvaluationContextInspection, LinterInspection,
    PackageInspection, VariableConfig, VariableInspection,
};

const PACKAGE_MANIFEST: &str = "rototo-package.toml";
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

pub async fn inspect_package(package_root: &Path) -> Result<PackageInspection> {
    let package_root = tokio::fs::canonicalize(package_root)
        .await
        .map_err(|err| RototoError::new(format!("package not found: {err}")))?;
    let manifest = read_toml(&package_root.join(PACKAGE_MANIFEST)).await?;
    validate_package_manifest(&manifest)?;
    let evaluation_contexts = discover_evaluation_contexts(&package_root).await?;
    let catalogs = discover_catalogs(&package_root).await?;
    let variables = discover_variables(&package_root).await?;
    let linters = discover_linters(&package_root).await?;

    Ok(PackageInspection {
        root: package_root,
        evaluation_contexts,
        catalogs,
        variables,
        linters,
    })
}

pub async fn find_package_root(start: &Path) -> Result<PathBuf> {
    let mut current = tokio::fs::canonicalize(start)
        .await
        .map_err(|err| RototoError::new(format!("failed to resolve current directory: {err}")))?;

    loop {
        if tokio::fs::metadata(current.join(PACKAGE_MANIFEST))
            .await
            .is_ok_and(|metadata| metadata.is_file())
        {
            return Ok(current);
        }

        if !current.pop() {
            return Err(RototoError::new(
                "package not found: pass a package source or run inside a rototo package",
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

pub async fn read_json(path: &Path) -> Result<JsonValue> {
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|err| RototoError::new(format!("failed to read {}: {err}", path.display())))?;
    serde_json::from_str::<JsonValue>(&text)
        .map_err(|err| RototoError::new(format!("failed to parse {}: {err}", path.display())))
}

pub async fn read_variable_toml(
    package_root: &Path,
    variable: &VariableInspection,
) -> Result<Value> {
    read_toml(&package_root.join(&variable.path)).await
}

pub fn variable_for_id<'a>(
    inspection: &'a PackageInspection,
    id: &str,
) -> Result<&'a VariableInspection> {
    inspection
        .variables
        .iter()
        .find(|variable| variable.id == id)
        .ok_or_else(|| RototoError::new(format!("variable not found: variable://{id}")))
}

pub fn catalog_for_id<'a>(
    inspection: &'a PackageInspection,
    id: &str,
) -> Result<&'a CatalogInspection> {
    inspection
        .catalogs
        .iter()
        .find(|catalog| catalog.id == id)
        .ok_or_else(|| RototoError::new(format!("catalog not found: catalog://{id}")))
}

pub async fn list_variables(package_root: &Path) -> Result<Vec<VariableInspection>> {
    Ok(inspect_package(package_root).await?.variables)
}

pub async fn list_catalogs(package_root: &Path) -> Result<Vec<CatalogInspection>> {
    Ok(inspect_package(package_root).await?.catalogs)
}

pub async fn read_variable(package_root: &Path, id: &str) -> Result<VariableConfig> {
    let inspection = inspect_package(package_root).await?;
    let variable = variable_for_id(&inspection, id)?;
    variable_config(&inspection.root, variable).await
}

pub async fn read_catalog(package_root: &Path, id: &str) -> Result<CatalogConfig> {
    let inspection = inspect_package(package_root).await?;
    let catalog = catalog_for_id(&inspection, id)?;
    catalog_config(&inspection.root, catalog).await
}

pub async fn read_variables(package_root: &Path) -> Result<Vec<VariableConfig>> {
    let inspection = inspect_package(package_root).await?;
    let mut configs = Vec::new();
    for variable in &inspection.variables {
        configs.push(variable_config(&inspection.root, variable).await?);
    }
    Ok(configs)
}

pub async fn read_catalogs(package_root: &Path) -> Result<Vec<CatalogConfig>> {
    let inspection = inspect_package(package_root).await?;
    let mut configs = Vec::new();
    for catalog in &inspection.catalogs {
        configs.push(catalog_config(&inspection.root, catalog).await?);
    }
    Ok(configs)
}

pub fn validate_package_manifest(manifest: &Value) -> Result<()> {
    let schema_version = manifest
        .get("schema_version")
        .and_then(Value::as_integer)
        .ok_or_else(|| RototoError::new("package manifest must declare schema_version = 1"))?;
    if schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(RototoError::new(format!(
            "unsupported package schema_version: {schema_version}"
        )));
    }

    package_extends_sources(manifest)?;
    Ok(())
}

pub fn package_extends_sources(manifest: &Value) -> Result<Vec<String>> {
    let Some(extends) = manifest.get("extends") else {
        return Ok(Vec::new());
    };
    let values = extends
        .as_array()
        .ok_or_else(|| RototoError::new("package extends must be an array of sources"))?;
    let mut sources = Vec::with_capacity(values.len());
    for source in values {
        let Some(source) = source.as_str() else {
            return Err(RototoError::new("package extends sources must be strings"));
        };
        if source.trim().is_empty() {
            return Err(RototoError::new("package extends source must not be blank"));
        }
        if source.trim() != source {
            return Err(RototoError::new(
                "package extends source must not contain surrounding whitespace",
            ));
        }
        sources.push(source.to_owned());
    }
    Ok(sources)
}

async fn variable_config(
    package_root: &Path,
    variable: &VariableInspection,
) -> Result<VariableConfig> {
    let value = serde_json::to_value(read_variable_toml(package_root, variable).await?)
        .map_err(|err| RototoError::new(err.to_string()))?;

    Ok(VariableConfig {
        id: variable.id.clone(),
        uri: variable.uri.clone(),
        path: variable.path.clone(),
        value,
    })
}

async fn catalog_config(package_root: &Path, catalog: &CatalogInspection) -> Result<CatalogConfig> {
    let value = read_catalog_json(package_root, catalog).await?;

    Ok(CatalogConfig {
        id: catalog.id.clone(),
        uri: catalog.uri.clone(),
        path: catalog.path.clone(),
        value,
    })
}

pub async fn read_catalog_json(
    package_root: &Path,
    catalog: &CatalogInspection,
) -> Result<JsonValue> {
    let mut json = read_json(&package_root.join(&catalog.path)).await?;
    let entries = read_catalog_entries_toml(package_root, catalog).await?;
    if entries.is_empty() {
        return Ok(json);
    }
    let Some(root_object) = json.as_object_mut() else {
        return Ok(json);
    };
    root_object.insert("entries".to_owned(), JsonValue::Object(entries));
    Ok(json)
}

async fn read_catalog_entries_toml(
    package_root: &Path,
    catalog: &CatalogInspection,
) -> Result<serde_json::Map<String, JsonValue>> {
    let entries_dir = package_root.join("data/catalogs").join(&catalog.id);
    let mut catalog_entries = serde_json::Map::new();
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
        catalog_entries.insert(
            id,
            serde_json::to_value(read_toml(&path).await?)
                .map_err(|err| RototoError::new(err.to_string()))?,
        );
    }
    Ok(catalog_entries)
}

async fn discover_variables(package_root: &Path) -> Result<Vec<VariableInspection>> {
    let mut variables = Vec::new();
    for path in discover_named_toml_files(package_root, "variables").await? {
        let Some(id) = namespaced_id_from_path(package_root, "variables", &path, ".toml") else {
            continue;
        };
        let relative_path = relative_path(package_root, &path)?;
        variables.push(VariableInspection {
            uri: format!("variable://{id}"),
            id,
            path: relative_path,
        });
    }
    variables.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(variables)
}

async fn discover_catalogs(package_root: &Path) -> Result<Vec<CatalogInspection>> {
    let mut catalogs = Vec::new();
    for path in discover_named_files(package_root, "model/catalogs", "json").await? {
        let Some(id) =
            namespaced_id_from_path(package_root, "model/catalogs", &path, ".schema.json")
        else {
            continue;
        };
        let relative_path = relative_path(package_root, &path)?;
        catalogs.push(CatalogInspection {
            uri: format!("catalog://{id}"),
            id,
            path: relative_path,
        });
    }
    catalogs.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(catalogs)
}

async fn discover_evaluation_contexts(
    package_root: &Path,
) -> Result<Vec<EvaluationContextInspection>> {
    let mut evaluation_contexts = Vec::new();
    for path in discover_named_files(package_root, "model/context", "json").await? {
        if path
            .strip_prefix(package_root.join("model/context"))
            .ok()
            .and_then(|relative| relative.parent())
            .is_some_and(|parent| {
                parent
                    .iter()
                    .filter_map(|component| component.to_str())
                    .any(|component| component.ends_with("-samples"))
            })
        {
            continue;
        }
        let Some(id) =
            namespaced_id_from_path(package_root, "model/context", &path, ".schema.json")
        else {
            continue;
        };
        let relative_path = relative_path(package_root, &path)?;
        evaluation_contexts.push(EvaluationContextInspection {
            uri: format!("evaluation-context://{id}"),
            id,
            path: relative_path,
        });
    }
    evaluation_contexts.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(evaluation_contexts)
}

async fn discover_linters(package_root: &Path) -> Result<Vec<LinterInspection>> {
    let mut linters = Vec::new();
    let dir = package_root.join("lint");
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
            let relative_path = relative_path(package_root, &path)?;
            linters.push(LinterInspection {
                id,
                path: relative_path,
            });
        }
    }
    linters.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(linters)
}

async fn discover_named_toml_files(package_root: &Path, dir: &str) -> Result<Vec<PathBuf>> {
    discover_named_files(package_root, dir, "toml").await
}

async fn discover_named_files(
    package_root: &Path,
    dir: &str,
    extension: &str,
) -> Result<Vec<PathBuf>> {
    // Directories namespace ids for every collection, so discovery walks
    // subdirectories too.
    let dir = package_root.join(dir);
    let mut paths = Vec::new();
    let mut pending = vec![dir.clone()];
    while let Some(directory) = pending.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(&directory).await else {
            continue;
        };
        while let Some(entry) = entries.next_entry().await.map_err(|err| {
            RototoError::new(format!("failed to read {}: {err}", directory.display()))
        })? {
            let path = entry.path();
            let Ok(metadata) = tokio::fs::metadata(&path).await else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some(extension)
            {
                paths.push(path);
            }
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

/// The namespaced id a file names below its collection root: the relative
/// path with separators normalized to `/` and the suffix stripped.
fn namespaced_id_from_path(
    package_root: &Path,
    dir: &str,
    path: &Path,
    suffix: &str,
) -> Option<String> {
    let relative = path.strip_prefix(package_root.join(dir)).ok()?;
    let relative = relative.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");
    let id = relative.strip_suffix(suffix)?;
    (!id.is_empty() && !id.ends_with('/')).then(|| id.to_owned())
}

fn relative_path(package_root: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(package_root)
        .map(Path::to_path_buf)
        .map_err(|err| RototoError::new(err.to_string()))
}
