use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml_edit::{Item, Table, TableLike, Value as EditValue};

use crate::diagnostics::{DiagnosticLocation, Severity};

use super::nodes::*;
use super::source::{SourceDocument, SourceStore};
use super::syntax::{ParsedToml, item_location, table_location, value_location};

pub(super) fn project_manifest(document: &SourceDocument, toml: &ParsedToml) -> ManifestNode {
    let root = toml.edit.as_table();
    ManifestNode {
        doc: document.id,
        location: document.document_location(),
        environments: project_workspace_environments(document, root),
        context_schema: project_context_schema(document, root),
        custom_rules: project_workspace_custom_rules(document, root),
    }
}

fn project_workspace_environments(
    document: &SourceDocument,
    root: &Table,
) -> WorkspaceEnvironmentCollection {
    let Some(item) = root.get("environments") else {
        return WorkspaceEnvironmentCollection::Missing;
    };
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return WorkspaceEnvironmentCollection::Invalid { location };
    };
    let Some(values_item) = table.get("values") else {
        return WorkspaceEnvironmentCollection::Missing;
    };
    let location = item_location(document, values_item);
    let Some(values) = values_item.as_array() else {
        return WorkspaceEnvironmentCollection::Invalid { location };
    };

    WorkspaceEnvironmentCollection::Environments {
        location,
        values: values
            .iter()
            .filter_map(|value| {
                Some(WorkspaceEnvironmentNode {
                    name: value.as_str()?.to_owned(),
                    location: value_location(document, value),
                })
            })
            .collect(),
    }
}

fn project_context_schema(document: &SourceDocument, root: &Table) -> Option<ContextSchemaNode> {
    let item = root.get("context")?;
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return Some(ContextSchemaNode {
            location: location.clone(),
            schema: ProjectField::Invalid {
                location: location.clone(),
            },
            invalid_shape: true,
        });
    };

    Some(ContextSchemaNode {
        location: location.clone(),
        schema: string_field(document, table, "schema", location),
        invalid_shape: false,
    })
}

fn project_workspace_custom_rules(document: &SourceDocument, root: &Table) -> CustomRuleCollection {
    let Some(item) = root.get("lint") else {
        return CustomRuleCollection::Rules(Vec::new());
    };
    let Some(table) = item.as_table() else {
        return CustomRuleCollection::Invalid {
            location: item_location(document, item),
        };
    };
    project_custom_rule_declarations(document, table)
}

pub(super) fn project_qualifier(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
) -> QualifierNode {
    let root = toml.edit.as_table();
    let location = document.document_location();
    let schema_version = integer_field(document, root, "schema_version", location.clone());
    let description = optional_string_field(document, root, "description");
    let predicates = project_predicates(document, root);

    QualifierNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        predicates,
    }
}

fn project_predicates(document: &SourceDocument, root: &Table) -> PredicateCollection {
    let Some(item) = root.get("predicate") else {
        return PredicateCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(predicates) = item.as_array_of_tables() else {
        return PredicateCollection::Invalid {
            location: item_location(document, item),
        };
    };

    PredicateCollection::Predicates(
        predicates
            .iter()
            .enumerate()
            .map(|(index, table)| project_predicate(document, index, table))
            .collect(),
    )
}

fn project_predicate(document: &SourceDocument, index: usize, table: &Table) -> PredicateNode {
    let location = table_location(document, table);
    let attribute = string_field(document, table, "attribute", location.clone());
    let op = predicate_op_field(document, table, location.clone());
    let value = table
        .get("value")
        .map(|item| project_value_shape(document, item));
    let salt = table
        .get("salt")
        .map(|_| string_field(document, table, "salt", location.clone()));
    let range = table
        .get("range")
        .map(|item| project_bucket_range(document, item));
    let has_bucket_value = table.contains_key("value");

    PredicateNode {
        index,
        location,
        attribute,
        op,
        value,
        salt,
        range,
        has_bucket_value,
    }
}

pub(super) fn project_variable(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
    source: &SourceStore,
) -> VariableNode {
    let root = toml.edit.as_table();
    let location = document.document_location();
    let schema_version = integer_field(document, root, "schema_version", location.clone());
    let description = optional_string_field(document, root, "description");
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
    root: &Table,
    location: DiagnosticLocation,
) -> TypeSourceNode {
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

fn project_custom_rule_declarations(
    document: &SourceDocument,
    lint_table: &Table,
) -> CustomRuleCollection {
    let Some(item) = lint_table.get("rule") else {
        return CustomRuleCollection::Rules(Vec::new());
    };
    let Some(rules) = item.as_array_of_tables() else {
        return CustomRuleCollection::Invalid {
            location: item_location(document, item),
        };
    };

    CustomRuleCollection::Rules(
        rules
            .iter()
            .map(|table| project_custom_rule_declaration(document, table))
            .collect(),
    )
}

fn project_custom_rule_declaration(
    document: &SourceDocument,
    table: &Table,
) -> CustomRuleDeclarationNode {
    let location = table_location(document, table);
    CustomRuleDeclarationNode {
        location: location.clone(),
        id: string_field(document, table, "id", location.clone()),
        title: string_field(document, table, "title", location.clone()),
        help: string_field(document, table, "help", location),
        severity: optional_severity_field(document, table, "severity"),
    }
}

fn project_values(
    document: &SourceDocument,
    toml: &ParsedToml,
    root: &Table,
    id: &str,
    source: &SourceStore,
) -> ValuesNode {
    let external_keys = source.external_value_keys(id);
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
    toml: &ParsedToml,
    table: &Table,
) -> BTreeMap<String, ValueNode> {
    let plain_values = toml.plain.get("values").and_then(TomlValue::as_table);
    table
        .iter()
        .filter_map(|(key, item)| {
            let value = plain_values?.get(key)?;
            Some((
                key.to_owned(),
                ValueNode {
                    key: key.to_owned(),
                    location: item_location(document, item),
                    value: json_from_toml_value(value),
                },
            ))
        })
        .collect()
}

pub(super) fn project_external_value(
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

fn project_environments(
    document: &SourceDocument,
    root: &Table,
    variable_id: &str,
) -> EnvironmentCollection {
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
                    environment.to_owned(),
                    project_environment_block(document, variable_id, environment, item),
                )
            })
            .collect(),
    )
}

