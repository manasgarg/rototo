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
            };
            self.add_disk_document(relative_path, kind).await;
            if matches!(collection, DocumentCollection::Variables) {
                self.add_external_value_documents(stem).await?;
            }
        }

        Ok(())
    }

    pub(crate) async fn add_external_value_documents(&mut self, variable_id: &str) -> Result<()> {
        let values_dir = self
            .root
            .join("variables")
            .join(format!("{variable_id}-values"));
        let entries = match sorted_directory_entries(&values_dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    values_dir.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }
            let Some(value_key) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let relative_path = PathBuf::from("variables")
                .join(format!("{variable_id}-values"))
                .join(path.file_name().expect("entry has filename"));
            self.add_disk_document(
                relative_path,
                DocumentKind::ExternalValue {
                    variable_id: variable_id.to_owned(),
                    value_key: value_key.to_owned(),
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
}

async fn sorted_directory_entries(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(path).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_file() || file_type.is_symlink() {
            entries.push(entry.path());
        }
    }
    entries.sort();
    Ok(entries)
}
