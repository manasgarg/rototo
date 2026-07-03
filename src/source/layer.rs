use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tempfile::TempDir;

use crate::error::{Result, RototoError};
use crate::package::package_extends_sources;

use super::PACKAGE_MANIFEST;
use super::governance::{Operation, read_governance_contract};
use super::load::{load_single_package_source, load_single_package_source_snapshot};
use super::path::relative_path_is_safe;
use super::types::{
    ExtendSourceBase, LoadedPackageSource, LocalStageMode, ResolvedExtendSource, SourceFingerprint,
    SourceLayer, SourceOptions, StagedPackage,
};
use super::uri::SourceUri;

const MAX_PACKAGE_EXTENDS_DEPTH: usize = 32;

pub(super) fn load_package_source_graph<'a>(
    source: &'a str,
    options: &'a SourceOptions,
    local_mode: LocalStageMode,
    base: Option<ExtendSourceBase<'a>>,
    stack: &'a mut Vec<String>,
) -> Pin<Box<dyn Future<Output = Result<LoadedPackageSource>> + Send + 'a>> {
    Box::pin(async move {
        if stack.len() >= MAX_PACKAGE_EXTENDS_DEPTH {
            return Err(RototoError::new(format!(
                "package extends depth exceeded {MAX_PACKAGE_EXTENDS_DEPTH}"
            )));
        }

        let resolved_source = resolve_extend_source(source, base)?;
        let loaded = match local_mode {
            LocalStageMode::Borrow => {
                load_single_package_source(&resolved_source.source, options).await?
            }
            LocalStageMode::Snapshot => {
                load_single_package_source_snapshot(&resolved_source.source, options).await?
            }
        };
        let layer_key = package_source_key(&resolved_source.source, loaded.staged()).await?;
        if let Some(cycle_start) = stack.iter().position(|key| key == &layer_key) {
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(layer_key);
            return Err(RototoError::new(format!(
                "package extends cycle detected: {}",
                cycle.join(" -> ")
            )));
        }

        stack.push(layer_key);
        let result = project_package_source_graph(
            loaded,
            options,
            local_mode,
            resolved_source.inherited_temporary_base,
            stack,
        )
        .await;
        stack.pop();
        result
    })
}

async fn project_package_source_graph(
    loaded: LoadedPackageSource,
    options: &SourceOptions,
    local_mode: LocalStageMode,
    inherited_temporary_base: bool,
    stack: &mut Vec<String>,
) -> Result<LoadedPackageSource> {
    let extends = read_package_extends(loaded.staged().path()).await?;
    if extends.is_empty() {
        return Ok(loaded);
    }

    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let target = tempdir.path().join("package");
    let base_path = extend_source_base_path(&loaded);
    let base = ExtendSourceBase {
        path: &base_path,
        temporary: inherited_temporary_base
            || (loaded.staged().is_temporary() && base_path == loaded.staged().path()),
    };
    let mut layers = Vec::new();
    let mut immutable = true;
    for parent_source in &extends {
        let parent =
            load_package_source_graph(parent_source, options, local_mode, Some(base), stack)
                .await?;
        enforce_layer_governance(parent.staged().path(), &target).await?;
        copy_package_layer(
            parent.staged().path(),
            &target,
            false,
            layer_label(&parent, parent_source),
        )
        .await?;
        immutable &= parent.immutable();
        layers.extend(parent.layers().iter().cloned());
    }

    enforce_layer_governance(loaded.staged().path(), &target).await?;
    let child_label = loaded
        .layers()
        .last()
        .map(|layer| layer.source().to_owned())
        .unwrap_or_else(|| "package".to_owned());
    copy_package_layer(loaded.staged().path(), &target, true, child_label).await?;
    immutable &= loaded.immutable();
    layers.extend(loaded.layers().iter().cloned());
    let fingerprint = combined_layer_fingerprint(&layers);
    Ok(LoadedPackageSource {
        staged: StagedPackage::temporary(target, tempdir),
        fingerprint,
        immutable,
        layers,
    })
}

