use super::super::nodes::ValueNode;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location};
use super::fields::json_from_toml_value;

pub(crate) fn project_external_value(
    document: &SourceDocument,
    toml: &ParsedToml,
    key: &str,
) -> ValueNode {
    let root = toml.edit.as_table();
    let wrapped_value = toml
        .plain
        .as_table()
        .filter(|table| table.len() == 1)
        .and_then(|table| table.get("value"));

    match wrapped_value {
        Some(value) => ValueNode {
            key: key.to_owned(),
            location: root
                .get("value")
                .map(|item| item_location(document, item))
                .unwrap_or_else(|| document.document_location()),
            value: json_from_toml_value(value),
        },
        None => ValueNode {
            key: key.to_owned(),
            location: document.document_location(),
            value: json_from_toml_value(&toml.plain),
        },
    }
}
