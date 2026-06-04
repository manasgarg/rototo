//! Workspace layering.
//!
//! A workspace can declare `extends = "<source>"` in its `rototo-workspace.toml`
//! manifest to layer itself on top of a parent workspace. Layering composes a
//! single effective workspace by overlaying the more-derived workspace's files
//! on top of the parent's: a child either adds new file-level entities
//! (qualifiers, variables, schemas, resources, lint handlers) or replaces a
//! parent entity by providing a file with the same workspace-relative path.
//! Entities are never removed by layering, which keeps the composed workspace
//! straightforward to reason about.
//!
//! Composition happens at load time and materializes the merged workspace into a
//! temporary directory. Everything downstream (inspection, lint, runtime
//! resolution) then operates on that single merged root unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use tempfile::TempDir;
use toml::Value as TomlValue;
use toml::map::Map as TomlMap;

use crate::error::{Result, RototoError};
use crate::source::{LoadedWorkspaceSource, SourceOptions, StagedWorkspace, stage_source_once};
use crate::workspace::read_toml;

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
const SUPPORTED_SCHEMA_VERSION: i64 = 1;
const MAX_LAYER_DEPTH: usize = 32;

/// A single workspace in a layered composition.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkspaceLayer {
    /// The source string that produced this layer.
    pub source: String,
}

/// Provenance for a composed workspace.
///
/// `layers` is ordered base-first: index `0` is the deepest parent and the last
/// entry is the most-derived workspace that was requested. A workspace with no
/// `extends` has a single layer.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct WorkspaceLayers {
    pub layers: Vec<WorkspaceLayer>,
    /// Maps a merged file's workspace-relative path to the index in `layers`
    /// that contributed it. Only populated for layered workspaces.
    #[serde(skip)]
    pub origins: BTreeMap<PathBuf, usize>,
}

impl WorkspaceLayers {
    /// Provenance for a workspace that does not extend anything.
    pub fn single(source: impl Into<String>) -> Self {
        Self {
            layers: vec![WorkspaceLayer {
                source: source.into(),
            }],
            origins: BTreeMap::new(),
        }
    }

    /// Whether the workspace is composed from more than one layer.
    pub fn is_layered(&self) -> bool {
        self.layers.len() > 1
    }

    pub fn layers(&self) -> &[WorkspaceLayer] {
        &self.layers
    }
}

/// Resolve and materialize the `extends` chain rooted at `root`.
///
/// If the staged workspace does not declare `extends`, this returns it unchanged
/// with single-layer provenance. Otherwise it stages each parent, validates the
/// chain, and materializes a merged workspace into a fresh temporary directory.
pub(crate) async fn compose_workspace_layers(
    root: LoadedWorkspaceSource,
    original_source: &str,
    options: &SourceOptions,
) -> Result<LoadedWorkspaceSource> {
    let fingerprint = root.fingerprint().cloned();
    let immutable = root.immutable();
    let root_staged = root.into_staged();

    // Walk the chain, most-derived first.
    let mut staged_layers: Vec<(String, StagedWorkspace)> =
        vec![(original_source.to_owned(), root_staged)];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    seen.insert(source_key(original_source).await);

    loop {
        let is_root = staged_layers.len() == 1;
        let (current_source, current_staged) = staged_layers
            .last()
            .expect("chain always has at least the root layer");
        let manifest = match read_toml(&current_staged.path().join(WORKSPACE_MANIFEST)).await {
            Ok(manifest) => manifest,
            // A missing or unparseable root manifest is not a layering error: leave
            // the workspace untouched so downstream lint reports the real
            // diagnostic. A broken parent manifest is a genuine layering failure.
            Err(_) if is_root => break,
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read parent workspace manifest for `{current_source}`: {err}"
                )));
            }
        };
        let extends = match manifest.get("extends") {
            None => break,
            Some(value) => value.as_str().ok_or_else(|| {
                RototoError::new(format!(
                    "workspace `{current_source}` declares `extends` but it is not a string"
                ))
            })?,
        };
        if extends.trim().is_empty() {
            return Err(RototoError::new(format!(
                "workspace `{current_source}` declares an empty `extends`"
            )));
        }
        if staged_layers.len() >= MAX_LAYER_DEPTH {
            return Err(RototoError::new(format!(
                "workspace layering exceeds the maximum depth of {MAX_LAYER_DEPTH}"
            )));
        }

        let parent_source = resolve_extends_source(extends, current_staged.path());
        let key = source_key(&parent_source).await;
        if !seen.insert(key) {
            return Err(RototoError::new(format!(
                "workspace layering forms a cycle: `{parent_source}` extended by `{current_source}` is already in the chain"
            )));
        }

        let parent = stage_source_once(&parent_source, options).await.map_err(|err| {
            RototoError::new(format!(
                "failed to load parent workspace `{parent_source}` extended by `{current_source}`: {err}"
            ))
        })?;
        staged_layers.push((parent_source, parent.into_staged()));
    }

    if staged_layers.len() == 1 {
        let (source, staged) = staged_layers.pop().expect("single layer is present");
        return Ok(LoadedWorkspaceSource::new(
            staged,
            fingerprint,
            immutable,
            WorkspaceLayers::single(source),
        ));
    }

    materialize(staged_layers, fingerprint, immutable).await
}