/// Enforce the layering contract carried by the projection built so far on
/// the layer about to land on it. A projection with no governance.toml is
/// ungoverned; with one, the incoming layer is default-closed over the
/// entities the projection declares.
async fn enforce_layer_governance(layer: &Path, target: &Path) -> Result<()> {
    let layer = layer.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || enforce_layer_governance_sync(&layer, &target))
        .await
        .map_err(|err| RototoError::new(format!("governance enforcement task failed: {err}")))?
}

fn enforce_layer_governance_sync(layer: &Path, target: &Path) -> Result<()> {
    let Some(contract) = read_governance_contract(target) else {
        return Ok(());
    };

    let mut pending = vec![layer.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read package layer {}: {err}",
                    directory.display()
                )));
            }
        };
        for entry in entries {
            let entry = entry.map_err(|err| {
                RototoError::new(format!("failed to read package layer entry: {err}"))
            })?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let Ok(relative) = path.strip_prefix(layer) else {
                continue;
            };
            check_governed_file(&contract, &path, relative, target)?;
        }
    }
    Ok(())
}

fn check_governed_file(
    contract: &super::governance::GovernanceContract,
    source_path: &Path,
    relative: &Path,
    target: &Path,
) -> Result<()> {
    let components: Vec<&str> = relative
        .iter()
        .filter_map(|component| component.to_str())
        .collect();
    let exists = |candidate: &Path| target.join(candidate).is_file();
    let stem = |file: &str, suffix: &str| file.strip_suffix(suffix).map(str::to_owned);

    match components.as_slice() {
        // The manifest is the layer's own identity, and the governance file
        // itself is checked as a ceiling, not as an operation.
        [file] if *file == PACKAGE_MANIFEST => Ok(()),
        ["governance.toml"] => {
            let text = std::fs::read_to_string(source_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to read package layer file {}: {err}",
                    source_path.display()
                ))
            })?;
            let value = text.parse::<toml::Value>().map_err(|err| {
                RototoError::new(format!("failed to parse layer governance.toml: {err}"))
            })?;
            contract.check_ceiling(&super::governance::parse_contract_value(&value))
        }
        ["model", "catalogs", file] => match stem(file, ".schema.json") {
            Some(id) if exists(relative) => {
                contract.check("catalog", &id, Operation::Constrain, None, &[])
            }
            _ => Ok(()),
        },
        ["model", "enums", file] => match stem(file, ".toml") {
            Some(id) if exists(relative) => {
                contract.check("enum", &id, Operation::Constrain, None, &[])
            }
            _ => Ok(()),
        },
        ["model", "context", file] => match stem(file, ".schema.json") {
            Some(id) if exists(relative) => {
                contract.check("evaluation_context", &id, Operation::Constrain, None, &[])
            }
            _ => Ok(()),
        },
        ["model", "context", samples, _] => match samples.strip_suffix("-samples") {
            Some(id)
                if exists(Path::new(&format!("model/context/{id}.schema.json")))
                    && exists(relative) =>
            {
                contract.check("evaluation_context", id, Operation::Constrain, None, &[])
            }
            _ => Ok(()),
        },
        ["data", "enums", file] => match stem(file, ".toml") {
            Some(id) if exists(relative) => {
                contract.check("enum", &id, Operation::Update, None, &[])
            }
            Some(id) if exists(Path::new(&format!("model/enums/{id}.toml"))) => {
                contract.check("enum", &id, Operation::Add, None, &[])
            }
            _ => Ok(()),
        },
        ["data", "catalogs", catalog, file] => {
            let declared = exists(Path::new(&format!("model/catalogs/{catalog}.schema.json")));
            if !declared {
                // A catalog this layer introduces is its own to fill.
                return Ok(());
            }
            if let Some(entry) = stem(file, ".deleted.toml") {
                return contract.check("catalog", catalog, Operation::Delete, Some(&entry), &[]);
            }
            if let Some(entry) = stem(file, ".patch.toml") {
                let fields = toml_top_level_keys(source_path)?;
                return contract.check(
                    "catalog",
                    catalog,
                    Operation::Update,
                    Some(&entry),
                    &fields,
                );
            }
            match stem(file, ".toml") {
                Some(entry) if exists(relative) => Err(RototoError::new(format!(
                    "governance does not model replacing catalog entry {entry} wholesale;                      use {entry}.patch.toml to update fields or {entry}.deleted.toml to                      remove it"
                ))),
                Some(_) => contract.check("catalog", catalog, Operation::Add, None, &[]),
                None => Ok(()),
            }
        }
        ["variables", .., file] => match stem(file, ".toml") {
            Some(_) if exists(relative) => {
                let id = relative
                    .strip_prefix("variables")
                    .ok()
                    .and_then(|path| path.with_extension("").to_str().map(str::to_owned))
                    .map(|id| id.replace(std::path::MAIN_SEPARATOR, "/"))
                    .unwrap_or_default();
                contract.check("variable", &id, Operation::Override, None, &[])
            }
            _ => Ok(()),
        },
        ["layers", file] => match stem(file, ".toml") {
            Some(id) if exists(relative) => {
                contract.check("layer", &id, Operation::Override, None, &[])
            }
            _ => Ok(()),
        },
        ["lint", file] if exists(relative) => Err(RototoError::new(format!(
            "governance does not model replacing a lint file the layer below owns: lint/{file}"
        ))),
        _ => Ok(()),
    }
}

