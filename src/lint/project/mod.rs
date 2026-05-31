mod external_value;
mod fields;
mod manifest;
mod qualifier;
mod schema;
mod variable;

use super::index::SemanticIndex;
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
                index.variables.insert(
                    id.clone(),
                    variable::project_variable(document, toml, id, source),
                );
            }
            DocumentKind::ExternalValue {
                variable_id,
                value_key,
            } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .external_values
                    .entry(variable_id.clone())
                    .or_default()
                    .insert(
                        value_key.clone(),
                        external_value::project_external_value(document, toml, value_key),
                    );
            }
            DocumentKind::Schema => {
                index.schemas.insert(
                    document.path.clone(),
                    schema::project_schema(document, syntax),
                );
            }
            DocumentKind::CustomLint => {}
        }
    }

    index
}
