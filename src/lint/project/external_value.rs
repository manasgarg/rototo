use super::super::index::{ValueNode, ValueOrigin};
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, table_location};
use super::fields::json_from_toml_value;

pub(crate) fn project_external_value(
    document: &SourceDocument,
    toml: &ParsedToml,
    variable_id: &str,
    key: &str,
) -> ValueNode {
    let root = toml.root();
    ValueNode {
        variable_id: variable_id.to_owned(),
        key: key.to_owned(),
        location: table_location(document, root),
        value: json_from_toml_value(root),
        origin: ValueOrigin::External {
            doc: document.id,
            path: document.path.clone(),
        },
    }
}
