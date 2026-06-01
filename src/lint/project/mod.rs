mod fields;
mod manifest;
mod qualifier;
mod resource;
mod schema;
mod variable;

use super::index::{CustomLintFileNode, CustomRuleDefinitionNode, SemanticIndex};
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
            DocumentKind::Resource { id } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .resources
                    .insert(id.clone(), resource::project_resource(document, toml, id));
            }
            DocumentKind::ResourceObject {
                resource_id,
                object_id,
            } => {
                let Some(toml) = syntax.toml.get(&document.id) else {
                    continue;
                };
                index
                    .resource_objects
                    .entry(resource_id.clone())
                    .or_default()
                    .insert(
                        object_id.clone(),
                        resource::project_resource_object(document, toml, resource_id, object_id),
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
    if let Some(manifest) = &index.manifest {
        index.custom_lints.rules = manifest_custom_rule_definitions(manifest)
            .into_iter()
            .map(|node| (node.definition.rule.clone(), node))
            .collect();
    }

    index
}

fn manifest_custom_rule_definitions(
    manifest: &super::index::ManifestNode,
) -> Vec<CustomRuleDefinitionNode> {
    let super::index::CustomRuleCollection::Rules(rules) = &manifest.custom_rules else {
        return Vec::new();
    };
    rules
        .iter()
        .filter_map(|rule| {
            Some(CustomRuleDefinitionNode {
                definition: rule.definition()?,
                location: rule.location.clone(),
            })
        })
        .collect()
}
