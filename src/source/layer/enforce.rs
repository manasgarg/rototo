use super::*;

/// Enforce the layering contract carried by the projection built so far on
/// the layer about to land on it. The incoming layer is default-closed over
/// the entities the projection declares; the projection's governance.toml
/// (or its [defaults] block) is where grants come from, and no file means
/// no grants.
pub(super) async fn enforce_layer_governance(layer: &Path, target: &Path) -> Result<()> {
    let layer = layer.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || enforce_layer_governance_sync(&layer, &target))
        .await
        .map_err(|err| RototoError::new(format!("governance enforcement task failed: {err}")))?
}

pub(super) fn enforce_layer_governance_sync(layer: &Path, target: &Path) -> Result<()> {
    // Deny by default is unconditional: a projection without a
    // governance.toml yields the empty contract, which grants nothing.
    let contract = read_governance_contract(target);

    let mut pending = vec![layer.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read package source {}: {err}",
                    directory.display()
                )));
            }
        };
        let mut entries = entries
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|err| {
                RototoError::new(format!("failed to read package source entry: {err}"))
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let Ok(relative) = path.strip_prefix(layer) else {
                continue;
            };
            // Restating a file byte-identical to the projection's is never a
            // semantic change; it is how diamond ancestry looks when two
            // bases share an ancestor and both carry its files unchanged.
            if file_identical(&path, &target.join(relative)) {
                continue;
            }
            check_governed_file(&contract, &path, relative, target)?;
        }
    }
    Ok(())
}

