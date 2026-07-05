use std::path::{Path, PathBuf};

use crate::error::{Result, RototoError};

use super::{DocumentCollection, DocumentKind, SourceStore};

impl SourceStore {
    pub(crate) async fn add_named_toml_documents(
        &mut self,
        directory: &str,
        collection: DocumentCollection,
    ) -> Result<()> {
        let directory_path = self.root.join(directory);
        let entries = match sorted_directory_entries_recursive(&directory_path).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    directory_path.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }
            // Namespaced ids are directories: variables/acme/in_trial.toml is
            // the variable acme/in_trial.
            let Ok(relative) = path.strip_prefix(&directory_path) else {
                continue;
            };
            let Some(id) = relative
                .with_extension("")
                .to_str()
                .map(|id| id.replace(std::path::MAIN_SEPARATOR, "/"))
            else {
                continue;
            };
            let relative_path = PathBuf::from(directory).join(relative);
            let kind = match collection {
                DocumentCollection::Variables => DocumentKind::Variable { id },
            };
            self.add_disk_document(relative_path, kind).await;
        }

        Ok(())
    }

    pub(crate) async fn add_enum_documents(&mut self) -> Result<()> {
        for (directory, declaration) in [("model/enums", true), ("data/enums", false)] {
            let directory_path = self.root.join(directory);
            let entries = match sorted_directory_entries_recursive(&directory_path).await {
                Ok(entries) => entries,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(RototoError::new(format!(
                        "failed to read {}: {err}",
                        directory_path.display()
                    )));
                }
            };
            for path in entries {
                let Some(id) = namespaced_id(&directory_path, &path, ".toml") else {
                    continue;
                };
                let relative_path =
                    PathBuf::from(directory).join(path.strip_prefix(&directory_path).unwrap());
                let kind = if declaration {
                    DocumentKind::EnumDeclaration { id }
                } else {
                    DocumentKind::EnumMembers { id }
                };
                self.add_disk_document(relative_path, kind).await;
            }
        }
        Ok(())
    }

    pub(crate) async fn add_layer_documents(&mut self) -> Result<()> {
        let directory_path = self.root.join("layers");
        let entries = match sorted_directory_entries_recursive(&directory_path).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    directory_path.display()
                )));
            }
        };
        for path in entries {
            let Some(id) = namespaced_id(&directory_path, &path, ".toml") else {
                continue;
            };
            let relative_path =
                PathBuf::from("layers").join(path.strip_prefix(&directory_path).unwrap());
            self.add_disk_document(relative_path, DocumentKind::Layer { id })
                .await;
        }
        Ok(())
    }

    pub(crate) async fn add_catalog_documents(&mut self) -> Result<()> {
        let directory = self.root.join("model/catalogs");
        let entries = match sorted_directory_entries_recursive(&directory).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    directory.display()
                )));
            }
        };

        // Two phases: every schema first, then entries. Entry namespacing
        // needs the full catalog id set, so that catalog `a` does not claim
        // the subtree that belongs to catalog `a/b`.
        let mut ids = Vec::new();
        for path in entries {
            let Some(id) = namespaced_id(&directory, &path, ".schema.json") else {
                continue;
            };
            let relative_path =
                PathBuf::from("model/catalogs").join(path.strip_prefix(&directory).unwrap());
            self.add_disk_document(relative_path, DocumentKind::Catalog { id: id.clone() })
                .await;
            ids.push(id);
        }
        for id in &ids {
            self.add_catalog_entry_documents(id).await?;
        }

        Ok(())
    }

    /// Every catalog id currently discovered, for subtree ownership checks.
    fn known_catalog_ids(&self) -> Vec<String> {
        self.documents
            .values()
            .filter_map(|document| match &document.kind {
                DocumentKind::Catalog { id } => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    pub(crate) async fn add_catalog_entry_documents(&mut self, catalog_id: &str) -> Result<()> {
        // Entries namespace recursively, but a subtree that is itself a
        // longer catalog id belongs to that catalog: catalog `a` never
        // claims files under data/catalogs/a/b/ when `a/b` is a catalog.
        let owned_by_longer = |entry_id: &str, known: &[String]| {
            known.iter().any(|other| {
                other
                    .strip_prefix(catalog_id)
                    .and_then(|rest| rest.strip_prefix('/'))
                    .is_some_and(|suffix| {
                        entry_id
                            .strip_prefix(suffix)
                            .is_some_and(|rest| rest.starts_with('/'))
                    })
            })
        };
        let known = self.known_catalog_ids();

        let entries_dir = self.root.join("data/catalogs").join(catalog_id);
        let entries = match sorted_directory_entries_recursive(&entries_dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    entries_dir.display()
                )));
            }
        };

        for path in entries {
            let Some(entry_id) = namespaced_id(&entries_dir, &path, ".toml") else {
                continue;
            };
            if owned_by_longer(&entry_id, &known) {
                continue;
            }
            let relative_path = PathBuf::from("data/catalogs")
                .join(catalog_id)
                .join(path.strip_prefix(&entries_dir).unwrap());
            self.add_disk_document(
                relative_path,
                DocumentKind::CatalogEntry {
                    catalog_id: catalog_id.to_owned(),
                    entry_id,
                },
            )
            .await;
        }

        let overlay_prefix = format!("data/catalogs/{catalog_id}/");
        let overlay_paths = self
            .overlays
            .keys()
            .filter(|path| path.starts_with(&overlay_prefix) && path.ends_with(".toml"))
            .cloned()
            .collect::<Vec<_>>();
        for path in overlay_paths {
            let Some(entry_id) = overlay_entry_id(&path, &overlay_prefix, ".toml") else {
                continue;
            };
            if owned_by_longer(&entry_id, &known) {
                continue;
            }
            self.add_disk_document(
                PathBuf::from(&path),
                DocumentKind::CatalogEntry {
                    catalog_id: catalog_id.to_owned(),
                    entry_id,
                },
            )
            .await;
        }

        Ok(())
    }

    pub(crate) async fn add_evaluation_context_documents(&mut self) -> Result<()> {
        let directory = self.root.join("model/context");
        let entries = match sorted_directory_entries_recursive(&directory).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    directory.display()
                )));
            }
        };

        for path in entries {
            // Files inside a `<id>-samples/` directory are samples, not
            // schemas, whatever their name.
            let in_samples_dir = path
                .strip_prefix(&directory)
                .ok()
                .and_then(|relative| relative.parent())
                .is_some_and(|parent| {
                    parent
                        .iter()
                        .filter_map(|component| component.to_str())
                        .any(|component| component.ends_with("-samples"))
                });
            if in_samples_dir {
                continue;
            }
            let Some(id) = namespaced_id(&directory, &path, ".schema.json") else {
                continue;
            };
            let relative_path =
                PathBuf::from("model/context").join(path.strip_prefix(&directory).unwrap());
            self.add_disk_document(
                relative_path,
                DocumentKind::EvaluationContext { id: id.clone() },
            )
            .await;
            self.add_evaluation_context_sample_documents(&id).await?;
        }

        Ok(())
    }

    pub(crate) async fn add_evaluation_context_sample_documents(
        &mut self,
        evaluation_context_id: &str,
    ) -> Result<()> {
        let samples_dir = self
            .root
            .join("model/context")
            .join(format!("{evaluation_context_id}-samples"));
        let samples = match sorted_directory_entries_recursive(&samples_dir).await {
            Ok(samples) => samples,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    samples_dir.display()
                )));
            }
        };

        for path in samples {
            let Some(sample_id) = namespaced_id(&samples_dir, &path, ".json") else {
                continue;
            };
            let relative_path = PathBuf::from("model/context")
                .join(format!("{evaluation_context_id}-samples"))
                .join(path.strip_prefix(&samples_dir).unwrap());
            self.add_disk_document(
                relative_path,
                DocumentKind::EvaluationContextSample {
                    evaluation_context_id: evaluation_context_id.to_owned(),
                    sample_id: sample_id.to_owned(),
                },
            )
            .await;
        }

        let overlay_prefix = format!("model/context/{evaluation_context_id}-samples/");
        let overlay_paths = self
            .overlays
            .keys()
            .filter(|path| path.starts_with(&overlay_prefix) && path.ends_with(".json"))
            .cloned()
            .collect::<Vec<_>>();
        for path in overlay_paths {
            let Some(sample_id) = overlay_entry_id(&path, &overlay_prefix, ".json") else {
                continue;
            };
            self.add_disk_document(
                PathBuf::from(&path),
                DocumentKind::EvaluationContextSample {
                    evaluation_context_id: evaluation_context_id.to_owned(),
                    sample_id,
                },
            )
            .await;
        }

        Ok(())
    }

    pub(crate) async fn add_custom_lint_documents(&mut self) -> Result<()> {
        let lint = self.root.join("lint");
        let entries = match sorted_directory_entries(&lint).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    lint.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("lua") {
                continue;
            }
            let relative_path =
                PathBuf::from("lint").join(path.file_name().expect("entry has filename"));
            self.add_disk_document(relative_path, DocumentKind::CustomLint)
                .await;
        }

        Ok(())
    }

    pub(crate) async fn add_overlay_documents(&mut self) -> Result<()> {
        let mut paths = self.overlays.keys().cloned().collect::<Vec<_>>();
        // Schemas declare the ids that own entry and sample subtrees, so
        // they go first; the owner's scan then claims its files before the
        // generic arms guess at them.
        paths.sort_by_key(|path| {
            let is_schema =
                path.starts_with("model/catalogs/") || path.starts_with("model/context/");
            (!is_schema, path.clone())
        });
        for path in paths {
            if self.document_by_path(&path).is_some() {
                continue;
            }
            let Some(kind) = overlay_document_kind(&path) else {
                continue;
            };
            let catalog_id = match &kind {
                DocumentKind::Catalog { id } => Some(id.clone()),
                _ => None,
            };
            let evaluation_context_id = match &kind {
                DocumentKind::EvaluationContext { id } => Some(id.clone()),
                _ => None,
            };
            self.add_disk_document(PathBuf::from(&path), kind).await;
            if let Some(catalog_id) = catalog_id {
                self.add_catalog_entry_documents(&catalog_id).await?;
            }
            if let Some(evaluation_context_id) = evaluation_context_id {
                self.add_evaluation_context_sample_documents(&evaluation_context_id)
                    .await?;
            }
        }
        Ok(())
    }
}

