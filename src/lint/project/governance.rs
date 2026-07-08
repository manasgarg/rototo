use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location, value_location};

/// The entity kinds a governance block may govern.
pub(crate) const GOVERNED_KINDS: &[&str] =
    &["catalog", "list", "variable", "evaluation_context", "layer"];

pub(crate) fn project_governance(document: &SourceDocument, toml: &ParsedToml) -> GovernanceNode {
    let location = document.document_location();
    let mut blocks = Vec::new();
    let mut unknown_kinds = Vec::new();

    if let Some(root) = toml.root_table() {
        for (kind_key, kind_item) in root.iter() {
            // The [defaults] block grants across every base-declared entity;
            // its keys sit directly on the table rather than under an id.
            if kind_key.name.as_ref() == "defaults" {
                let block_location = item_location(document, kind_item);
                let Some(block) = kind_item.as_table() else {
                    blocks.push(GovernanceBlockNode {
                        location: block_location.clone(),
                        kind: "defaults".to_owned(),
                        id: String::new(),
                        allowed_operations: Some(ProjectField::Invalid {
                            location: block_location,
                        }),
                        denied_operations: None,
                        update_policy: None,
                        delete_policy: None,
                        unknown_keys: Vec::new(),
                    });
                    continue;
                };
                let mut unknown_keys = Vec::new();
                for (key, item) in block.iter() {
                    if !matches!(
                        key.name.as_ref(),
                        "allowed_operations"
                            | "denied_operations"
                            | "update_policy"
                            | "delete_policy"
                    ) {
                        unknown_keys.push(Spanned {
                            value: key.name.to_string(),
                            location: item_location(document, item),
                        });
                    }
                }
                blocks.push(GovernanceBlockNode {
                    location: block_location,
                    kind: "defaults".to_owned(),
                    id: String::new(),
                    allowed_operations: string_list(document, block, "allowed_operations"),
                    denied_operations: string_list(document, block, "denied_operations"),
                    update_policy: policy(document, block, "update_policy"),
                    delete_policy: policy(document, block, "delete_policy"),
                    unknown_keys,
                });
                continue;
            }
            if !GOVERNED_KINDS.contains(&kind_key.name.as_ref()) {
                unknown_kinds.push(Spanned {
                    value: kind_key.name.to_string(),
                    location: item_location(document, kind_item),
                });
                continue;
            }
            let Some(kind_table) = kind_item.as_table() else {
                unknown_kinds.push(Spanned {
                    value: kind_key.name.to_string(),
                    location: item_location(document, kind_item),
                });
                continue;
            };
            for (id_key, id_item) in kind_table.iter() {
                let block_location = item_location(document, id_item);
                let Some(block) = id_item.as_table() else {
                    blocks.push(GovernanceBlockNode {
                        location: block_location.clone(),
                        kind: kind_key.name.to_string(),
                        id: id_key.name.to_string(),
                        allowed_operations: Some(ProjectField::Invalid {
                            location: block_location,
                        }),
                        denied_operations: None,
                        update_policy: None,
                        delete_policy: None,
                        unknown_keys: Vec::new(),
                    });
                    continue;
                };

                let mut unknown_keys = Vec::new();
                for (key, item) in block.iter() {
                    if !matches!(
                        key.name.as_ref(),
                        "allowed_operations"
                            | "denied_operations"
                            | "update_policy"
                            | "delete_policy"
                    ) {
                        unknown_keys.push(Spanned {
                            value: key.name.to_string(),
                            location: item_location(document, item),
                        });
                    }
                }

                blocks.push(GovernanceBlockNode {
                    location: block_location,
                    kind: kind_key.name.to_string(),
                    id: id_key.name.to_string(),
                    allowed_operations: string_list(document, block, "allowed_operations"),
                    denied_operations: string_list(document, block, "denied_operations"),
                    update_policy: policy(document, block, "update_policy"),
                    delete_policy: policy(document, block, "delete_policy"),
                    unknown_keys,
                });
            }
        }
    }

    GovernanceNode {
        doc: document.id,
        location,
        blocks,
        unknown_kinds,
    }
}

fn policy(
    document: &SourceDocument,
    block: &toml_span::value::Table<'_>,
    key: &str,
) -> Option<GovernancePolicyNode> {
    let item = block.get(key)?;
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return Some(GovernancePolicyNode {
            location: location.clone(),
            allowed_entries: Some(ProjectField::Invalid { location }),
            denied_entries: None,
            allowed_fields: None,
            denied_fields: None,
        });
    };
    Some(GovernancePolicyNode {
        location,
        allowed_entries: string_list(document, table, "allowed_entries"),
        denied_entries: string_list(document, table, "denied_entries"),
        allowed_fields: string_list(document, table, "allowed_fields"),
        denied_fields: string_list(document, table, "denied_fields"),
    })
}

fn string_list(
    document: &SourceDocument,
    table: &toml_span::value::Table<'_>,
    key: &str,
) -> Option<ProjectField<Vec<Spanned<String>>>> {
    let item = table.get(key)?;
    Some(match item.as_array() {
        Some(values) => {
            let mut list = Vec::new();
            for value in values {
                match value.as_str() {
                    Some(value_str) => list.push(Spanned {
                        value: value_str.to_owned(),
                        location: value_location(document, value),
                    }),
                    None => {
                        return Some(ProjectField::Invalid {
                            location: item_location(document, item),
                        });
                    }
                }
            }
            ProjectField::Present(Spanned {
                value: list,
                location: item_location(document, item),
            })
        }
        None => ProjectField::Invalid {
            location: item_location(document, item),
        },
    })
}