/// Copy each layer into a fresh temporary directory, base-first, then write the
/// merged manifest.
async fn materialize(
    staged_layers: Vec<(String, StagedWorkspace)>,
    fingerprint: Option<crate::source::SourceFingerprint>,
    immutable: bool,
) -> Result<LoadedWorkspaceSource> {
    let merged_dir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let merged_root = merged_dir.path().join("workspace");
    tokio::fs::create_dir_all(&merged_root)
        .await
        .map_err(|err| RototoError::new(format!("failed to create merged workspace: {err}")))?;

    let mut origins: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let mut layer_infos: Vec<WorkspaceLayer> = Vec::new();
    let mut manifests: Vec<TomlValue> = Vec::new();

    // base-first
    for (index, (source, staged)) in staged_layers.iter().rev().enumerate() {
        let manifest = read_toml(&staged.path().join(WORKSPACE_MANIFEST)).await?;
        validate_schema_version(&manifest, source)?;
        manifests.push(manifest);
        layer_infos.push(WorkspaceLayer {
            source: source.clone(),
        });

        let layer_root = staged.path().to_path_buf();
        let merged = merged_root.clone();
        let copied = tokio::task::spawn_blocking(move || copy_layer_overlay(&layer_root, &merged))
            .await
            .map_err(|err| RototoError::new(format!("workspace overlay task failed: {err}")))??;
        for relative in copied {
            origins.insert(relative, index);
        }
    }

    let merged_manifest = merge_manifests(&manifests);
    let manifest_text = toml::to_string(&merged_manifest)
        .map_err(|err| RototoError::new(format!("failed to write merged manifest: {err}")))?;
    tokio::fs::write(merged_root.join(WORKSPACE_MANIFEST), manifest_text)
        .await
        .map_err(|err| RototoError::new(format!("failed to write merged manifest: {err}")))?;

    Ok(LoadedWorkspaceSource::new(
        StagedWorkspace::temporary(merged_root, merged_dir),
        fingerprint,
        immutable,
        WorkspaceLayers {
            layers: layer_infos,
            origins,
        },
    ))
}