/// The top-level keys of a patch file, which are the fields it updates.
fn toml_top_level_keys(path: &Path) -> Result<Vec<String>> {
    let value = read_layer_toml(path)?;
    Ok(value
        .as_table()
        .map(|table| table.keys().cloned().collect())
        .unwrap_or_default())
}

/// The label a layer's resolve contributions are recorded under: the source
/// string the author wrote in `extends`, or the layer's own identity.
fn layer_label(loaded: &LoadedPackageSource, written_source: &str) -> String {
    if !written_source.trim().is_empty() {
        return written_source.to_owned();
    }
    loaded
        .layers()
        .last()
        .map(|layer| layer.source().to_owned())
        .unwrap_or_else(|| "package".to_owned())
}

/// The sidecar the flatten leaves in the projection: which layer's
/// `[resolve]` block each variable ended up carrying. The resolution trace
/// reads it back as provenance.
pub(crate) const RESOLVE_PROVENANCE_FILE: &str = ".rototo-provenance.json";

async fn copy_package_layer(
    source: &Path,
    target: &Path,
    include_manifest: bool,
    label: String,
) -> Result<()> {
    let source = source.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || {
        // A layer that is itself a flattened projection carries its own
        // sidecar naming which of its sub-layers produced each block; those
        // finer labels win over the layer's single label.
        let nested = read_provenance(&source);
        let mut provenance = BTreeMap::new();
        copy_package_layer_recursive(
            &source,
            &target,
            include_manifest,
            Path::new(""),
            &LayerProvenance {
                label: &label,
                nested: &nested,
                recorded: std::cell::RefCell::new(&mut provenance),
            },
        )?;
        if !provenance.is_empty() {
            let mut merged = read_provenance(&target);
            merged.extend(provenance);
            write_provenance(&target, &merged)?;
        }
        Ok(())
    })
    .await
    .map_err(|err| RototoError::new(format!("package layer copy task failed: {err}")))?
}

struct LayerProvenance<'a> {
    label: &'a str,
    nested: &'a BTreeMap<String, String>,
    recorded: std::cell::RefCell<&'a mut BTreeMap<String, String>>,
}

impl LayerProvenance<'_> {
    fn record(&self, variable_id: &str) {
        let label = self
            .nested
            .get(variable_id)
            .cloned()
            .unwrap_or_else(|| self.label.to_owned());
        self.recorded
            .borrow_mut()
            .insert(variable_id.to_owned(), label);
    }
}

/// Read the provenance sidecar for the runtime; a package that never
/// composed has none.
pub(crate) async fn read_resolve_provenance(root: &Path) -> BTreeMap<String, String> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || read_provenance(&root))
        .await
        .unwrap_or_default()
}

