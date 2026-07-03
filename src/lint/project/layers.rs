use toml_span::Value as TomlValue;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location, value_location};
use super::fields::{
    expression_field, integer_field, optional_expression_field, optional_string_field,
};
use crate::diagnostics::DiagnosticLocation;

pub(crate) fn project_layer(document: &SourceDocument, toml: &ParsedToml, id: &str) -> LayerNode {
    let root = toml.root_table();
    let location = document.document_location();
    let schema_version = root
        .map(|root| integer_field(document, root, "schema_version", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let description = root.and_then(|root| optional_string_field(document, root, "description"));
    let unit = root
        .map(|root| expression_field(document, root, "unit", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let buckets = root
        .map(|root| integer_field(document, root, "buckets", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });

    let mut allocations = Vec::new();
    let mut allocations_invalid = false;
    match root.and_then(|root| root.get("allocation")) {
        None => {}
        Some(item) => match item.as_array() {
            Some(values) => {
                allocations = values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| project_allocation(document, index, value))
                    .collect();
            }
            None => allocations_invalid = true,
        },
    }

    LayerNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        unit,
        buckets,
        allocations,
        allocations_invalid,
    }
}

fn project_allocation(
    document: &SourceDocument,
    index: usize,
    value: &TomlValue<'_>,
) -> AllocationNode {
    let location = value_location(document, value);
    let Some(table) = value.as_table() else {
        return AllocationNode {
            index,
            location: location.clone(),
            id: ProjectField::Invalid { location },
            status: None,
            eligibility: None,
            arms: Vec::new(),
            arms_invalid: false,
            invalid_shape: true,
        };
    };

    let id = string_field(document, table, "id", location.clone());
    let status = optional_string_field(document, table, "status");
    let eligibility = optional_expression_field(document, table, "eligibility");

    let mut arms = Vec::new();
    let mut arms_invalid = false;
    match table.get("arm") {
        None => {}
        Some(item) => match item.as_array() {
            Some(values) => {
                arms = values
                    .iter()
                    .enumerate()
                    .map(|(arm_index, value)| project_arm(document, arm_index, value))
                    .collect();
            }
            None => arms_invalid = true,
        },
    }

    AllocationNode {
        index,
        location,
        id,
        status,
        eligibility,
        arms,
        arms_invalid,
        invalid_shape: false,
    }
}

fn project_arm(document: &SourceDocument, index: usize, value: &TomlValue<'_>) -> ArmNode {
    let location = value_location(document, value);
    let Some(table) = value.as_table() else {
        return ArmNode {
            index,
            location: location.clone(),
            name: ProjectField::Invalid {
                location: location.clone(),
            },
            buckets: ProjectField::Invalid { location },
            invalid_shape: true,
        };
    };

    ArmNode {
        index,
        location: location.clone(),
        name: string_field(document, table, "name", location.clone()),
        buckets: string_field(document, table, "buckets", location),
        invalid_shape: false,
    }
}

fn string_field(
    document: &SourceDocument,
    table: &toml_span::value::Table<'_>,
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
