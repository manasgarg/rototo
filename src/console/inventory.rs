use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::error::{Result, RototoError};
use crate::lint::{ModelEntityRef, ModelReferenceVia, PackageSemanticModel};

use super::github::package_repo_path;
use super::store::PackageRecord;

/* The inventory derives from rototo's semantic model — the console does not
parse package files itself. */

/// Browser inventory for one staged package.
///
/// This is rebuilt from `PackageSemanticModel` plus a lightweight context
/// directory scan whenever a package or branch screen loads. It is not stored;
/// the source files and the staged semantic model own its lifecycle.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInventory {
    pub variables: Vec<VariableInventoryItem>,
    pub qualifiers: Vec<QualifierInventoryItem>,
    pub catalogs: Vec<CatalogInventoryItem>,
    pub catalog_entries: Vec<CatalogEntryInventoryItem>,
    pub linters: Vec<LinterInventoryItem>,
    pub context: ContextInventory,
}

/// Variable row in the console inventory.
///
/// It gives the UI enough resolved metadata to list, filter, and draw
/// references without reparsing TOML. Each item is derived from one semantic
/// variable model and disappears when that model no longer exists.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableInventoryItem {
    pub id: String,
    pub path: String,
    pub description: Option<String>,
    pub declaration: String,
    pub default_value: Option<String>,
    pub rule_count: usize,
    pub qualifier_references: Vec<String>,
    /// Distinct string values selected by rules. For catalog-typed variables
    /// these name catalog values; primitive literals are not inventory links.
    pub rule_values: Vec<String>,
    pub catalog_reference: Option<String>,
}

/// Qualifier row in the console inventory.
///
/// It exists so screens can show named runtime conditions and their references
/// while leaving predicate semantics to the Rust model. The item is regenerated
/// for each staged checkout.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualifierInventoryItem {
    pub id: String,
    pub path: String,
    pub description: Option<String>,
    pub predicate_count: usize,
    pub qualifier_references: Vec<String>,
}

/// Catalog row in the console inventory.
///
/// The console uses this projection to connect catalog declarations, schema
/// references, and entry counts. It is derived from the semantic model and
/// never cached separately from the staged package view.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInventoryItem {
    pub id: String,
    pub path: String,
    pub description: Option<String>,
    pub schema: Option<String>,
    pub entry_count: usize,
}

/// Catalog value row in the console inventory.
///
/// This binds the catalog id and value name to the source path the editor can
/// open. It is rebuilt from catalog value models for each staged checkout.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntryInventoryItem {
    pub catalog_id: String,
    pub key: String,
    pub id: String,
    pub path: String,
}

/// Custom linter row in the console inventory.
///
/// It lets the UI navigate Lua lint scripts and show the rules declared by
/// each script. The item is derived from linter models and source paths, not a
/// persisted console table.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinterInventoryItem {
    pub id: String,
    pub title: Option<String>,
    pub path: Option<String>,
    pub kind: &'static str,
}

/// Request context schemas and sample entries discovered for one package.
///
/// The semantic model owns `request-contexts/<id>.schema.json` and
/// `request-contexts/<id>-entries/*.json`. The projection is rebuilt with the
/// inventory and used for preview inputs.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInventory {
    pub request_contexts: Vec<RequestContextInventoryItem>,
    pub entries: Vec<RequestContextEntryInventoryItem>,
    pub example_count: usize,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContextInventoryItem {
    pub id: String,
    pub path: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub entry_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContextEntryInventoryItem {
    pub request_context_id: String,
    pub key: String,
    pub id: String,
    pub path: String,
}

/// Source text loaded for one package definition file.
///
/// The editor receives this per request after the route validates that the path
/// belongs to the staged package. It is discarded once the response is sent.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDefinition {
    pub path: String,
    pub text: String,
    pub language: &'static str,
}

pub async fn inspect_package_inventory(
    package: &PackageRecord,
    model: &PackageSemanticModel,
    _staged_root: &Path,
) -> Result<PackageInventory> {
    let context = inspect_context(package, model);
    Ok(inventory_from_model(package, model, context))
}

pub async fn read_package_definition(
    package: &PackageRecord,
    staged_root: &Path,
    path: &str,
) -> Result<PackageDefinition> {
    let local_path = package_local_path(package, path)?;
    let text = tokio::fs::read_to_string(staged_root.join(&local_path))
        .await
        .map_err(|err| RototoError::new(format!("failed to read {path}: {err}")))?;
    Ok(PackageDefinition {
        path: path.to_owned(),
        text,
        language: language_for_path(path),
    })
}

