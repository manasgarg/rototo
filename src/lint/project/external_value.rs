use super::super::index::ValueNode;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location};
use super::fields::json_from_toml_value;

pub(crate) fn project_external_value(
    document: &SourceDocument,
    toml: &ParsedToml,
    key: &str,
) -> ValueNode {
    let root = toml.root();
    let wrapped_value = root
        .as_table()
        .filter(|table| table.len() == 1)
        .and_then(|table| table.get("value"));

    match wrapped_value {
        Some(value) => ValueNode {
            key: key.to_owned(),
            location: item_location(document, value),
            value: json_from_toml_value(value),
        },
        None => ValueNode {
            key: key.to_owned(),
            location: document.document_location(),
            value: json_from_toml_value(root),
        },
    }
}
