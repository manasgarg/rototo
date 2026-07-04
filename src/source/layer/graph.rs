use super::*;

pub(super) const MAX_PACKAGE_EXTENDS_DEPTH: usize = 32;

pub(crate) fn load_package_source_graph<'a>(
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

pub(super) async fn project_package_source_graph(
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
    let sibling_entities = std::sync::Arc::new(std::sync::Mutex::new(BTreeMap::new()));
    for parent_source in &extends {
        let parent =
            load_package_source_graph(parent_source, options, local_mode, Some(base), stack)
                .await?;
        // No governance enforcement between siblings: none of the bases is
        // an overlay of another, each already enforced its own extends
        // chain while being projected, and a base touching another base's
        // entity is a sibling conflict, not a governed operation.
        copy_package_layer(
            parent.staged().path(),
            &target,
            false,
            layer_label(&parent, parent_source),
            Some(sibling_entities.clone()),
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
    copy_package_layer(loaded.staged().path(), &target, true, child_label, None).await?;
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

/// The label a layer's resolve contributions are recorded under: the source
/// string the author wrote in `extends`, or the layer's own identity.
pub(super) fn layer_label(loaded: &LoadedPackageSource, written_source: &str) -> String {
    if !written_source.trim().is_empty() {
        return written_source.to_owned();
    }
    loaded
        .layers()
        .last()
        .map(|layer| layer.source().to_owned())
        .unwrap_or_else(|| "package".to_owned())
}

pub(super) async fn read_package_extends(root: &Path) -> Result<Vec<String>> {
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

pub(super) fn resolve_extend_source(
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

pub(super) async fn package_source_key(source: &str, staged: &StagedPackage) -> Result<String> {
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

pub(super) fn extend_source_base_path(loaded: &LoadedPackageSource) -> PathBuf {
    if loaded.staged().is_temporary()
        && let [layer] = loaded.layers()
        && SourceUri::parse(layer.source()).ok().flatten().is_none()
    {
        return PathBuf::from(layer.source());
    }
    loaded.staged().path().to_path_buf()
}

pub(super) fn combined_layer_fingerprint(layers: &[SourceLayer]) -> Option<SourceFingerprint> {
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

pub(super) fn package_source_uri_is_local_filesystem(uri: &SourceUri) -> bool {
    matches!(uri.scheme.as_str(), "file" | "git+file")
}