fn read_provenance(root: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(root.join(RESOLVE_PROVENANCE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_provenance(root: &Path, provenance: &BTreeMap<String, String>) -> Result<()> {
    let text = serde_json::to_string_pretty(provenance)
        .map_err(|err| RototoError::new(format!("failed to serialize provenance: {err}")))?;
    std::fs::write(root.join(RESOLVE_PROVENANCE_FILE), text).map_err(|err| {
        RototoError::new(format!(
            "failed to write provenance sidecar in {}: {err}",
            root.display()
        ))
    })
}

/// The variable id a `variables/**.toml` path names, if it is one.
fn variable_id_for_relative(relative: &Path) -> Option<String> {
    let path = relative.strip_prefix("variables").ok()?;
    if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
        return None;
    }
    path.with_extension("")
        .to_str()
        .map(|id| id.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn copy_package_layer_recursive(
    source: &Path,
    target: &Path,
    include_manifest: bool,
    relative: &Path,
    provenance: &LayerProvenance<'_>,
) -> Result<()> {
    let metadata = std::fs::metadata(source).map_err(|err| {
        RototoError::new(format!(
            "failed to inspect package layer {}: {err}",
            source.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "package layer source is not a directory: {}",
            source.display()
        )));
    }
    std::fs::create_dir_all(target).map_err(|err| {
        RototoError::new(format!(
            "failed to create package projection {}: {err}",
            target.display()
        ))
    })?;
    let root = relative.as_os_str().is_empty();
    let mut entries = std::fs::read_dir(source)
        .map_err(|err| {
            RototoError::new(format!(
                "failed to read package layer {}: {err}",
                source.display()
            ))
        })?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|err| RototoError::new(format!("failed to read package layer entry: {err}")))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_name = entry.file_name();
        if root && !include_manifest && file_name == PACKAGE_MANIFEST {
            continue;
        }
        if root && file_name == RESOLVE_PROVENANCE_FILE {
            // The sidecar is folded via the nested map and written once at
            // the end of the layer copy; copying it raw would clobber the
            // entries earlier layers contributed.
            continue;
        }
        let source_path = entry.path();
        let target_path = target.join(&file_name);
        let relative_path = relative.join(&file_name);
        let metadata = entry.metadata().map_err(|err| {
            RototoError::new(format!(
                "failed to inspect package layer entry {}: {err}",
                source_path.display()
            ))
        })?;
        if metadata.is_dir() {
            if target_path.is_file() {
                std::fs::remove_file(&target_path).map_err(|err| {
                    RototoError::new(format!(
                        "failed to replace projected package file {}: {err}",
                        target_path.display()
                    ))
                })?;
            }
            copy_package_layer_recursive(
                &source_path,
                &target_path,
                include_manifest,
                &relative_path,
                provenance,
            )?;
        } else if metadata.is_file() {
            if target_path.is_dir() {
                std::fs::remove_dir_all(&target_path).map_err(|err| {
                    RototoError::new(format!(
                        "failed to replace projected package directory {}: {err}",
                        target_path.display()
                    ))
                })?;
            }
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    RototoError::new(format!(
                        "failed to create projected package directory {}: {err}",
                        parent.display()
                    ))
                })?;
            }
            compose_package_layer_file(
                &source_path,
                target,
                &file_name,
                &relative_path,
                provenance,
            )?;
        } else {
            return Err(RototoError::new(format!(
                "package layer contains unsupported entry type: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

/// How one layer file lands on the projection built from the layers below it.
enum LayerFileComposition {
    /// Plain copy; a same-path file below is replaced whole.
    Replace,
    /// `data/catalogs/<id>/<entry>.deleted.toml`: remove the entry a layer
    /// below provided. The deleted marker itself never lands in the
    /// projection.
    CatalogEntryDeleted { entry: String },
    /// `data/catalogs/<id>/<entry>.patch.toml`: field-level override of the
    /// entry a layer below provided; unpatched fields are inherited.
    CatalogEntryPatch { entry: String },
    /// `variables/**.toml` over an existing file: top-level keys replace the
    /// base's (so an overlay `[resolve]` block replaces the whole resolution),
    /// keys the overlay does not declare are inherited.
    VariableMerge,
    /// `data/enums/<id>.toml` over an existing file: the member sets union,
    /// keeping enum members tenant-extensible data.
    EnumMembersUnion,
}

/// Classify a layer file by its package-relative path. Composition is
/// path-shaped: only catalog entries and variables compose structurally;
/// everything else replaces whole.
fn classify_layer_file(
    relative: &Path,
    file_name: &str,
    target_exists: bool,
) -> LayerFileComposition {
    let components: Vec<&str> = relative
        .iter()
        .filter_map(|component| component.to_str())
        .collect();
    match components.as_slice() {
        ["data", "catalogs", _, _] => {
            if let Some(entry) = file_name.strip_suffix(".deleted.toml") {
                return LayerFileComposition::CatalogEntryDeleted {
                    entry: entry.to_owned(),
                };
            }
            if let Some(entry) = file_name.strip_suffix(".patch.toml") {
                return LayerFileComposition::CatalogEntryPatch {
                    entry: entry.to_owned(),
                };
            }
            LayerFileComposition::Replace
        }
        ["variables", .., _] if target_exists && file_name.ends_with(".toml") => {
            LayerFileComposition::VariableMerge
        }
        ["data", "enums", _] if target_exists && file_name.ends_with(".toml") => {
            LayerFileComposition::EnumMembersUnion
        }
        _ => LayerFileComposition::Replace,
    }
}

fn compose_package_layer_file(
    source_path: &Path,
    target_dir: &Path,
    file_name: &std::ffi::OsStr,
    relative: &Path,
    provenance: &LayerProvenance<'_>,
) -> Result<()> {
    let target_path = target_dir.join(file_name);
    let file_name = file_name.to_string_lossy();
    match classify_layer_file(relative, &file_name, target_path.is_file()) {
        LayerFileComposition::Replace => {
            std::fs::copy(source_path, &target_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to copy package layer entry {}: {err}",
                    source_path.display()
                ))
            })?;
            if let Some(id) = variable_id_for_relative(relative) {
                provenance.record(&id);
            }
            Ok(())
        }
        LayerFileComposition::CatalogEntryDeleted { entry } => {
            reject_same_layer_entry(source_path, &entry, "deleted marker")?;
            let entry_path = target_dir.join(format!("{entry}.toml"));
            if !entry_path.is_file() {
                return Err(RototoError::new(format!(
                    "deleted marker has no catalog entry to remove in the layers below: {}",
                    relative.display()
                )));
            }
            std::fs::remove_file(&entry_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to remove deleted catalog entry {}: {err}",
                    entry_path.display()
                ))
            })?;
            Ok(())
        }
        LayerFileComposition::CatalogEntryPatch { entry } => {
            reject_same_layer_entry(source_path, &entry, "patch")?;
            let entry_path = target_dir.join(format!("{entry}.toml"));
            if !entry_path.is_file() {
                return Err(RototoError::new(format!(
                    "patch has no catalog entry to override in the layers below: {}",
                    relative.display()
                )));
            }
            let mut base = read_layer_toml(&entry_path)?;
            let patch = read_layer_toml(source_path)?;
            deep_merge_toml(&mut base, patch);
            write_layer_toml(&entry_path, &base)
        }
        LayerFileComposition::VariableMerge => {
            let mut base = read_layer_toml(&target_path)?;
            let overlay = read_layer_toml(source_path)?;
            let replaces_resolve = overlay
                .as_table()
                .is_some_and(|table| table.contains_key("resolve"));
            merge_variable_toml(&mut base, overlay, relative)?;
            write_layer_toml(&target_path, &base)?;
            if replaces_resolve && let Some(id) = variable_id_for_relative(relative) {
                provenance.record(&id);
            }
            Ok(())
        }
        LayerFileComposition::EnumMembersUnion => {
            let mut base = read_layer_toml(&target_path)?;
            let overlay = read_layer_toml(source_path)?;
            union_enum_members(&mut base, overlay);
            write_layer_toml(&target_path, &base)
        }
    }
}

