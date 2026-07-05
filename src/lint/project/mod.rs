mod catalog;
mod enums;
mod evaluation_context;
mod fields;
mod governance;
mod layers;
mod manifest;
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
            DocumentKind::Variable { id } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .variables
                    .insert(id.clone(), variable::project_variable(document, toml, id));
            }
            DocumentKind::Enum { id } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .enums
                    .insert(id.clone(), enums::project_enum(document, toml, id));
            }
            DocumentKind::Governance => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index.governance = Some(governance::project_governance(document, toml));
            }
            DocumentKind::Layer { id } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .layers
                    .insert(id.clone(), layers::project_layer(document, toml, id));
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
            DocumentKind::EvaluationContext { id } => {
                index.evaluation_contexts.insert(
                    id.clone(),
                    evaluation_context::project_evaluation_context(document, syntax, id),
                );
            }
            DocumentKind::EvaluationContextSample {
                evaluation_context_id,
                sample_id,
            } => {
                index
                    .evaluation_context_samples
                    .entry(evaluation_context_id.clone())
                    .or_default()
                    .insert(
                        sample_id.clone(),
                        evaluation_context::project_evaluation_context_sample(
                            document,
                            syntax,
                            evaluation_context_id,
                            sample_id,
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