fn project_environment_block(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    item: &Item,
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
    table: &Table,
) -> RuleCollection {
    let Some(item) = table.get("rule") else {
        return RuleCollection::Rules(Vec::new());
    };

    if let Some(rules) = item.as_array_of_tables() {
        return RuleCollection::Rules(
            rules
                .iter()
                .enumerate()
                .map(|(index, table)| {
                    project_rule_from_table(document, variable_id, environment, index, table)
                })
                .collect(),
        );
    }

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

fn project_rule_from_table(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    index: usize,
    table: &Table,
) -> VariableRuleNode {
    let location = table_location(document, table);
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

fn project_rule_from_value(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    index: usize,
    value: &EditValue,
) -> VariableRuleNode {
    let location = value_location(document, value);
    let Some(table) = value.as_inline_table() else {
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
    table: &dyn TableLike,
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

fn integer_field(
    document: &SourceDocument,
    table: &dyn TableLike,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<i64> {
    match table.get(key) {
        Some(item) => match item.as_integer() {
            Some(value) => ProjectField::Present(Spanned {
                value,
                location: item_location(document, item),
            }),
            None => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Missing {
            location: missing_location,
        },
    }
}

fn string_field(
    document: &SourceDocument,
    table: &dyn TableLike,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<String> {
    match table.get(key) {
        Some(item) => match item.as_str() {
            Some(value) => ProjectField::Present(Spanned {
                value: value.to_owned(),
                location: item_location(document, item),
            }),
            None => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Missing {
            location: missing_location,
        },
    }
}

fn optional_string_field(
    document: &SourceDocument,
    table: &dyn TableLike,
    key: &str,
) -> Option<ProjectField<String>> {
    let item = table.get(key)?;
    Some(match item.as_str() {
        Some(value) => ProjectField::Present(Spanned {
            value: value.to_owned(),
            location: item_location(document, item),
        }),
        None => ProjectField::Invalid {
            location: item_location(document, item),
        },
    })
}

fn optional_severity_field(
    document: &SourceDocument,
    table: &dyn TableLike,
    key: &str,
) -> Option<ProjectField<Severity>> {
    let item = table.get(key)?;
    Some(match item.as_str().and_then(Severity::parse) {
        Some(value) => ProjectField::Present(Spanned {
            value,
            location: item_location(document, item),
        }),
        None => ProjectField::Invalid {
            location: item_location(document, item),
        },
    })
}

fn predicate_op_field(
    document: &SourceDocument,
    table: &Table,
    missing_location: DiagnosticLocation,
) -> ProjectField<PredicateOp> {
    match string_field(document, table, "op", missing_location) {
        ProjectField::Present(op) => ProjectField::Present(Spanned {
            value: PredicateOp::from_str(&op.value),
            location: op.location,
        }),
        ProjectField::Invalid { location } => ProjectField::Invalid { location },
        ProjectField::Missing { location } => ProjectField::Missing { location },
    }
}

fn project_value_shape(document: &SourceDocument, item: &Item) -> ValueShapeNode {
    ValueShapeNode {
        location: item_location(document, item),
        shape: value_shape(item),
    }
}

fn project_bucket_range(document: &SourceDocument, item: &Item) -> BucketRangeNode {
    let location = item_location(document, item);
    let Some(array) = item.as_array() else {
        return BucketRangeNode {
            location,
            is_array: false,
            len: 0,
            start: None,
            end: None,
        };
    };
    let values: Vec<_> = array.iter().collect();
    BucketRangeNode {
        location,
        is_array: true,
        len: values.len(),
        start: values.first().and_then(|value| value.as_integer()),
        end: values.get(1).and_then(|value| value.as_integer()),
    }
}

fn value_shape(item: &Item) -> ValueShape {
    if item.as_str().is_some() {
        ValueShape::String
    } else if item.as_integer().is_some() {
        ValueShape::Integer
    } else if item.as_float().is_some() {
        ValueShape::Float
    } else if item.as_bool().is_some() {
        ValueShape::Boolean
    } else if item.as_array().is_some() {
        ValueShape::Array
    } else {
        ValueShape::Table
    }
}

pub(super) fn json_from_toml_value(value: &TomlValue) -> JsonValue {
    serde_json::to_value(value).unwrap_or(JsonValue::Null)
}
