mod external_value;
mod fields;
mod manifest;
mod qualifier;
mod variable;

use super::nodes::SemanticIndex;
use super::source::{DocumentKind, SourceStore};
use super::syntax::SyntaxIndex;

pub(super) use fields::json_from_toml_value;

pub(super) fn build_semantic_index(source: &SourceStore, syntax: &SyntaxIndex) -> SemanticIndex {
    let mut index = SemanticIndex::default();

    for document in source.documents.values() {
        let Some(toml) = syntax.toml.get(&document.id) else {
            continue;
        };

        match &document.kind {
            DocumentKind::Manifest => {
                index.manifest = Some(manifest::project_manifest(document, toml));
            }
            DocumentKind::Qualifier { id } => {
                index
                    .qualifiers
                    .insert(id.clone(), qualifier::project_qualifier(document, toml, id));
            }
            DocumentKind::Variable { id } => {
                index.variables.insert(
                    id.clone(),
                    variable::project_variable(document, toml, id, source),
                );
            }
            DocumentKind::ExternalValue {
                variable_id,
                value_key,
            } => {
                index
                    .external_values
                    .entry(variable_id.clone())
                    .or_default()
                    .insert(
                        value_key.clone(),
                        external_value::project_external_value(document, toml, value_key),
                    );
            }
            DocumentKind::Schema | DocumentKind::CustomLint => {}
        }
    }

    index
}