/// A layer that both provides `<entry>.toml` and deletes or patches the
/// same entry is contradicting itself; composition targets the layers below.
fn reject_same_layer_entry(source_path: &Path, entry: &str, operation: &str) -> Result<()> {
    let sibling = source_path
        .parent()
        .map(|parent| parent.join(format!("{entry}.toml")))
        .filter(|sibling| sibling.is_file());
    if sibling.is_some() {
        return Err(RototoError::new(format!(
            "layer both provides catalog entry {entry} and declares a {operation} for it"
        )));
    }
    Ok(())
}

fn read_layer_toml(path: &Path) -> Result<toml::Value> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        RototoError::new(format!(
            "failed to read package layer file {}: {err}",
            path.display()
        ))
    })?;
    text.parse::<toml::Value>().map_err(|err| {
        RototoError::new(format!(
            "failed to parse package layer file {}: {err}",
            path.display()
        ))
    })
}

fn write_layer_toml(path: &Path, value: &toml::Value) -> Result<()> {
    let text = toml::to_string_pretty(value)
        .map_err(|err| RototoError::new(format!("failed to serialize composed file: {err}")))?;
    std::fs::write(path, text).map_err(|err| {
        RototoError::new(format!(
            "failed to write composed package file {}: {err}",
            path.display()
        ))
    })
}

