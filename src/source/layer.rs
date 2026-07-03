use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tempfile::TempDir;

use crate::error::{Result, RototoError};
use crate::package::package_extends_sources;

use super::PACKAGE_MANIFEST;
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
        copy_package_layer(parent.staged().path(), &target, false).await?;
        immutable &= parent.immutable();
        layers.extend(parent.layers().iter().cloned());
    }

    copy_package_layer(loaded.staged().path(), &target, true).await?;
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

async fn copy_package_layer(source: &Path, target: &Path, include_manifest: bool) -> Result<()> {
    let source = source.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || {
        copy_package_layer_recursive(&source, &target, include_manifest, Path::new(""))
    })
    .await
    .map_err(|err| RototoError::new(format!("package layer copy task failed: {err}")))?
}

fn copy_package_layer_recursive(
    source: &Path,
    target: &Path,
    include_manifest: bool,
    relative: &Path,
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
            compose_package_layer_file(&source_path, target, &file_name, &relative_path)?;
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
    /// `data/catalogs/<id>/<entry>.tombstone.toml`: disable the entry a layer
    /// below provided. The tombstone itself never lands in the projection.
    CatalogEntryTombstone { entry: String },
    /// `data/catalogs/<id>/<entry>.patch.toml`: field-level override of the
    /// entry a layer below provided; unpatched fields are inherited.
    CatalogEntryPatch { entry: String },
    /// `variables/**.toml` over an existing file: top-level keys replace the
    /// base's (so an overlay `[resolve]` block replaces the whole resolution),
    /// keys the overlay does not declare are inherited.
    VariableMerge,
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
            if let Some(entry) = file_name.strip_suffix(".tombstone.toml") {
                return LayerFileComposition::CatalogEntryTombstone {
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
        _ => LayerFileComposition::Replace,
    }
}

fn compose_package_layer_file(
    source_path: &Path,
    target_dir: &Path,
    file_name: &std::ffi::OsStr,
    relative: &Path,
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
            Ok(())
        }
        LayerFileComposition::CatalogEntryTombstone { entry } => {
            reject_same_layer_entry(source_path, &entry, "tombstone")?;
            let entry_path = target_dir.join(format!("{entry}.toml"));
            if !entry_path.is_file() {
                return Err(RototoError::new(format!(
                    "tombstone has no catalog entry to disable in the layers below: {}",
                    relative.display()
                )));
            }
            std::fs::remove_file(&entry_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to remove tombstoned catalog entry {}: {err}",
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
            merge_variable_toml(&mut base, overlay);
            write_layer_toml(&target_path, &base)
        }
    }
}

/// A layer that both provides `<entry>.toml` and tombstones or patches the
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

/// Merge an overlay variable file over the base's: every top-level key the
/// overlay declares replaces the base's key whole, so `[resolve]` swaps
/// atomically and the type (and anything else left out) stays with the base.
fn merge_variable_toml(base: &mut toml::Value, overlay: toml::Value) {
    let (toml::Value::Table(base), toml::Value::Table(overlay)) = (base, overlay) else {
        return;
    };
    for (key, value) in overlay {
        base.insert(key, value);
    }
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

        copy_package_layer(&source, &target, false).await.unwrap();

        assert!(!target.join(PACKAGE_MANIFEST).exists());
        assert!(
            target
                .join("data/catalogs/config")
                .join(PACKAGE_MANIFEST)
                .is_file()
        );
    }
}
