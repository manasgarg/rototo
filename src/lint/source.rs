use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use crate::diagnostics::{DiagnosticLocation, DocId, SourcePosition, SourceRange};
use crate::error::{Result, RototoError};
use crate::model::{SourceDocumentSummary, SourceKind};

use super::input::OverlayDocument;

pub(super) struct SourceStore {
    pub(super) root: PathBuf,
    pub(super) overlays: BTreeMap<String, OverlayDocument>,
    pub(super) documents: BTreeMap<DocId, SourceDocument>,
    by_path: BTreeMap<String, DocId>,
}

impl SourceStore {
    pub(super) fn new(root: PathBuf, overlays: BTreeMap<String, OverlayDocument>) -> Self {
        Self {
            root,
            overlays,
            documents: BTreeMap::new(),
            by_path: BTreeMap::new(),
        }
    }

    pub(super) async fn add_named_toml_documents(
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

    pub(super) async fn add_external_value_documents(&mut self, variable_id: &str) -> Result<()> {
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

    pub(super) async fn add_schema_documents(&mut self) -> Result<()> {
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

    pub(super) async fn add_custom_lint_documents(&mut self) -> Result<()> {
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

    pub(super) async fn add_disk_document(
        &mut self,
        relative_path: PathBuf,
        kind: DocumentKind,
    ) -> DocId {
        let path = workspace_path(&relative_path);
        if let Some(doc) = self.by_path.get(&path).copied() {
            return doc;
        }

        let id = DocId(self.documents.len() as u32);
        let absolute_path = self.root.join(&relative_path);
        let containment_error = path_containment_error(&self.root, &absolute_path).await;
        let (text, version, read_error) = if let Some(err) = containment_error {
            (String::new(), None, Some(err))
        } else {
            match self.overlays.get(&path) {
                Some(overlay) => (overlay.text.clone(), overlay.version, None),
                None => match tokio::fs::read_to_string(&absolute_path).await {
                    Ok(text) => (text, None, None),
                    Err(err) => (String::new(), None, Some(err.to_string())),
                },
            }
        };
        let document = SourceDocument {
            id,
            path: path.clone(),
            uri: file_uri(&absolute_path),
            version,
            kind,
            line_index: LineIndex::new(&text),
            text,
            read_error,
        };

        self.documents.insert(id, document);
        self.by_path.insert(path, id);
        id
    }

    pub(super) fn external_value_keys(&self, variable_id: &str) -> BTreeSet<String> {
        self.documents
            .values()
            .filter_map(|document| match &document.kind {
                DocumentKind::ExternalValue {
                    variable_id: document_variable_id,
                    value_key,
                } if document_variable_id == variable_id => Some(value_key.clone()),
                _ => None,
            })
            .collect()
    }

    pub(super) fn document_by_path(&self, path: &str) -> Option<&SourceDocument> {
        self.by_path
            .get(path)
            .and_then(|document_id| self.documents.get(document_id))
    }

    pub(super) fn document_summaries(&self) -> Vec<SourceDocumentSummary> {
        self.documents
            .values()
            .map(|document| SourceDocumentSummary {
                id: document.id,
                path: document.path.clone(),
                uri: document.uri.clone(),
                version: document.version,
                kind: document.kind.summary_kind(),
            })
            .collect()
    }
}

#[derive(Clone)]
pub(super) enum DocumentKind {
    Manifest,
    Qualifier {
        id: String,
    },
    Variable {
        id: String,
    },
    ExternalValue {
        variable_id: String,
        value_key: String,
    },
    Schema,
    CustomLint,
}

impl DocumentKind {
    fn summary_kind(&self) -> SourceKind {
        match self {
            Self::Manifest => SourceKind::Manifest,
            Self::Qualifier { .. } => SourceKind::Qualifier,
            Self::Variable { .. } => SourceKind::Variable,
            Self::ExternalValue { .. } => SourceKind::ExternalValue,
            Self::Schema => SourceKind::Schema,
            Self::CustomLint => SourceKind::CustomLint,
        }
    }
}

async fn path_containment_error(root: &Path, path: &Path) -> Option<String> {
    let root = tokio::fs::canonicalize(root).await.ok()?;
    let path = tokio::fs::canonicalize(path).await.ok()?;
    if path.starts_with(&root) {
        None
    } else {
        Some("path escapes workspace".to_owned())
    }
}

#[derive(Clone)]
pub(super) struct SourceDocument {
    pub(super) id: DocId,
    pub(super) path: String,
    pub(super) uri: String,
    pub(super) version: Option<i32>,
    pub(super) kind: DocumentKind,
    pub(super) text: String,
    pub(super) line_index: LineIndex,
    pub(super) read_error: Option<String>,
}

impl SourceDocument {
    pub(super) fn document_location(&self) -> DiagnosticLocation {
        DiagnosticLocation::document(self.id, self.path.clone())
    }

    pub(super) fn span_location(&self, range: Range<usize>) -> DiagnosticLocation {
        DiagnosticLocation::span(self.id, self.path.clone(), self.line_index.range(range))
    }
}

#[derive(Clone)]
pub(super) struct LineIndex {
    line_starts: Vec<usize>,
    text_len: usize,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self {
            line_starts,
            text_len: text.len(),
        }
    }

    fn range(&self, range: Range<usize>) -> SourceRange {
        let start = range.start.min(self.text_len);
        let end = range.end.min(self.text_len).max(start);
        SourceRange {
            start: self.position(start),
            end: self.position(end),
        }
    }

    fn position(&self, offset: usize) -> SourcePosition {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next_line) => next_line.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        SourcePosition {
            line,
            character: offset.saturating_sub(line_start),
        }
    }

    pub(super) fn offset_for_line_character(&self, line: usize, character: usize) -> usize {
        let line_start = self.line_starts.get(line).copied().unwrap_or(self.text_len);
        line_start.saturating_add(character).min(self.text_len)
    }
}

#[derive(Clone, Copy)]
pub(super) enum DocumentCollection {
    Qualifiers,
    Variables,
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

pub(super) fn workspace_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}