fn inventory_from_model(
    package: &PackageRecord,
    model: &PackageSemanticModel,
    context: ContextInventory,
) -> PackageInventory {
    let repo_path = |path: &str| package_repo_path(&package.path, path);

    let variables = model
        .variables
        .iter()
        .map(|variable| {
            let rules: &[crate::lint::RuleModel] = variable
                .resolve
                .as_ref()
                .map(|resolve| resolve.rules.as_slice())
                .unwrap_or_default();
            let qualifier_references =
                distinct_sorted(model.references.iter().filter_map(|reference| {
                    match (&reference.from, &reference.to, &reference.via) {
                        (
                            ModelEntityRef::Variable { id: variable_id },
                            ModelEntityRef::Qualifier { id: qualifier_id },
                            ModelReferenceVia::RuleCondition { .. },
                        ) if variable_id == &variable.id => Some(qualifier_id.clone()),
                        _ => None,
                    }
                }));
            VariableInventoryItem {
                id: variable.id.clone(),
                path: repo_path(&variable.location.path),
                description: variable.description.clone(),
                declaration: declaration_label(&variable.declaration),
                default_value: (variable.declaration.kind == "catalog")
                    .then(|| {
                        variable
                            .resolve
                            .as_ref()
                            .and_then(|resolve| resolve.default.as_ref())
                            .and_then(|default| default.value.as_ref())
                            .and_then(|value| value.as_str())
                            .map(str::to_owned)
                    })
                    .flatten(),
                rule_count: rules.len(),
                qualifier_references,
                rule_values: if variable.declaration.kind == "catalog" {
                    distinct_sorted(
                        rules
                            .iter()
                            .filter_map(|rule| rule.value.as_ref())
                            .filter_map(|value| value.value.as_ref())
                            .filter_map(|value| value.as_str())
                            .map(str::to_owned),
                    )
                } else {
                    Vec::new()
                },
                catalog_reference: (variable.declaration.kind == "catalog")
                    .then(|| variable.declaration.value.clone())
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
        .map(|catalog| CatalogInventoryItem {
            id: catalog.id.clone(),
            path: repo_path(&catalog.location.path),
            description: catalog.description.clone(),
            schema: Some(catalog.path.clone()),
            entry_count: entry_counts
                .get(catalog.id.as_str())
                .copied()
                .unwrap_or_default(),
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

    PackageInventory {
        variables,
        qualifiers,
        catalogs,
        catalog_entries,
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

fn inspect_context(package: &PackageRecord, model: &PackageSemanticModel) -> ContextInventory {
    let repo_path = |path: &str| package_repo_path(&package.path, path);
    let mut entry_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &model.request_context_entries {
        *entry_counts
            .entry(entry.request_context.as_str())
            .or_default() += 1;
    }
    let request_contexts = model
        .request_contexts
        .iter()
        .map(|context| RequestContextInventoryItem {
            id: context.id.clone(),
            path: repo_path(&context.path),
            title: context.title.clone(),
            description: context.description.clone(),
            entry_count: entry_counts
                .get(context.id.as_str())
                .copied()
                .unwrap_or_default(),
        })
        .collect();
    let entries = model
        .request_context_entries
        .iter()
        .map(|entry| RequestContextEntryInventoryItem {
            request_context_id: entry.request_context.clone(),
            key: entry.key.clone(),
            id: format!("{}/{}", entry.request_context, entry.key),
            path: repo_path(&entry.path),
        })
        .collect::<Vec<_>>();
    let mut examples = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    examples.sort();
    ContextInventory {
        request_contexts,
        example_count: entries.len(),
        examples,
        entries,
    }
}

/// Maps a repo path to a staged-checkout-relative path, rejecting anything
/// that escapes the package.
pub fn package_local_path(package: &PackageRecord, path: &str) -> Result<String> {
    if path.starts_with('/') || path.split('/').any(|segment| segment == "..") {
        return Err(RototoError::new(
            "package definition path must stay inside the package",
        ));
    }
    if package.path == "." {
        return Ok(path.to_owned());
    }
    let prefix = format!("{}/", package.path);
    path.strip_prefix(&prefix)
        .map(str::to_owned)
        .ok_or_else(|| RototoError::new("package definition path does not belong to this package"))
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
