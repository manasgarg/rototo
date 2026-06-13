use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};
use crate::lint::WorkspaceSemanticModel;

use super::github::workspace_repo_path;
use super::store::WorkspaceRecord;

/* The inventory derives from rototo's semantic model — the console does not
parse workspace files itself. Only context examples are enumerated from the
contexts/ directory, which is file listing, not parsing. */

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInventory {
    pub variables: Vec<VariableInventoryItem>,
    pub qualifiers: Vec<QualifierInventoryItem>,
    pub catalogs: Vec<CatalogInventoryItem>,
    pub catalog_entries: Vec<CatalogEntryInventoryItem>,
    pub schemas: Vec<SchemaInventoryItem>,
    pub linters: Vec<LinterInventoryItem>,
    pub context: ContextInventory,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableInventoryItem {
    pub id: String,
    pub path: String,
    pub description: Option<String>,
    pub declaration: String,
    pub default_value_key: Option<String>,
    pub rule_count: usize,
    pub qualifier_references: Vec<String>,
    /// Distinct value keys selected by rules; for catalog-typed variables
    /// these name catalog entries.
    pub rule_value_keys: Vec<String>,
    pub catalog_reference: Option<String>,
    pub schema_reference: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualifierInventoryItem {
    pub id: String,
    pub path: String,
    pub description: Option<String>,
    pub predicate_count: usize,
    pub qualifier_references: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInventoryItem {
    pub id: String,
    pub path: String,
    pub description: Option<String>,
    pub schema: Option<String>,
    pub schema_reference: Option<String>,
    pub entry_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntryInventoryItem {
    pub catalog_id: String,
    pub key: String,
    pub id: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInventoryItem {
    pub id: String,
    pub path: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinterInventoryItem {
    pub id: String,
    pub title: Option<String>,
    pub path: Option<String>,
    pub kind: &'static str,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInventory {
    pub schema_path: Option<String>,
    pub example_count: usize,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDefinition {
    pub path: String,
    pub text: String,
    pub language: &'static str,
}

pub async fn inspect_workspace_inventory(
    workspace: &WorkspaceRecord,
    model: &WorkspaceSemanticModel,
    staged_root: &Path,
) -> Result<WorkspaceInventory> {
    let context = inspect_context(workspace, staged_root).await?;
    Ok(inventory_from_model(workspace, model, context))
}

pub async fn read_workspace_definition(
    workspace: &WorkspaceRecord,
    staged_root: &Path,
    path: &str,
) -> Result<WorkspaceDefinition> {
    let local_path = workspace_local_path(workspace, path)?;
    let text = tokio::fs::read_to_string(staged_root.join(&local_path))
        .await
        .map_err(|err| RototoError::new(format!("failed to read {path}: {err}")))?;
    Ok(WorkspaceDefinition {
        path: path.to_owned(),
        text,
        language: language_for_path(path),
    })
}

fn inventory_from_model(
    workspace: &WorkspaceRecord,
    model: &WorkspaceSemanticModel,
    context: ContextInventory,
) -> WorkspaceInventory {
    let repo_path = |path: &str| workspace_repo_path(&workspace.path, path);

    let variables = model
        .variables
        .iter()
        .map(|variable| {
            let rules: &[crate::lint::RuleModel] = variable
                .resolve
                .as_ref()
                .map(|resolve| resolve.rules.as_slice())
                .unwrap_or_default();
            VariableInventoryItem {
                id: variable.id.clone(),
                path: repo_path(&variable.location.path),
                description: variable.description.clone(),
                declaration: declaration_label(&variable.declaration),
                default_value_key: variable
                    .resolve
                    .as_ref()
                    .and_then(|resolve| resolve.default.as_ref())
                    .and_then(|default| default.value.clone()),
                rule_count: rules.len(),
                qualifier_references: distinct_sorted(
                    rules
                        .iter()
                        .filter_map(|rule| rule.qualifier.as_ref())
                        .filter_map(|qualifier| qualifier.value.clone()),
                ),
                rule_value_keys: distinct_sorted(
                    rules
                        .iter()
                        .filter_map(|rule| rule.value.as_ref())
                        .filter_map(|value| value.value.clone()),
                ),
                catalog_reference: (variable.declaration.kind == "catalog")
                    .then(|| variable.declaration.value.clone())
                    .flatten(),
                schema_reference: (variable.declaration.kind == "schema")
                    .then(|| {
                        variable
                            .declaration
                            .value
                            .as_ref()
                            .and_then(|value| value.rsplit('/').next())
                            .map(str::to_owned)
                    })
                    .flatten(),
            }
        })
        .collect();

    let mut qualifier_edges: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for reference in &model.references {
        if let (
            crate::lint::ModelEntityRef::Qualifier { id: from },
            crate::lint::ModelEntityRef::Qualifier { id: to },
        ) = (&reference.from, &reference.to)
        {
            qualifier_edges.entry(from).or_default().push(to.clone());
        }
    }
    let qualifiers = model
        .qualifiers
        .iter()
        .map(|qualifier| QualifierInventoryItem {
            id: qualifier.id.clone(),
            path: repo_path(&qualifier.location.path),
            description: qualifier.description.clone(),
            predicate_count: qualifier.predicates.len(),
            qualifier_references: distinct_sorted(
                qualifier_edges
                    .get(qualifier.id.as_str())
                    .cloned()
                    .unwrap_or_default()
                    .into_iter(),
            ),
        })
        .collect();

    let mut entry_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &model.catalog_entries {
        *entry_counts.entry(entry.catalog.as_str()).or_default() += 1;
    }
    let catalogs = model
        .catalogs
        .iter()
        .map(|catalog| {
            let schema = catalog
                .schema
                .as_ref()
                .and_then(|schema| schema.value.clone());
            CatalogInventoryItem {
                id: catalog.id.clone(),
                path: repo_path(&catalog.location.path),
                description: catalog.description.clone(),
                schema_reference: schema
                    .as_ref()
                    .and_then(|value| value.rsplit('/').next())
                    .map(str::to_owned),
                schema,
                entry_count: entry_counts
                    .get(catalog.id.as_str())
                    .copied()
                    .unwrap_or_default(),
            }
        })
        .collect();

    let catalog_entries = model
        .catalog_entries
        .iter()
        .map(|entry| CatalogEntryInventoryItem {
            catalog_id: entry.catalog.clone(),
            key: entry.key.clone(),
            id: format!("{}/{}", entry.catalog, entry.key),
            path: repo_path(&entry.location.path),
        })
        .collect();

    let schemas = model
        .schemas
        .iter()
        .filter(|schema| !schema.path.ends_with("context.schema.json"))
        .map(|schema| {
            let json = schema.json.as_ref();
            let title = json
                .and_then(|json| json.get("title"))
                .and_then(JsonValue::as_str)
                .or_else(|| {
                    json.and_then(|json| json.get("$id"))
                        .and_then(JsonValue::as_str)
                })
                .map(str::to_owned);
            SchemaInventoryItem {
                id: schema
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(schema.path.as_str())
                    .to_owned(),
                path: repo_path(&schema.path),
                title,
            }
        })
        .collect();

    let linters = model
        .linters
        .iter()
        .map(|linter| {
            let file_name = linter.path.rsplit('/').next().unwrap_or(&linter.path);
            let titles: BTreeSet<&str> = linter
                .rules
                .iter()
                .map(|rule| rule.title.as_str())
                .collect();
            LinterInventoryItem {
                id: file_name
                    .rsplit_once('.')
                    .map(|(stem, _)| stem.to_owned())
                    .unwrap_or_else(|| file_name.to_owned()),
                title: (!titles.is_empty())
                    .then(|| titles.into_iter().collect::<Vec<_>>().join(" · ")),
                path: Some(repo_path(&linter.path)),
                kind: "script",
            }
        })
        .collect();

    WorkspaceInventory {
        variables,
        qualifiers,
        catalogs,
        catalog_entries,
        schemas,
        linters,
        context,
    }
}

fn declaration_label(declaration: &crate::lint::DeclarationModel) -> String {
    match declaration.kind.as_str() {
        "primitive" => declaration
            .value
            .clone()
            .unwrap_or_else(|| "undeclared".to_owned()),
        "catalog" => format!("catalog:{}", declaration.value.as_deref().unwrap_or("?")),
        "schema" => format!("schema:{}", declaration.value.as_deref().unwrap_or("?")),
        "missing" => "undeclared".to_owned(),
        other => other.to_owned(),
    }
}

async fn inspect_context(
    workspace: &WorkspaceRecord,
    staged_root: &Path,
) -> Result<ContextInventory> {
    let schema_entries = read_dir_file_names(&staged_root.join("schemas")).await?;
    let has_context_schema = schema_entries
        .iter()
        .any(|name| name == "context.schema.json");
    let mut examples: Vec<String> = read_dir_file_names(&staged_root.join("contexts"))
        .await?
        .into_iter()
        .filter(|name| name.ends_with(".json"))
        .map(|name| workspace_repo_path(&workspace.path, &format!("contexts/{name}")))
        .collect();
    examples.sort();
    Ok(ContextInventory {
        schema_path: has_context_schema
            .then(|| workspace_repo_path(&workspace.path, "schemas/context.schema.json")),
        example_count: examples.len(),
        examples,
    })
}

async fn read_dir_file_names(path: &Path) -> Result<Vec<String>> {
    let mut entries = match tokio::fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(RototoError::new(format!(
                "failed to list {}: {err}",
                path.display()
            )));
        }
    };
    let mut names = Vec::new();
    loop {
        let entry = entries
            .next_entry()
            .await
            .map_err(|err| RototoError::new(format!("failed to list {}: {err}", path.display())))?;
        let Some(entry) = entry else {
            break;
        };
        let is_file = entry
            .file_type()
            .await
            .map(|file_type| file_type.is_file())
            .unwrap_or(false);
        if is_file && let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

/// Maps a repo path to a staged-checkout-relative path, rejecting anything
/// that escapes the workspace.
pub fn workspace_local_path(workspace: &WorkspaceRecord, path: &str) -> Result<String> {
    if path.starts_with('/') || path.split('/').any(|segment| segment == "..") {
        return Err(RototoError::new(
            "workspace definition path must stay inside the workspace",
        ));
    }
    if workspace.path == "." {
        return Ok(path.to_owned());
    }
    let prefix = format!("{}/", workspace.path);
    path.strip_prefix(&prefix)
        .map(str::to_owned)
        .ok_or_else(|| {
            RototoError::new("workspace definition path does not belong to this workspace")
        })
}

pub fn language_for_path(path: &str) -> &'static str {
    if path.ends_with(".toml") {
        "toml"
    } else if path.ends_with(".json") {
        "json"
    } else if path.ends_with(".lua") {
        "lua"
    } else {
        "text"
    }
}

fn distinct_sorted(values: impl Iterator<Item = String>) -> Vec<String> {
    let set: BTreeSet<String> = values.collect();
    set.into_iter().collect()
}
