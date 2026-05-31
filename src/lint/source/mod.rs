mod discover;
mod line_index;
mod path;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;

use crate::diagnostics::{DiagnosticLocation, DocId, TextRange};
use crate::model::{SourceDocumentSummary, SourceKind};

use super::input::OverlayDocument;
use line_index::LineIndex;
use path::{file_uri, path_containment_error};

pub(super) use path::workspace_path;
pub(super) use path::{resolve_workspace_relative_path, resolve_workspace_root_path};

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
}

impl SourceStore {
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
        let text_range = TextRange::new(range.start, range.end);
        DiagnosticLocation::source_span(
            self.id,
            self.path.clone(),
            text_range,
            self.line_index.range(range),
        )
    }
}

#[derive(Clone, Copy)]
pub(super) enum DocumentCollection {
    Qualifiers,
    Variables,
}
