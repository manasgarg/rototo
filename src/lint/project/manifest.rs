use toml_span::value::Table;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location, value_location};
use super::fields::expression_field;

pub(crate) fn project_manifest(document: &SourceDocument, toml: &ParsedToml) -> ManifestNode {
    let root = toml.root_table();
    let location = document.document_location();
    ManifestNode {
        doc: document.id,
        location,
        extends: project_extends(document, root),
        trace: project_trace_policies(document, root),
    }
}

fn project_trace_policies(
    document: &SourceDocument,
    root: Option<&Table<'_>>,
) -> Vec<TracePolicyNode> {
    let Some(root) = root else {
        return Vec::new();
    };
    let Some(item) = root.get("trace") else {
        return Vec::new();
    };
    let Some(values) = item.as_array() else {
        // A non-array `trace` is reported as a single malformed policy so lint
        // surfaces it rather than silently ignoring the key.
        return vec![TracePolicyNode {
            index: 0,
            when: ProjectField::Invalid {
                location: item_location(document, item),
            },
        }];
    };

    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let entry_location = value_location(document, value);
            let when = match value.as_table() {
                Some(table) => expression_field(document, table, "when", entry_location),
                None => ProjectField::Invalid {
                    location: entry_location,
                },
            };
            TracePolicyNode { index, when }
        })
        .collect()
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
