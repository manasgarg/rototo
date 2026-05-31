use toml_span::Value as TomlValue;
use toml_span::value::Table;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location, table_location, value_location};
use super::fields::{optional_severity_field, string_field};

pub(crate) fn project_manifest(document: &SourceDocument, toml: &ParsedToml) -> ManifestNode {
    let root = toml.root_table();
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
    root: Option<&Table<'_>>,
) -> WorkspaceEnvironmentCollection {
    let Some(root) = root else {
        return WorkspaceEnvironmentCollection::Missing;
    };
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

fn project_context_schema(
    document: &SourceDocument,
    root: Option<&Table<'_>>,
) -> Option<ContextSchemaNode> {
    let root = root?;
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

fn project_workspace_custom_rules(
    document: &SourceDocument,
    root: Option<&Table<'_>>,
) -> CustomRuleCollection {
    let Some(root) = root else {
        return CustomRuleCollection::Rules(Vec::new());
    };
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

fn project_custom_rule_declarations(
    document: &SourceDocument,
    lint_table: &Table<'_>,
) -> CustomRuleCollection {
    let Some(item) = lint_table.get("rule") else {
        return CustomRuleCollection::Rules(Vec::new());
    };
    let Some(rules) = item.as_array() else {
        return CustomRuleCollection::Invalid {
            location: item_location(document, item),
        };
    };

    CustomRuleCollection::Rules(
        rules
            .iter()
            .map(|value| project_custom_rule_declaration(document, value))
            .collect(),
    )
}

fn project_custom_rule_declaration(
    document: &SourceDocument,
    value: &TomlValue<'_>,
) -> CustomRuleDeclarationNode {
    let location = table_location(document, value);
    let Some(table) = value.as_table() else {
        return CustomRuleDeclarationNode {
            location: location.clone(),
            id: ProjectField::Invalid {
                location: location.clone(),
            },
            title: ProjectField::Invalid {
                location: location.clone(),
            },
            help: ProjectField::Invalid {
                location: location.clone(),
            },
            severity: None,
        };
    };
    CustomRuleDeclarationNode {
        location: location.clone(),
        id: string_field(document, table, "id", location.clone()),
        title: string_field(document, table, "title", location.clone()),
        help: string_field(document, table, "help", location),
        severity: optional_severity_field(document, table, "severity"),
    }
}
