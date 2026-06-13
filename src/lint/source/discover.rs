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
        let entries = match sorted_directory_entries(&directory_path).await {
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
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let relative_path =
                PathBuf::from(directory).join(path.file_name().expect("entry has filename"));
            let kind = match collection {
                DocumentCollection::Qualifiers => DocumentKind::Qualifier {
                    id: stem.to_owned(),
                },
                DocumentCollection::Variables => DocumentKind::Variable {
                    id: stem.to_owned(),
                },
                DocumentCollection::Catalogs => DocumentKind::Catalog {
                    id: stem.to_owned(),
                },
            };
            self.add_disk_document(relative_path, kind).await;
            if matches!(collection, DocumentCollection::Catalogs) {
                self.add_catalog_entry_documents(stem).await?;
            }
        }

        Ok(())
    }

    pub(crate) async fn add_catalog_entry_documents(&mut self, catalog_id: &str) -> Result<()> {
        let entries_dir = self
            .root
            .join("catalogs")
            .join(format!("{catalog_id}-entries"));
        let entries = match sorted_directory_entries(&entries_dir).await {
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
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }
            let Some(entry_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let relative_path = PathBuf::from("catalogs")
                .join(format!("{catalog_id}-entries"))
                .join(path.file_name().expect("entry has filename"));
            self.add_disk_document(
                relative_path,
                DocumentKind::CatalogEntry {
                    catalog_id: catalog_id.to_owned(),
                    entry_id: entry_id.to_owned(),
                },
            )
            .await;
        }

        let overlay_prefix = format!("catalogs/{catalog_id}-entries/");
        let overlay_paths = self
            .overlays
            .keys()
            .filter(|path| path.starts_with(&overlay_prefix) && path.ends_with(".toml"))
            .cloned()
            .collect::<Vec<_>>();
        for path in overlay_paths {
            let Some(entry_id) = overlay_entry_id(&path, &overlay_prefix) else {
                continue;
            };
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

    pub(crate) async fn add_schema_documents(&mut self) -> Result<()> {
        let schemas = self.root.join("schemas");
        let entries = match sorted_directory_entries(&schemas).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    schemas.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let relative_path =
                PathBuf::from("schemas").join(path.file_name().expect("entry has filename"));
            self.add_disk_document(relative_path, DocumentKind::Schema)
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
        let paths = self.overlays.keys().cloned().collect::<Vec<_>>();
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
            self.add_disk_document(PathBuf::from(&path), kind).await;
            if let Some(catalog_id) = catalog_id {
                self.add_catalog_entry_documents(&catalog_id).await?;
            }
        }
        Ok(())
    }
}

fn overlay_document_kind(path: &str) -> Option<DocumentKind> {
    if path == "rototo-workspace.toml" {
        return Some(DocumentKind::Manifest);
    }
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["qualifiers", file] if file.ends_with(".toml") => {
            let id = file.strip_suffix(".toml")?;
            (!id.is_empty()).then(|| DocumentKind::Qualifier { id: id.to_owned() })
        }
        ["variables", file] if file.ends_with(".toml") => {
            let id = file.strip_suffix(".toml")?;
            (!id.is_empty()).then(|| DocumentKind::Variable { id: id.to_owned() })
        }
        ["catalogs", file] if file.ends_with(".toml") => {
            let id = file.strip_suffix(".toml")?;
            (!id.is_empty()).then(|| DocumentKind::Catalog { id: id.to_owned() })
        }
        ["catalogs", dir, file] if dir.ends_with("-entries") && file.ends_with(".toml") => {
            let catalog_id = dir.strip_suffix("-entries")?;
            let entry_id = file.strip_suffix(".toml")?;
            (!catalog_id.is_empty() && !entry_id.is_empty()).then(|| DocumentKind::CatalogEntry {
                catalog_id: catalog_id.to_owned(),
                entry_id: entry_id.to_owned(),
            })
        }
        ["schemas", file] if file.ends_with(".json") => Some(DocumentKind::Schema),
        ["lint", file] if file.ends_with(".lua") => Some(DocumentKind::CustomLint),
        _ => None,
    }
}

fn overlay_entry_id(path: &str, prefix: &str) -> Option<String> {
    let file = path.strip_prefix(prefix)?;
    if file.contains('/') {
        return None;
    }
    let key = file.strip_suffix(".toml")?;
    (!key.is_empty()).then(|| key.to_owned())
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