fn validate_schema_version(manifest: &TomlValue, source: &str) -> Result<()> {
    let version = manifest
        .get("schema_version")
        .and_then(TomlValue::as_integer)
        .ok_or_else(|| {
            RototoError::new(format!(
                "workspace `{source}` must declare schema_version = {SUPPORTED_SCHEMA_VERSION}"
            ))
        })?;
    if version != SUPPORTED_SCHEMA_VERSION {
        return Err(RototoError::new(format!(
            "workspace `{source}` declares unsupported schema_version {version}; layered workspaces must all declare schema_version = {SUPPORTED_SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

/// Resolve a parent source declared in `extends`.
///
/// URI sources (containing `://`) and absolute paths are used verbatim; relative
/// local paths resolve against the declaring layer's root.
fn resolve_extends_source(extends: &str, current_root: &Path) -> String {
    if extends.contains("://") {
        return extends.to_owned();
    }
    let path = Path::new(extends);
    if path.is_absolute() {
        return extends.to_owned();
    }
    current_root.join(path).to_string_lossy().into_owned()
}

/// A stable identity for cycle detection. Local paths are canonicalized; URI
/// sources are compared by their string form.
async fn source_key(source: &str) -> String {
    if source.contains("://") {
        return source.to_owned();
    }
    match tokio::fs::canonicalize(source).await {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => source.to_owned(),
    }
}

/// Copy every file from `layer_root` into `merged_root`, overwriting existing
/// files (so a more-derived layer replaces a parent's file of the same path).
/// The root manifest is handled separately and skipped here. Returns the
/// workspace-relative paths of the files copied.
fn copy_layer_overlay(layer_root: &Path, merged_root: &Path) -> Result<Vec<PathBuf>> {
    let mut copied = Vec::new();
    copy_layer_overlay_inner(layer_root, merged_root, layer_root, &mut copied)?;
    Ok(copied)
}

fn copy_layer_overlay_inner(
    layer_root: &Path,
    merged_root: &Path,
    dir: &Path,
    copied: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|err| {
        RototoError::new(format!(
            "failed to read workspace layer {}: {err}",
            dir.display()
        ))
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|err| RototoError::new(format!("failed to read layer entry: {err}")))?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|err| {
            RototoError::new(format!(
                "failed to inspect layer entry {}: {err}",
                path.display()
            ))
        })?;
        let relative = path
            .strip_prefix(layer_root)
            .map_err(|err| RototoError::new(err.to_string()))?
            .to_path_buf();
        if metadata.is_dir() {
            // Do not descend into a nested VCS directory.
            if relative.components().next() == Some(Component::Normal(std::ffi::OsStr::new(".git")))
            {
                continue;
            }
            std::fs::create_dir_all(merged_root.join(&relative)).map_err(|err| {
                RototoError::new(format!("failed to create merged directory: {err}"))
            })?;
            copy_layer_overlay_inner(layer_root, merged_root, &path, copied)?;
        } else if metadata.is_file() {
            if relative == Path::new(WORKSPACE_MANIFEST) {
                continue;
            }
            let target = merged_root.join(&relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    RototoError::new(format!("failed to create merged directory: {err}"))
                })?;
            }
            std::fs::copy(&path, &target).map_err(|err| {
                RototoError::new(format!(
                    "failed to copy layer file {}: {err}",
                    path.display()
                ))
            })?;
            copied.push(relative);
        } else {
            return Err(RototoError::new(format!(
                "workspace layer contains unsupported entry type: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Merge layer manifests (base-first) into a single effective manifest.
///
/// Top-level keys use last-writer-wins (the more-derived layer wins), so the
/// most-derived `[environments]` and `[context]` are authoritative. Custom lint
/// rules under `[[lint.rule]]` are additive: rules from all layers are kept,
/// deduplicated by `id` with the more-derived declaration winning. `extends` is
/// dropped from the merged manifest.
fn merge_manifests(manifests: &[TomlValue]) -> TomlValue {
    let mut scalars: TomlMap<String, TomlValue> = TomlMap::new();
    let mut tables: TomlMap<String, TomlValue> = TomlMap::new();
    let mut lint_other: TomlMap<String, TomlValue> = TomlMap::new();
    let mut lint_rules: Vec<TomlValue> = Vec::new();
    let mut lint_rule_index: BTreeMap<String, usize> = BTreeMap::new();
    let mut has_lint = false;

    for manifest in manifests {
        let Some(table) = manifest.as_table() else {
            continue;
        };
        for (key, value) in table {
            match key.as_str() {
                "extends" | "schema_version" => {}
                "lint" => {
                    has_lint = true;
                    if let Some(lint_table) = value.as_table() {
                        for (lint_key, lint_value) in lint_table {
                            if lint_key == "rule" {
                                if let Some(rules) = lint_value.as_array() {
                                    for rule in rules {
                                        merge_lint_rule(
                                            rule,
                                            &mut lint_rules,
                                            &mut lint_rule_index,
                                        );
                                    }
                                }
                            } else {
                                lint_other.insert(lint_key.clone(), lint_value.clone());
                            }
                        }
                    }
                }
                _ if value.is_table() || value.is_array() => {
                    tables.insert(key.clone(), value.clone());
                }
                _ => {
                    scalars.insert(key.clone(), value.clone());
                }
            }
        }
    }

    // Emit scalars before tables so the serialized manifest is valid TOML.
    let mut merged: TomlMap<String, TomlValue> = TomlMap::new();
    merged.insert(
        "schema_version".to_owned(),
        TomlValue::Integer(SUPPORTED_SCHEMA_VERSION),
    );
    for (key, value) in scalars {
        merged.insert(key, value);
    }
    for (key, value) in tables {
        merged.insert(key, value);
    }
    if has_lint {
        let mut lint_table = lint_other;
        if !lint_rules.is_empty() {
            lint_table.insert("rule".to_owned(), TomlValue::Array(lint_rules));
        }
        merged.insert("lint".to_owned(), TomlValue::Table(lint_table));
    }

    TomlValue::Table(merged)
}

fn merge_lint_rule(
    rule: &TomlValue,
    rules: &mut Vec<TomlValue>,
    index: &mut BTreeMap<String, usize>,
) {
    let id = rule
        .as_table()
        .and_then(|table| table.get("id"))
        .and_then(TomlValue::as_str)
        .map(str::to_owned);
    match id {
        Some(id) => match index.get(&id) {
            Some(&position) => rules[position] = rule.clone(),
            None => {
                index.insert(id, rules.len());
                rules.push(rule.clone());
            }
        },
        None => rules.push(rule.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> TomlValue {
        text.parse::<TomlValue>().expect("valid TOML fixture")
    }

    #[test]
    fn merge_drops_extends_and_forces_schema_version() {
        let base = parse("schema_version = 1\n[environments]\nvalues = ['dev', 'prod']\n");
        let child = parse("schema_version = 1\nextends = '../base'\n");
        let merged = merge_manifests(&[base, child]);

        assert_eq!(merged.get("extends"), None);
        assert_eq!(
            merged.get("schema_version").and_then(TomlValue::as_integer),
            Some(1)
        );
    }

    #[test]
    fn child_environments_override_parent() {
        let base = parse("schema_version = 1\n[environments]\nvalues = ['dev', 'prod']\n");
        let child = parse(
            "schema_version = 1\nextends = '../base'\n[environments]\nvalues = ['dev', 'prod', 'canary']\n",
        );
        let merged = merge_manifests(&[base, child]);

        let values: Vec<&str> = merged["environments"]["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(values, ["dev", "prod", "canary"]);
    }

    #[test]
    fn context_section_is_inherited_when_child_omits_it() {
        let base = parse(
            "schema_version = 1\n[environments]\nvalues = ['prod']\n[context]\nschema = 'schemas/context.json'\n",
        );
        let child = parse("schema_version = 1\nextends = '../base'\n");
        let merged = merge_manifests(&[base, child]);

        assert_eq!(
            merged["context"]["schema"].as_str(),
            Some("schemas/context.json")
        );
    }

    #[test]
    fn lint_rules_are_additive_with_child_override() {
        let base = parse(
            "schema_version = 1\n[environments]\nvalues = ['prod']\n[[lint.rule]]\nid = 'a'\ntitle = 'A'\nhelp = 'ha'\n[[lint.rule]]\nid = 'b'\ntitle = 'B base'\nhelp = 'hb'\n",
        );
        let child = parse(
            "schema_version = 1\nextends = '../base'\n[[lint.rule]]\nid = 'b'\ntitle = 'B child'\nhelp = 'hb2'\n[[lint.rule]]\nid = 'c'\ntitle = 'C'\nhelp = 'hc'\n",
        );
        let merged = merge_manifests(&[base, child]);

        let rules = merged["lint"]["rule"].as_array().unwrap();
        let ids: Vec<&str> = rules
            .iter()
            .map(|rule| rule["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["a", "b", "c"], "rules unioned by id, child appended");
        let rule_b = rules
            .iter()
            .find(|rule| rule["id"].as_str() == Some("b"))
            .unwrap();
        assert_eq!(rule_b["title"].as_str(), Some("B child"));
    }

    #[test]
    fn resolve_extends_source_handles_paths_and_uris() {
        assert_eq!(
            resolve_extends_source("../base", Path::new("/workspaces/child")),
            "/workspaces/child/../base"
        );
        assert_eq!(
            resolve_extends_source("/abs/base", Path::new("/workspaces/child")),
            "/abs/base"
        );
        assert_eq!(
            resolve_extends_source("git+https://h/r#main:base", Path::new("/workspaces/child")),
            "git+https://h/r#main:base"
        );
    }
}
