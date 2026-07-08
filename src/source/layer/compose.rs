use super::*;

pub(super) async fn copy_package_layer(
    source: &Path,
    target: &Path,
    include_manifest: bool,
    label: String,
    sibling_entities: Option<std::sync::Arc<std::sync::Mutex<BTreeMap<String, String>>>>,
) -> Result<()> {
    let source = source.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || {
        // A layer that is itself a flattened projection carries its own
        // sidecar naming which of its sub-layers produced each block; those
        // finer labels win over the layer's single label.
        let nested = read_provenance(&source);
        let mut provenance = BTreeMap::new();
        let siblings = sibling_entities.as_ref().map(|provided| SiblingBases {
            label: &label,
            provided,
        });
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
            siblings.as_ref(),
        )?;
        if !provenance.is_empty() {
            let mut merged = read_provenance(&target);
            merged.extend(provenance);
            write_provenance(&target, &merged)?;
        }
        Ok(())
    })
    .await
    .map_err(|err| RototoError::new(format!("package source copy task failed: {err}")))?
}

/// The variable id a `variables/**.toml` path names, if it is one.
pub(super) fn variable_id_for_relative(relative: &Path) -> Option<String> {
    let path = relative.strip_prefix("variables").ok()?;
    if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
        return None;
    }
    let id = path
        .with_extension("")
        .to_str()
        .map(|id| id.replace(std::path::MAIN_SEPARATOR, "/"))?;
    // An update marker names the variable it updates, not a `.update` id.
    Some(id.strip_suffix(".update").unwrap_or(&id).to_owned())
}