/// The namespaced id a file under a collection directory names: the path
/// relative to the collection root, separators normalized to `/`, with the
/// given suffix stripped. Directories are namespaces for every rototo
/// collection, so `model/enums/acme/tier.toml` is the enum `acme/tier`.
fn namespaced_id(base: &Path, path: &Path, suffix: &str) -> Option<String> {
    let relative = path.strip_prefix(base).ok()?;
    let relative = relative.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");
    let id = relative.strip_suffix(suffix)?;
    (!id.is_empty() && !id.ends_with('/')).then(|| id.to_owned())
}

fn overlay_document_kind(path: &str) -> Option<DocumentKind> {
    if path == "rototo-package.toml" {
        return Some(DocumentKind::Manifest);
    }
    let joined = |parts: &[&str], suffix: &str| -> Option<String> {
        let id = parts.join("/").strip_suffix(suffix)?.to_owned();
        (!id.is_empty() && !id.ends_with('/')).then_some(id)
    };
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        // Update and deleted markers are consumed at flatten time; an
        // unsaved marker is not a lintable document on its own.
        ["variables", rest @ .., file]
            if file.ends_with(".toml")
                && !file.ends_with(".update.toml")
                && !file.ends_with(".deleted.toml") =>
        {
            let _ = rest;
            joined(&parts[1..], ".toml").map(|id| DocumentKind::Variable { id })
        }
        ["model", "enums", .., file] if file.ends_with(".toml") => {
            joined(&parts[2..], ".toml").map(|id| DocumentKind::EnumDeclaration { id })
        }
        ["data", "enums", .., file] if file.ends_with(".toml") => {
            joined(&parts[2..], ".toml").map(|id| DocumentKind::EnumMembers { id })
        }
        ["model", "catalogs", .., file] if file.ends_with(".schema.json") => {
            joined(&parts[2..], ".schema.json").map(|id| DocumentKind::Catalog { id })
        }
        ["data", "catalogs", middle @ .., file]
            if !middle.is_empty()
                && file.ends_with(".toml")
                && !file.ends_with(".update.toml")
                && !file.ends_with(".deleted.toml") =>
        {
            let catalog_id = middle.join("/");
            let entry_id = file.strip_suffix(".toml")?;
            (!entry_id.is_empty()).then(|| DocumentKind::CatalogEntry {
                catalog_id,
                entry_id: entry_id.to_owned(),
            })
        }
        ["model", "context", rest @ ..]
            if parts.len() > 3
                && rest.last().is_some_and(|file| file.ends_with(".json"))
                && rest.iter().any(|part| part.ends_with("-samples")) =>
        {
            let samples_at = rest
                .iter()
                .position(|part| part.ends_with("-samples"))
                .expect("checked above");
            let evaluation_context_id = joined(&rest[..=samples_at], "-samples")?;
            let sample_id = joined(&rest[samples_at + 1..], ".json")?;
            Some(DocumentKind::EvaluationContextSample {
                evaluation_context_id,
                sample_id,
            })
        }
        ["model", "context", .., file] if file.ends_with(".schema.json") => {
            joined(&parts[2..], ".schema.json").map(|id| DocumentKind::EvaluationContext { id })
        }
        ["layers", .., file] if file.ends_with(".toml") => {
            joined(&parts[1..], ".toml").map(|id| DocumentKind::Layer { id })
        }
        ["lint", file] if file.ends_with(".lua") => Some(DocumentKind::CustomLint),
        _ => None,
    }
}