pub(super) fn check_governed_file(
    contract: &crate::source::governance::GovernanceContract,
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
    // Directories namespace ids for every collection: the id is the joined
    // path below the collection root, suffix stripped.
    let namespaced = |parts: &[&str], suffix: &str| -> Option<String> {
        let id = parts.join("/").strip_suffix(suffix)?.to_owned();
        (!id.is_empty() && !id.ends_with('/')).then_some(id)
    };

    match components.as_slice() {
        // The manifest is the layer's own identity, and the governance file
        // itself is checked as a ceiling, not as an operation.
        [file] if *file == PACKAGE_MANIFEST => Ok(()),
        ["governance.toml"] => {
            let text = std::fs::read_to_string(source_path).map_err(|err| {
                RototoError::new(format!(
                    "failed to read package source file {}: {err}",
                    source_path.display()
                ))
            })?;
            let value = text.parse::<toml::Value>().map_err(|err| {
                RototoError::new(format!(
                    "failed to parse the overlay governance.toml: {err}"
                ))
            })?;
            let declared_below = |kind: &str, id: &str| -> bool {
                let path = match kind {
                    "catalog" => format!("model/catalogs/{id}.schema.json"),
                    "enum" => format!("model/enums/{id}.toml"),
                    "variable" => format!("variables/{id}.toml"),
                    "evaluation_context" => format!("model/context/{id}.schema.json"),
                    "layer" => format!("layers/{id}.toml"),
                    _ => return true,
                };
                target.join(path).is_file()
            };
            let any_declared_below = ["model", "variables", "data", "layers"]
                .iter()
                .any(|dir| target.join(dir).is_dir());
            contract.check_ceiling(
                &crate::source::governance::parse_contract_value(&value),
                &declared_below,
                any_declared_below,
            )
        }
        ["model", "catalogs", ..] => match namespaced(&components[2..], ".schema.json") {
            Some(id) if exists(relative) => Err(schema_edit_denied("catalog schema", &id)),
            _ => Ok(()),
        },
        ["model", "enums", ..] => match namespaced(&components[2..], ".toml") {
            Some(id) if exists(relative) => Err(schema_edit_denied("enum declaration", &id)),
            _ => Ok(()),
        },
        ["model", "context", .., samples, _] if samples.ends_with("-samples") => {
            match namespaced(&components[2..components.len() - 1], "-samples") {
                Some(id)
                    if exists(Path::new(&format!("model/context/{id}.schema.json")))
                        && exists(relative) =>
                {
                    Err(RototoError::new(format!(
                        "governance does not allow an overlay to change a base sample for \
                         evaluation context {id}; add a new sample file instead"
                    )))
                }
                _ => Ok(()),
            }
        }
        ["model", "context", ..] => match namespaced(&components[2..], ".schema.json") {
            Some(id) if exists(relative) => {
                Err(schema_edit_denied("evaluation context schema", &id))
            }
            _ => Ok(()),
        },
        ["data", "enums", ..] => match namespaced(&components[2..], ".toml") {
            Some(id) if exists(relative) => {
                contract.check("enum", &id, Operation::Update, None, &[])
            }
            Some(id) if exists(Path::new(&format!("model/enums/{id}.toml"))) => {
                contract.check("enum", &id, Operation::Add, None, &[])
            }
            _ => Ok(()),
        },
        ["data", "catalogs", middle @ .., file] if !middle.is_empty() => {
            // Entries namespace: data/catalogs/banner/promo/summer.toml can
            // be entry promo/summer of catalog banner. The catalog is the
            // longest directory prefix the base declares a schema for, so a
            // nested entry of a governed catalog cannot dodge policy by
            // looking like a deeper catalog.
            let Some((catalog, entry_namespace)) = (1..=middle.len()).rev().find_map(|len| {
                let candidate = middle[..len].join("/");
                exists(Path::new(&format!(
                    "model/catalogs/{candidate}.schema.json"
                )))
                .then(|| (candidate, middle[len..].join("/")))
            }) else {
                // A catalog this layer introduces is its own to fill.
                return Ok(());
            };
            let catalog = &catalog;
            let entry_name = |stem: String| -> String {
                if entry_namespace.is_empty() {
                    stem
                } else {
                    format!("{entry_namespace}/{stem}")
                }
            };
            if let Some(entry) = stem(file, ".deleted.toml").map(&entry_name) {
                reject_same_layer_entry(source_path, &entry, "a deleted marker")?;
                if let Some(local_stem) = stem(file, ".deleted.toml")
                    && source_path
                        .parent()
                        .map(|parent| parent.join(format!("{local_stem}.update.toml")))
                        .is_some_and(|sibling| sibling.is_file())
                {
                    return Err(RototoError::new(format!(
                        "package both declares an update and a deleted marker for catalog \
                         entry {entry}; updating and removing it are contradictory"
                    )));
                }
                return contract.check("catalog", catalog, Operation::Delete, Some(&entry), &[]);
            }
            if let Some(entry) = stem(file, ".update.toml").map(&entry_name) {
                reject_same_layer_entry(source_path, &entry, "an update")?;
                let fields = toml_top_level_keys(source_path)?;
                return contract.check(
                    "catalog",
                    catalog,
                    Operation::Update,
                    Some(&entry),
                    &fields,
                );
            }
            match stem(file, ".toml").map(&entry_name) {
                Some(entry) if exists(relative) => Err(RototoError::new(format!(
                    "governance does not model replacing catalog entry {entry} wholesale;                      use {entry}.update.toml to update fields or {entry}.deleted.toml to                      remove it"
                ))),
                Some(_) => contract.check("catalog", catalog, Operation::Add, None, &[]),
                None => Ok(()),
            }
        }
        ["variables", .., file] => {
            let Some(id) = variable_id_for_relative(relative) else {
                return Ok(());
            };
            if file.ends_with(".update.toml") {
                let base_file = relative
                    .parent()
                    .map(|parent| parent.join(format!("{}.toml", file_stem_of(file))))
                    .unwrap_or_default();
                if exists(&base_file) {
                    return contract.check("variable", &id, Operation::Update, None, &[]);
                }
                // An orphan marker is a structural error the compose step
                // reports; governance has nothing to say about it.
                return Ok(());
            }
            if exists(relative) {
                return Err(variable_restate_denied(&id));
            }
            Ok(())
        }
        ["layers", ..] => match namespaced(&components[1..], ".toml") {
            Some(id) if exists(relative) => {
                contract.check("layer", &id, Operation::Update, None, &[])
            }
            _ => Ok(()),
        },
        ["lint", file] if exists(relative) => Err(RototoError::new(format!(
            "governance does not model replacing a lint file the base owns: lint/{file}"
        ))),
        _ => Ok(()),
    }
}

/// A governed base's `model/` files are not editable from above at all: an
/// overlay narrows a base contract with a custom lint rule under `lint/`,
/// never by changing the schema file.
pub(super) fn schema_edit_denied(what: &str, id: &str) -> RototoError {
    RototoError::new(format!(
        "governance does not allow an overlay to change a base {what} {id}; narrow the \
         contract with a custom lint rule under lint/ instead"
    ))
}

/// The top-level keys of an update file, which are the fields it updates.
pub(super) fn toml_top_level_keys(path: &Path) -> Result<Vec<String>> {
    let value = read_layer_toml(path)?;
    Ok(value
        .as_table()
        .map(|table| table.keys().cloned().collect())
        .unwrap_or_default())
}
