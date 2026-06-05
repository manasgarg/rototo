use toml_span::value::Table;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location, value_location};

pub(crate) fn project_manifest(document: &SourceDocument, toml: &ParsedToml) -> ManifestNode {
    let root = toml.root_table();
    let location = document.document_location();
    ManifestNode {
        doc: document.id,
        location,
        extends: project_extends(document, root),
    }
}

fn project_extends(
    document: &SourceDocument,
    root: Option<&Table<'_>>,
) -> WorkspaceExtendsCollection {
    let Some(root) = root else {
        return WorkspaceExtendsCollection::Missing;
    };
    let Some(item) = root.get("extends") else {
        return WorkspaceExtendsCollection::Missing;
    };
    let location = item_location(document, item);
    let Some(values) = item.as_array() else {
        return WorkspaceExtendsCollection::Invalid { location };
    };

    WorkspaceExtendsCollection::Sources {
        location,
        values: values
            .iter()
            .filter_map(|value| {
                Some(WorkspaceExtendNode {
                    source: value.as_str()?.to_owned(),
                    location: value_location(document, value),
                })
            })
            .collect(),
    }
}