/// Deep merge for catalog entry patches: tables merge recursively, everything
/// else (scalars, arrays) replaces, and unpatched fields are inherited.
fn deep_merge_toml(base: &mut toml::Value, patch: toml::Value) {
    match (base, patch) {
        (toml::Value::Table(base), toml::Value::Table(patch)) => {
            for (key, value) in patch {
                match base.get_mut(&key) {
                    Some(existing) => deep_merge_toml(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, patch) => *base = patch,
    }
}

/// Union an overlay's enum members into the base's: members are a set, so a
/// layer extends it by declaring the members it adds; the base's members are
/// kept, duplicates collapse, and order is base first.
fn union_enum_members(base: &mut toml::Value, overlay: toml::Value) {
    let (Some(base_table), Some(overlay_table)) = (base.as_table_mut(), overlay.as_table()) else {
        return;
    };
    let Some(toml::Value::Array(base_members)) = base_table.get_mut("members") else {
        if let Some(members) = overlay_table.get("members") {
            base_table.insert("members".to_owned(), members.clone());
        }
        return;
    };
    if let Some(toml::Value::Array(overlay_members)) = overlay_table.get("members") {
        for member in overlay_members {
            if !base_members.contains(member) {
                base_members.push(member.clone());
            }
        }
    }
}

/// Merge an overlay variable file over the base's: every top-level key the
/// overlay declares replaces the base's key whole, so `[resolve]` swaps
/// atomically and the type (and anything else left out) stays with the base.
///
/// The type is the one key that may not change: shape composes by narrowing
/// only, and for a variable's type the only narrowing is restating it. An
/// overlay that declares a different type is contradicting the contract, not
/// overriding a value.
fn merge_variable_toml(
    base: &mut toml::Value,
    overlay: toml::Value,
    relative: &Path,
) -> Result<()> {
    let (toml::Value::Table(base), toml::Value::Table(overlay)) = (base, overlay) else {
        return Ok(());
    };
    if let (Some(toml::Value::String(below)), Some(toml::Value::String(above))) =
        (base.get("type"), overlay.get("type"))
        && below != above
    {
        return Err(RototoError::new(format!(
            "overlay changes the variable's type from {below} to {above}: {}; \
             the type stays with the layer that declared it",
            relative.display()
        )));
    }
    for (key, value) in overlay {
        base.insert(key, value);
    }
    Ok(())
}

async fn read_package_extends(root: &Path) -> Result<Vec<String>> {
    let path = root.join(PACKAGE_MANIFEST);
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(_) => return Ok(Vec::new()),
    };
    let manifest = text.parse::<toml::Value>().map_err(|err| {
        RototoError::new(format!(
            "failed to parse package manifest {}: {err}",
            path.display()
        ))
    })?;
    package_extends_sources(&manifest)
}

fn resolve_extend_source(
    source: &str,
    base: Option<ExtendSourceBase<'_>>,
) -> Result<ResolvedExtendSource> {
    let uri = SourceUri::parse(source)?;
    if let Some(base) = base
        && base.temporary
    {
        if let Some(uri) = uri.as_ref() {
            if package_source_uri_is_local_filesystem(uri) {
                return Err(RototoError::new(format!(
                    "package extends source escapes a staged package: {source}"
                )));
            }
            return Ok(ResolvedExtendSource {
                source: source.to_owned(),
                inherited_temporary_base: false,
            });
        }
        if Path::new(source).is_absolute() || !relative_path_is_safe(Path::new(source)) {
            return Err(RototoError::new(format!(
                "relative package extends source escapes a staged package: {source}"
            )));
        }
        return Ok(ResolvedExtendSource {
            source: base.path.join(source).to_string_lossy().into_owned(),
            inherited_temporary_base: true,
        });
    }
    if uri.is_some() || Path::new(source).is_absolute() {
        return Ok(ResolvedExtendSource {
            source: source.to_owned(),
            inherited_temporary_base: false,
        });
    }
    let Some(base) = base else {
        return Ok(ResolvedExtendSource {
            source: source.to_owned(),
            inherited_temporary_base: false,
        });
    };
    Ok(ResolvedExtendSource {
        source: base.path.join(source).to_string_lossy().into_owned(),
        inherited_temporary_base: false,
    })
}

async fn package_source_key(source: &str, staged: &StagedPackage) -> Result<String> {
    if SourceUri::parse(source)?.is_some() {
        return Ok(source.to_owned());
    }
    let path = if source.is_empty() {
        staged.path()
    } else {
        Path::new(source)
    };
    tokio::fs::canonicalize(path)
        .await
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|err| {
            RototoError::new(format!(
                "failed to canonicalize package source {}: {err}",
                path.display()
            ))
        })
}

fn extend_source_base_path(loaded: &LoadedPackageSource) -> PathBuf {
    if loaded.staged().is_temporary()
        && let [layer] = loaded.layers()
        && SourceUri::parse(layer.source()).ok().flatten().is_none()
    {
        return PathBuf::from(layer.source());
    }
    loaded.staged().path().to_path_buf()
}

fn combined_layer_fingerprint(layers: &[SourceLayer]) -> Option<SourceFingerprint> {
    let mut fingerprints = Vec::with_capacity(layers.len());
    for layer in layers {
        fingerprints.push(layer.fingerprint.clone()?);
    }
    match fingerprints.len() {
        0 => None,
        1 => fingerprints.pop(),
        _ => Some(SourceFingerprint::PackageLayers(fingerprints)),
    }
}

fn package_source_uri_is_local_filesystem(uri: &SourceUri) -> bool {
    matches!(uri.scheme.as_str(), "file" | "git+file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_extend_base_rejects_local_filesystem_escape_sources() {
        let staged = tempfile::TempDir::new().unwrap();
        let base = ExtendSourceBase {
            path: staged.path(),
            temporary: true,
        };

        for source in [
            "/tmp/outside",
            "../outside",
            "file:///tmp/outside",
            "git+file:///tmp/outside.git",
        ] {
            let err = resolve_extend_source(source, Some(base)).unwrap_err();
            assert!(err.to_string().contains("escapes a staged package"));
        }

        let resolved = resolve_extend_source("parent", Some(base)).unwrap();
        assert_eq!(
            resolved.source,
            staged.path().join("parent").display().to_string()
        );
        assert!(resolved.inherited_temporary_base);
    }

    #[tokio::test]
    async fn read_package_extends_rejects_blank_sources() {
        let temp = tempfile::TempDir::new().unwrap();
        tokio::fs::write(
            temp.path().join(PACKAGE_MANIFEST),
            r#"schema_version = 1
extends = ["../base", "  "]
"#,
        )
        .await
        .unwrap();

        let err = read_package_extends(temp.path()).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("package extends source must not be blank")
        );
    }

    #[tokio::test]
    async fn parent_layer_copy_skips_only_root_manifest() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        tokio::fs::create_dir_all(source.join("data/catalogs/config"))
            .await
            .unwrap();
        tokio::fs::write(source.join(PACKAGE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();
        tokio::fs::write(
            source.join("data/catalogs/config").join(PACKAGE_MANIFEST),
            "value = true\n",
        )
        .await
        .unwrap();

        copy_package_layer(&source, &target, false, "test-layer".to_owned())
            .await
            .unwrap();

        assert!(!target.join(PACKAGE_MANIFEST).exists());
        assert!(
            target
                .join("data/catalogs/config")
                .join(PACKAGE_MANIFEST)
                .is_file()
        );
    }
}
