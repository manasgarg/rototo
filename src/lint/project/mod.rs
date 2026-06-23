mod catalog;
mod fields;
mod manifest;
mod qualifier;
mod request_context;
mod variable;

use super::index::{CustomLintFileNode, SemanticIndex};
use super::source::{DocumentKind, SourceStore};
use super::syntax::SyntaxIndex;

pub(super) use fields::json_from_toml_value;

pub(super) fn build_semantic_index(source: &SourceStore, syntax: &SyntaxIndex) -> SemanticIndex {
    let mut index = SemanticIndex::default();

    for document in source.documents.values() {
        match &document.kind {
            DocumentKind::Manifest => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index.manifest = Some(manifest::project_manifest(document, toml));
            }
            DocumentKind::Qualifier { id } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .qualifiers
                    .insert(id.clone(), qualifier::project_qualifier(document, toml, id));
            }
            DocumentKind::Variable { id } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .variables
                    .insert(id.clone(), variable::project_variable(document, toml, id));
            }
            DocumentKind::Catalog { id } => {
                index
                    .catalogs
                    .insert(id.clone(), catalog::project_catalog(document, syntax, id));
            }
            DocumentKind::CatalogEntry {
                catalog_id,
                entry_id,
            } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .catalog_entries
                    .entry(catalog_id.clone())
                    .or_default()
                    .insert(
                        entry_id.clone(),
                        catalog::project_catalog_entry(document, toml, catalog_id, entry_id),
                    );
            }
            DocumentKind::RequestContext { id } => {
                index.request_contexts.insert(
                    id.clone(),
                    request_context::project_request_context(document, syntax, id),
                );
            }
            DocumentKind::RequestContextEntry {
                request_context_id,
                entry_id,
            } => {
                index
                    .request_context_entries
                    .entry(request_context_id.clone())
                    .or_default()
                    .insert(
                        entry_id.clone(),
                        request_context::project_request_context_entry(
                            document,
                            syntax,
                            request_context_id,
                            entry_id,
                        ),
                    );
            }
            DocumentKind::CustomLint => {
                index.custom_lints.files.insert(
                    document.path.clone(),
                    CustomLintFileNode {
                        path: document.path.clone(),
                        doc: document.id,
                        location: document.document_location(),
                    },
                );
            }
        }
    }
    catalog::compile_catalog_validators(&mut index);
    index
}