pub(super) fn copy_package_layer_recursive(
    source: &Path,
    target: &Path,
    include_manifest: bool,
    relative: &Path,
    provenance: &LayerProvenance<'_>,
    siblings: Option<&SiblingBases<'_>>,
) -> Result<()> {
    let metadata = std::fs::metadata(source).map_err(|err| {
        RototoError::new(format!(
            "failed to inspect package source {}: {err}",
            source.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(RototoError::new(format!(
            "package source is not a directory: {}",
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
                "failed to read package source {}: {err}",
                source.display()
            ))
        })?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|err| RototoError::new(format!("failed to read package source entry: {err}")))?;
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
                "failed to inspect package source entry {}: {err}",
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
                siblings,
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
            if let Some(siblings) = siblings
                && siblings.admit(&relative_path, &source_path, &target_path)?
            {
                continue;
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
                "package source contains unsupported entry type: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

/// How one layer file lands on the projection built from the layers below it.
pub(super) enum LayerFileComposition {
    /// Plain copy; a same-path file below is replaced whole.
    Replace,
    /// `data/catalogs/<id>/<entry>.deleted.toml`: remove the entry a layer
    /// below provided. The deleted marker itself never lands in the
    /// projection.
    CatalogEntryDeleted { entry: String },
    /// `data/catalogs/<id>/<entry>.update.toml`: field-level update of the
    /// entry a layer below provided; fields the update does not mention are
    /// inherited.
    CatalogEntryUpdate { entry: String },
    /// `variables/**/<id>.update.toml`: update the base variable's resolution
    /// (and description). The marker carries only the keys it changes; each
    /// replaces the base's key whole, and the marker itself never lands in
    /// the projection.
    VariableUpdate,
    /// A plain `variables/**.toml` over an existing base file. Byte-identical
    /// restatement is a no-op (diamond ancestry); anything else is an error
    /// pointing at the update marker.
    VariableRestate,
    /// `data/lists/<id>.toml`: the member sets compose - the overlay's
    /// `members` union into the base's and its `deleted` values are removed
    /// from the result, keeping list members tenant-adjustable data.
    ListUpdate,
    /// `lists/<id>.toml` restating a base list: byte-identical restatements
    /// compose as a no-op (diamond ancestry); anything else is an error,
    /// the update marker is the only spelling for changing one.
    ListRestate,
}

/// Classify a layer file by its package-relative path. Composition is
/// path-shaped: only catalog entries and variables compose structurally;
/// everything else replaces whole.
pub(super) fn classify_layer_file(
    relative: &Path,
    file_name: &str,
    target_exists: bool,
) -> LayerFileComposition {
    let components: Vec<&str> = relative
        .iter()
        .filter_map(|component| component.to_str())
        .collect();
    match components.as_slice() {
        ["data", "catalogs", middle @ .., _] if !middle.is_empty() => {
            if let Some(entry) = file_name.strip_suffix(".deleted.toml") {
                return LayerFileComposition::CatalogEntryDeleted {
                    entry: entry.to_owned(),
                };
            }
            if let Some(entry) = file_name.strip_suffix(".update.toml") {
                return LayerFileComposition::CatalogEntryUpdate {
                    entry: entry.to_owned(),
                };
            }
            LayerFileComposition::Replace
        }
        ["variables", .., _] if file_name.ends_with(".update.toml") => {
            LayerFileComposition::VariableUpdate
        }
        ["variables", .., _] if target_exists && file_name.ends_with(".toml") => {
            LayerFileComposition::VariableRestate
        }
        ["lists", .., _] if file_name.ends_with(".update.toml") => LayerFileComposition::ListUpdate,
        ["lists", .., _] if target_exists && file_name.ends_with(".toml") => {
            LayerFileComposition::ListRestate
        }
        _ => LayerFileComposition::Replace,
    }
}

pub(super) fn compose_package_layer_file(
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
                    "failed to copy package source entry {}: {err}",
                    source_path.display()
                ))
            })?;
            if let Some(id) = variable_id_for_relative(relative) {
                provenance.record(&id);
            }
            Ok(())
        }
        LayerFileComposition::CatalogEntryDeleted { entry } => {
            reject_same_layer_entry(source_path, &entry, "a deleted marker")?;
            let update_sibling = source_path
                .parent()
                .map(|parent| parent.join(format!("{entry}.update.toml")))
                .filter(|sibling| sibling.is_file());
            if update_sibling.is_some() {
                return Err(RototoError::new(format!(
                    "package both declares an update and a deleted marker for catalog entry \
                     {entry}; updating and removing it are contradictory"
                )));
            }
            let entry_path = target_dir.join(format!("{entry}.toml"));
            if !entry_path.is_file() {
                return Err(RototoError::new(format!(
                    "deleted marker has no catalog entry to remove in the base packages: {}",
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
        LayerFileComposition::CatalogEntryUpdate { entry } => {
            reject_same_layer_entry(source_path, &entry, "an update")?;
            let entry_path = target_dir.join(format!("{entry}.toml"));
            if !entry_path.is_file() {
                return Err(RototoError::new(format!(
                    "update has no catalog entry to update in the base packages: {}",
                    relative.display()
                )));
            }
            let mut base = read_layer_toml(&entry_path)?;
            let update = read_layer_toml(source_path)?;
            deep_merge_toml(&mut base, update);
            write_layer_toml(&entry_path, &base)
        }
        LayerFileComposition::VariableUpdate => {
            let id = variable_id_for_relative(relative).unwrap_or_default();
            let sibling = source_path
                .parent()
                .map(|parent| parent.join(format!("{}.toml", file_stem_of(&file_name))))
                .filter(|sibling| sibling.is_file());
            if sibling.is_some() {
                return Err(RototoError::new(format!(
                    "package both provides variable {id} and declares an update for it"
                )));
            }
            let entry_path = target_dir.join(format!("{}.toml", file_stem_of(&file_name)));
            if !entry_path.is_file() {
                return Err(RototoError::new(format!(
                    "variable update has no base variable to update in the base packages: {}",
                    relative.display()
                )));
            }
            let update = read_layer_toml(source_path)?;
            let Some(update_table) = update.as_table() else {
                return Err(RototoError::new(format!(
                    "variable update is not a TOML table: {}",
                    relative.display()
                )));
            };
            for key in update_table.keys() {
                if key != "resolve" && key != "description" {
                    return Err(RototoError::new(format!(
                        "a variable update may only update [resolve] and description; {} \
                         declares {key}, which stays with the base",
                        relative.display()
                    )));
                }
            }
            let replaces_resolve = update_table.contains_key("resolve");
            let mut base = read_layer_toml(&entry_path)?;
            if let Some(base_table) = base.as_table_mut() {
                for (key, value) in update_table {
                    base_table.insert(key.clone(), value.clone());
                }
            }
            write_layer_toml(&entry_path, &base)?;
            if replaces_resolve && !id.is_empty() {
                provenance.record(&id);
            }
            Ok(())
        }
        LayerFileComposition::VariableRestate => {
            if file_identical(source_path, &target_path) {
                return Ok(());
            }
            let id = variable_id_for_relative(relative).unwrap_or_default();
            Err(variable_restate_denied(&id))
        }
        LayerFileComposition::ListUpdate => {
            // The marker file is lists/<id>.update.toml; it composes into
            // the base's lists/<id>.toml and never lands in the projection.
            let base_path = target_path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".update.toml"))
                .zip(target_path.parent())
                .map(|(stem, parent)| parent.join(format!("{stem}.toml")))
                .unwrap_or_default();
            if !base_path.is_file() {
                return Err(RototoError::new(format!(
                    "list update has no base list to update: {}",
                    relative.display()
                )));
            }
            let overlay = read_layer_toml(source_path)?;
            if let Some(table) = overlay.as_table() {
                for key in table.keys() {
                    if !matches!(key.as_str(), "members" | "deleted" | "description") {
                        return Err(RototoError::new(format!(
                            "a list update may only update members, deleted, and \
                             description; {} sets `{key}`",
                            relative.display()
                        )));
                    }
                }
            }
            let mut base = read_layer_toml(&base_path)?;
            if let (Some(base_table), Some(overlay_table)) =
                (base.as_table_mut(), overlay.as_table())
                && let Some(description) = overlay_table.get("description")
            {
                base_table.insert("description".to_owned(), description.clone());
            }
            compose_list_members(&mut base, overlay, relative)?;
            write_layer_toml(&base_path, &base)
        }
        LayerFileComposition::ListRestate => {
            if file_identical(source_path, &target_path) {
                return Ok(());
            }
            let id = relative
                .strip_prefix("lists")
                .ok()
                .and_then(|rest| rest.to_str())
                .and_then(|rest| rest.strip_suffix(".toml"))
                .unwrap_or_default()
                .replace(std::path::MAIN_SEPARATOR, "/");
            Err(RototoError::new(format!(
                "list {id} is declared in the base packages; update it with \
                 lists/{id}.update.toml instead of restating the file"
            )))
        }
    }
}

/// A layer that both provides `<entry>.toml` and deletes or updates the
/// same entry is contradicting itself; composition targets the layers below.
pub(super) fn reject_same_layer_entry(
    source_path: &Path,
    entry: &str,
    operation: &str,
) -> Result<()> {
    // The sibling lives next to the marker file, so it is found by the
    // local file stem; `entry` may be namespaced and is for the message.
    let local_stem = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| {
            name.strip_suffix(".deleted.toml")
                .or_else(|| name.strip_suffix(".update.toml"))
        });
    let sibling = local_stem
        .zip(source_path.parent())
        .map(|(stem, parent)| parent.join(format!("{stem}.toml")))
        .filter(|sibling| sibling.is_file());
    if sibling.is_some() {
        return Err(RototoError::new(format!(
            "package both provides catalog entry {entry} and declares {operation} for it"
        )));
    }
    Ok(())
}

