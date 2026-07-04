use super::*;

/// The sidecar the flatten leaves in the projection: which layer's
/// `[resolve]` block each variable ended up carrying. The resolution trace
/// reads it back as provenance.
pub(crate) const RESOLVE_PROVENANCE_FILE: &str = ".rototo-provenance.json";

pub(super) struct LayerProvenance<'a> {
    pub(super) label: &'a str,
    pub(super) nested: &'a BTreeMap<String, String>,
    pub(super) recorded: std::cell::RefCell<&'a mut BTreeMap<String, String>>,
}

impl LayerProvenance<'_> {
    pub(super) fn record(&self, variable_id: &str) {
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

/// While the bases of one `extends` list land, tracks which base provided
/// each entity in the projection. The bases are siblings: none of them was
/// authored as an overlay of another, so a later base touching an entity an
/// earlier base declared is a conflict, not a composition. The one exception
/// is a byte-identical restatement of the same file - that is how diamond
/// ancestry looks when two bases share an ancestor and both carry its files
/// unchanged.
pub(super) struct SiblingBases<'a> {
    pub(super) label: &'a str,
    pub(super) provided: &'a std::sync::Mutex<BTreeMap<String, String>>,
}

impl SiblingBases<'_> {
    /// Admit one file landing. Returns `Ok(true)` when the landing should be
    /// skipped (an identical restatement of another sibling's file); errors
    /// when it would silently merge over or mutate another sibling's entity.
    pub(super) fn admit(
        &self,
        relative: &Path,
        source_path: &Path,
        projected: &Path,
    ) -> Result<bool> {
        let key = sibling_entity_key(relative);
        let mut provided = self.provided.lock().expect("sibling entity map poisoned");
        match provided.get(&key) {
            Some(owner) if owner != self.label => {
                if projected.is_file() && file_identical(source_path, projected) {
                    return Ok(true);
                }
                Err(RototoError::new(format!(
                    "package extends bases conflict on {key}: {} and {} both provide it; make one base extend the other or move the shared piece into one package",
                    owner, self.label
                )))
            }
            _ => {
                provided.insert(key, self.label.to_owned());
                Ok(false)
            }
        }
    }
}

/// The entity one projected file belongs to, for sibling-base conflict
/// detection: every file of an enum (declaration, members), evaluation
/// context (schema, samples), variable, or layer maps to that entity's key.
/// Catalogs are finer-grained so siblings can share one additively: the
/// schema is one key, and each entry (with its markers) is its own key, so
/// two bases adding distinct entries to a shared ancestor's catalog compose,
/// while touching the same entry or the schema still conflicts. Files outside
/// the entity model conflict per path.
pub(super) fn sibling_entity_key(relative: &Path) -> String {
    let components: Vec<&str> = relative
        .iter()
        .filter_map(|component| component.to_str())
        .collect();
    let stem = |file: &str, suffix: &str| file.strip_suffix(suffix).map(str::to_owned);
    let namespaced = |parts: &[&str], suffix: &str| -> Option<String> {
        let id = parts.join("/").strip_suffix(suffix)?.to_owned();
        (!id.is_empty() && !id.ends_with('/')).then_some(id)
    };
    match components.as_slice() {
        ["model", "catalogs", ..] => {
            namespaced(&components[2..], ".schema.json").map(|id| format!("catalog {id} schema"))
        }
        ["data", "catalogs", middle @ .., file] if !middle.is_empty() => {
            let catalog = middle.join("/");
            stem(file, ".deleted.toml")
                .or_else(|| stem(file, ".update.toml"))
                .or_else(|| stem(file, ".toml"))
                .map(|entry| format!("catalog {catalog} entry {entry}"))
        }
        ["model", "enums", ..] => {
            namespaced(&components[2..], ".toml").map(|id| format!("enum {id}"))
        }
        ["data", "enums", ..] => {
            namespaced(&components[2..], ".toml").map(|id| format!("enum {id}"))
        }
        ["model", "context", .., samples, sample_file] if samples.ends_with("-samples") => {
            namespaced(&components[2..components.len() - 1], "-samples").map(|id| {
                // Samples are additive the way catalog entries are: two
                // bases can each add their own samples for a shared
                // ancestor's context, so each sample file is its own key.
                format!("evaluation context {id} sample {sample_file}")
            })
        }
        ["model", "context", ..] => namespaced(&components[2..], ".schema.json")
            .map(|id| format!("evaluation context {id}")),
        ["layers", ..] => namespaced(&components[1..], ".toml").map(|id| format!("layer {id}")),
        ["variables", ..] => variable_id_for_relative(relative).map(|id| format!("variable {id}")),
        _ => None,
    }
    .unwrap_or_else(|| format!("file {}", relative.display()))
}

/// Whether two paths hold byte-identical files. Missing files are never
/// identical to anything.
pub(super) fn file_identical(left: &Path, right: &Path) -> bool {
    match (std::fs::read(left), std::fs::read(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
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

pub(super) fn read_provenance(root: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(root.join(RESOLVE_PROVENANCE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(super) fn write_provenance(root: &Path, provenance: &BTreeMap<String, String>) -> Result<()> {
    let text = serde_json::to_string_pretty(provenance)
        .map_err(|err| RototoError::new(format!("failed to serialize provenance: {err}")))?;
    std::fs::write(root.join(RESOLVE_PROVENANCE_FILE), text).map_err(|err| {
        RototoError::new(format!(
            "failed to write provenance sidecar in {}: {err}",
            root.display()
        ))
    })
}
