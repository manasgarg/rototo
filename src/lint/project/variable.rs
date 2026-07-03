use std::collections::BTreeMap;

use toml_span::Value as TomlValue;
use toml_span::value::Table;

use crate::diagnostics::DiagnosticLocation;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location, value_location};
use super::fields::{
    integer_field, json_field, json_from_toml_value, optional_expression_field,
    optional_string_field,
};

pub(crate) fn project_variable(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
) -> VariableNode {
    let root = toml.root_table();
    let location = document.document_location();
    let schema_version = root
        .map(|root| integer_field(document, root, "schema_version", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let description = root.and_then(|root| optional_string_field(document, root, "description"));
    let type_source = project_type_source(document, root, location.clone());
    let values = project_values(document, root, id);
    let resolve = project_resolve(document, root, id);

    VariableNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        type_source,
        values,
        resolve,
    }
}

fn project_type_source(
    document: &SourceDocument,
    root: Option<&Table<'_>>,
    location: DiagnosticLocation,
) -> TypeSourceNode {
    let Some(root) = root else {
        return TypeSourceNode::Missing { location };
    };
    let type_item = root.get("type");
    let schema_item = root.get("schema");
    match (type_item, schema_item) {
        (None, None) => TypeSourceNode::Missing { location },
        (Some(_type_item), Some(schema_item)) => TypeSourceNode::Conflict {
            location: item_location(document, schema_item),
        },
        (Some(item), None) => match item.as_str() {
            Some(type_name) => {
                let location = item_location(document, item);
                if let Some(catalog_id) = type_name.strip_prefix("catalog:") {
                    TypeSourceNode::Catalog(Spanned {
                        value: catalog_id.to_owned(),
                        location,
                    })
                } else {
                    TypeSourceNode::Primitive(Spanned {
                        value: type_name.to_owned(),
                        location,
                    })
                }
            }
            None => TypeSourceNode::Invalid {
                location: item_location(document, item),
            },
        },
        (None, Some(item)) => match item.as_str() {
            Some(schema) => TypeSourceNode::Schema(Spanned {
                value: schema.to_owned(),
                location: item_location(document, item),
            }),
            None => TypeSourceNode::Invalid {
                location: item_location(document, item),
            },
        },
    }
}

fn project_values(document: &SourceDocument, root: Option<&Table<'_>>, id: &str) -> ValuesNode {
    let Some(root) = root else {
        return ValuesNode {
            location: document.document_location(),
            inline_values: BTreeMap::new(),
            invalid_shape: false,
        };
    };
    let Some(item) = root.get("values") else {
        return ValuesNode {
            location: document.document_location(),
            inline_values: BTreeMap::new(),
            invalid_shape: false,
        };
    };
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return ValuesNode {
            location,
            inline_values: BTreeMap::new(),
            invalid_shape: true,
        };
    };

    let inline_values = project_inline_values(document, id, table);
    ValuesNode {
        location,
        inline_values,
        invalid_shape: false,
    }
}

fn project_inline_values(
    document: &SourceDocument,
    variable_id: &str,
    table: &Table<'_>,
) -> BTreeMap<String, ValueNode> {
    table
        .iter()
        .map(|(key, value)| {
            let key = key.name.to_string();
            (
                key.clone(),
                ValueNode {
                    variable_id: variable_id.to_owned(),
                    key,
                    location: item_location(document, value),
                    value: json_from_toml_value(value),
                    origin: ValueOrigin::Inline {
                        variable_doc: document.id,
                    },
                },
            )
        })
        .collect()
}

fn project_resolve(
    document: &SourceDocument,
    root: Option<&Table<'_>>,
    variable_id: &str,
) -> ResolveNode {
    let Some(root) = root else {
        return ResolveNode::Missing {
            location: document.document_location(),
        };
    };
    let Some(item) = root.get("resolve") else {
        return ResolveNode::Missing {
            location: document.document_location(),
        };
    };
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return ResolveNode::Invalid { location };
    };

    ResolveNode::Resolve {
        location: location.clone(),
        default: Box::new(json_field(document, table, "default", location.clone())),
        rules: project_rules(document, variable_id, table),
    }
}

fn project_rules(
    document: &SourceDocument,
    variable_id: &str,
    table: &Table<'_>,
) -> RuleCollection {
    let Some(item) = table.get("rule") else {
        return RuleCollection::Rules(Vec::new());
    };

    if let Some(array) = item.as_array() {
        return RuleCollection::Rules(
            array
                .iter()
                .enumerate()
                .map(|(index, value)| project_rule_from_value(document, variable_id, index, value))
                .collect(),
        );
    }

    RuleCollection::Invalid {
        location: item_location(document, item),
    }
}

fn project_rule_from_value(
    document: &SourceDocument,
    variable_id: &str,
    index: usize,
    value: &TomlValue<'_>,
) -> VariableRuleNode {
    let location = value_location(document, value);
    let Some(table) = value.as_table() else {
        return VariableRuleNode {
            index,
            location: location.clone(),
            when: None,
            query: None,
            value: ProjectField::Invalid { location },
            invalid_shape: true,
        };
    };
    project_rule_from_table_like(document, variable_id, index, table, location, false)
}

fn project_rule_from_table_like(
    document: &SourceDocument,
    _variable_id: &str,
    index: usize,
    table: &Table<'_>,
    location: DiagnosticLocation,
    invalid_shape: bool,
) -> VariableRuleNode {
    VariableRuleNode {
        index,
        location: location.clone(),
        when: optional_expression_field(document, table, "when"),
        query: optional_expression_field(document, table, "query"),
        value: json_field(document, table, "value", location.clone()),
        invalid_shape,
    }
}