pub(super) fn read_layer_toml(path: &Path) -> Result<toml::Value> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        RototoError::new(format!(
            "failed to read package source file {}: {err}",
            path.display()
        ))
    })?;
    text.parse::<toml::Value>().map_err(|err| {
        RototoError::new(format!(
            "failed to parse package source file {}: {err}",
            path.display()
        ))
    })
}

pub(super) fn write_layer_toml(path: &Path, value: &toml::Value) -> Result<()> {
    let text = toml::to_string_pretty(value)
        .map_err(|err| RototoError::new(format!("failed to serialize composed file: {err}")))?;
    std::fs::write(path, text).map_err(|err| {
        RototoError::new(format!(
            "failed to write composed package file {}: {err}",
            path.display()
        ))
    })
}

/// Deep merge for catalog entry updates: tables merge recursively, everything
/// else (scalars, arrays) replaces, and fields the update does not mention are
/// inherited.
pub(super) fn deep_merge_toml(base: &mut toml::Value, update: toml::Value) {
    match (base, update) {
        (toml::Value::Table(base), toml::Value::Table(update)) => {
            for (key, value) in update {
                match base.get_mut(&key) {
                    Some(existing) => deep_merge_toml(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, update) => *base = update,
    }
}

/// Compose an overlay's list member file into the base's. Members are a set:
/// the overlay's `members` union in (base first, duplicates collapse) and its
/// `deleted` values are removed from the result. Every deleted value has to
/// name a member a layer below actually provides, a layer may not both add
/// and delete the same value, and the composed set may not end up empty. The
/// `deleted` key itself never lands in the flattened file.
pub(super) fn compose_list_members(
    base: &mut toml::Value,
    overlay: toml::Value,
    relative: &Path,
) -> Result<()> {
    let (Some(base_table), Some(overlay_table)) = (base.as_table_mut(), overlay.as_table()) else {
        return Ok(());
    };
    let overlay_members = overlay_table
        .get("members")
        .and_then(|item| item.as_array());
    let deleted = match overlay_table.get("deleted") {
        None => None,
        Some(toml::Value::Array(values)) => Some(values),
        Some(_) => {
            return Err(RototoError::new(format!(
                "deleted list members must be an array: {}",
                relative.display()
            )));
        }
    };
    let Some(toml::Value::Array(base_members)) = base_table.get_mut("members") else {
        if deleted.is_some() {
            return Err(RototoError::new(format!(
                "deleted list members have no member set to remove in the base packages: {}",
                relative.display()
            )));
        }
        if let Some(members) = overlay_table.get("members") {
            base_table.insert("members".to_owned(), members.clone());
        }
        return Ok(());
    };
    if let Some(overlay_members) = overlay_members {
        for member in overlay_members {
            if !base_members.contains(member) {
                base_members.push(member.clone());
            }
        }
    }
    if let Some(deleted) = deleted {
        for value in deleted {
            if overlay_members.is_some_and(|members| members.contains(value)) {
                return Err(RototoError::new(format!(
                    "package both adds list member {value} and deletes it: {}",
                    relative.display()
                )));
            }
            let Some(position) = base_members.iter().position(|member| member == value) else {
                return Err(RototoError::new(format!(
                    "deleted list member is not in the base packages: {value} ({})",
                    relative.display()
                )));
            };
            base_members.remove(position);
        }
        if base_members.is_empty() {
            return Err(RototoError::new(format!(
                "deleting these members leaves the list with no members: {}",
                relative.display()
            )));
        }
    }
    Ok(())
}

/// Merge an overlay variable file over the base's: every top-level key the
/// overlay declares replaces the base's key whole, so `[resolve]` swaps
/// atomically and the type (and anything else left out) stays with the base.
/// A plain variable file may never restate a base variable: the update
/// marker is the only spelling for changing one, so a reviewer can tell an
/// add from an update by the file name alone.
pub(super) fn variable_restate_denied(id: &str) -> RototoError {
    RototoError::new(format!(
        "variable {id} is declared in the base packages; update it with \
         variables/{id}.update.toml instead of restating the file"
    ))
}

/// The file stem with a trailing `.update` marker suffix removed, so
/// `active_plan.update.toml` names the variable `active_plan`.
pub(super) fn file_stem_of(file_name: &str) -> &str {
    let stem = file_name.strip_suffix(".toml").unwrap_or(file_name);
    stem.strip_suffix(".update").unwrap_or(stem)
}
