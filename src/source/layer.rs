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
        copy_package_layer_recursive(&source, &target, include_manifest, true)
    })
    .await
    .map_err(|err| RototoError::new(format!("package layer copy task failed: {err}")))?
}

fn copy_package_layer_recursive(
    source: &Path,
    target: &Path,
    include_manifest: bool,
    root: bool,
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
    for entry in std::fs::read_dir(source).map_err(|err| {
        RototoError::new(format!(
            "failed to read package layer {}: {err}",
            source.display()
        ))
    })? {
        let entry = entry.map_err(|err| {
            RototoError::new(format!("failed to read package layer entry: {err}"))
        })?;
        let file_name = entry.file_name();
        if root && !include_manifest && file_name == PACKAGE_MANIFEST {
            continue;
        }
        let source_path = entry.path();
        let target_path = target.join(&file_name);
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
            copy_package_layer_recursive(&source_path, &target_path, include_manifest, false)?;
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
            std::fs::copy(&source_path, &target_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to copy package layer entry {}: {err}",
                    source_path.display()
                ))
            })?;
        } else {
            return Err(RototoError::new(format!(
                "package layer contains unsupported entry type: {}",
                source_path.display()
            )));
        }
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
