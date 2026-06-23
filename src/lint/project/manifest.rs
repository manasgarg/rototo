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
) -> PackageExtendsCollection {
    let Some(root) = root else {
        return PackageExtendsCollection::Missing;
    };
    let Some(item) = root.get("extends") else {
        return PackageExtendsCollection::Missing;
    };
    let location = item_location(document, item);
    let Some(values) = item.as_array() else {
        return PackageExtendsCollection::Invalid { location };
    };

    PackageExtendsCollection::Sources {
        location,
        values: values
            .iter()
            .filter_map(|value| {
                Some(PackageExtendNode {
                    source: value.as_str()?.to_owned(),
                    location: value_location(document, value),
                })
            })
            .collect(),
    }
}
