use std::collections::{BTreeMap, BTreeSet};

use toml_span::Value as TomlValue;
use toml_span::value::Table;

use crate::diagnostics::DiagnosticLocation;

use super::super::index::*;
use super::super::source::{SourceDocument, SourceStore};
use super::super::syntax::{ParsedToml, item_location, value_location};
use super::fields::{integer_field, json_from_toml_value, optional_string_field, string_field};

pub(crate) fn project_variable(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
    source: &SourceStore,
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
    let values = project_values(document, toml, root, id, source);
    let environments = project_environments(document, root, id);

    VariableNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        type_source,
        values,
        environments,
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
            Some(type_name) => TypeSourceNode::Primitive(Spanned {
                value: type_name.to_owned(),
                location: item_location(document, item),
            }),
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

fn project_values(
    document: &SourceDocument,
    toml: &ParsedToml,
    root: Option<&Table<'_>>,
    id: &str,
    source: &SourceStore,
) -> ValuesNode {
    let external_keys = source.external_value_keys(id);
    let Some(root) = root else {
        return ValuesNode {
            location: document.document_location(),
            inline_keys: BTreeSet::new(),
            inline_values: BTreeMap::new(),
            external_keys,
            invalid_shape: false,
        };
    };
    let Some(item) = root.get("values") else {
        return ValuesNode {
            location: document.document_location(),
            inline_keys: BTreeSet::new(),
            inline_values: BTreeMap::new(),
            external_keys,
            invalid_shape: false,
        };
    };
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return ValuesNode {
            location,
            inline_keys: BTreeSet::new(),
            inline_values: BTreeMap::new(),
            external_keys,
            invalid_shape: true,
        };
    };

    let inline_values = project_inline_values(document, toml, table);
    ValuesNode {
        location,
        inline_keys: inline_values.keys().cloned().collect(),
        inline_values,
        external_keys,
        invalid_shape: false,
    }
}

fn project_inline_values(
    document: &SourceDocument,
    _toml: &ParsedToml,
    table: &Table<'_>,
) -> BTreeMap<String, ValueNode> {
    table
        .iter()
        .map(|(key, value)| {
            let key = key.name.to_string();
            (
                key.clone(),
                ValueNode {
                    key,
                    location: item_location(document, value),
                    value: json_from_toml_value(value),
                },
            )
        })
        .collect()
}

fn project_environments(
    document: &SourceDocument,
    root: Option<&Table<'_>>,
    variable_id: &str,
) -> EnvironmentCollection {
    let Some(root) = root else {
        return EnvironmentCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(item) = root.get("env") else {
        return EnvironmentCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(table) = item.as_table() else {
        return EnvironmentCollection::Invalid {
            location: item_location(document, item),
        };
    };

    EnvironmentCollection::Environments(
        table
            .iter()
            .map(|(environment, item)| {
                (
                    environment.name.to_string(),
                    project_environment_block(document, variable_id, &environment.name, item),
                )
            })
            .collect(),
    )
}

fn project_environment_block(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    item: &TomlValue<'_>,
) -> EnvironmentBlockNode {
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return EnvironmentBlockNode {
            environment: environment.to_owned(),
            location: location.clone(),
            value: ProjectField::Invalid {
                location: location.clone(),
            },
            rules: RuleCollection::Rules(Vec::new()),
        };
    };

    EnvironmentBlockNode {
        environment: environment.to_owned(),
        location: location.clone(),
        value: string_field(document, table, "value", location.clone()),
        rules: project_rules(document, variable_id, environment, table),
    }
}

fn project_rules(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
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
                .map(|(index, value)| {
                    project_rule_from_value(document, variable_id, environment, index, value)
                })
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
    environment: &str,
    index: usize,
    value: &TomlValue<'_>,
) -> VariableRuleNode {
    let location = value_location(document, value);
    let Some(table) = value.as_table() else {
        return VariableRuleNode {
            index,
            location: location.clone(),
            qualifier: ProjectField::Invalid {
                location: location.clone(),
            },
            value: ProjectField::Invalid { location },
            invalid_shape: true,
        };
    };
    project_rule_from_table_like(
        document,
        variable_id,
        environment,
        index,
        table,
        location,
        false,
    )
}

fn project_rule_from_table_like(
    document: &SourceDocument,
    _variable_id: &str,
    _environment: &str,
    index: usize,
    table: &Table<'_>,
    location: DiagnosticLocation,
    invalid_shape: bool,
) -> VariableRuleNode {
    VariableRuleNode {
        index,
        location: location.clone(),
        qualifier: string_field(document, table, "qualifier", location.clone()),
        value: string_field(document, table, "value", location.clone()),
        invalid_shape,
    }
}
