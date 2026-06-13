mod catalog;
mod fields;
mod manifest;
mod qualifier;
mod schema;
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
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .catalogs
                    .insert(id.clone(), catalog::project_catalog(document, toml, id));
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
            DocumentKind::Schema => {
                index.schemas.insert(
                    document.path.clone(),
                    schema::project_schema(document, syntax),
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
    index
}