fn overlay_entry_id(path: &str, prefix: &str, suffix: &str) -> Option<String> {
    let file = path.strip_prefix(prefix)?;
    let key = file.strip_suffix(suffix)?;
    (!key.is_empty() && !key.ends_with('/')).then(|| key.to_owned())
}

async fn sorted_directory_entries(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(path).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_file()
            || (file_type.is_symlink()
                && tokio::fs::metadata(entry.path())
                    .await
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(true))
        {
            entries.push(entry.path());
        }
    }
    entries.sort();
    Ok(entries)
}

/// Like [`sorted_directory_entries`], but walking subdirectories too (for
/// collections whose ids namespace with `/`). Symlinked directories are not
/// followed.
async fn sorted_directory_entries_recursive(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut read_dir = tokio::fs::read_dir(&directory).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file()
                || (file_type.is_symlink()
                    && tokio::fs::metadata(entry.path())
                        .await
                        .map(|metadata| metadata.is_file())
                        .unwrap_or(true))
            {
                entries.push(entry.path());
            }
        }
    }
    entries.sort();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn sorted_directory_entries_skips_symlinked_directories() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let target_dir = root.join("looks-like-file.toml");
        tokio::fs::create_dir(&target_dir).await.unwrap();
        tokio::fs::write(root.join("real.toml"), "schema_version = 1")
            .await
            .unwrap();
        std::os::unix::fs::symlink(&target_dir, root.join("linked.toml")).unwrap();

        let entries = sorted_directory_entries(root).await.unwrap();
        let names = entries
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["real.toml"]);
    }
}
