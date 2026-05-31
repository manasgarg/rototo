use crate::diagnostics::{DiagnosticLocation, DocId, SourcePosition};

use super::super::WorkspaceLintSnapshot;
use super::super::builtins::qualifier_reference;
use super::super::engine::{resolve_workspace_relative_path, resolve_workspace_root_path};
use super::super::nodes::*;
use super::WorkspaceReference;
use super::common::{
    location_contains_position, source_range_size, variable_value_definition_location,
};

pub(crate) fn references(
    snapshot: &WorkspaceLintSnapshot,
    path: &str,
    position: SourcePosition,
    include_declaration: bool,
) -> Vec<WorkspaceReference> {
    let Some(target) = reference_target_at_position(&snapshot.index, path, position) else {
        return Vec::new();
    };
    let mut references = reference_locations_for_target(&snapshot.index, &target);
    if include_declaration
        && let Some(declaration) = reference_target_declaration(&snapshot.index, &target)
    {
        references.push(declaration);
    }
    references_from_locations(snapshot, references)
}

fn references_from_locations(
    snapshot: &WorkspaceLintSnapshot,
    locations: Vec<DiagnosticLocation>,
) -> Vec<WorkspaceReference> {
    let mut references = locations
        .into_iter()
        .filter_map(|mut location| {
            let document = snapshot
                .lint
                .documents
                .iter()
                .find(|document| document.path == location.path)?;
            location.doc = Some(document.id);
            Some(WorkspaceReference {
                uri: document.uri.clone(),
                location,
            })
        })
        .collect::<Vec<_>>();
    sort_and_deduplicate_workspace_references(&mut references);
    references
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReferenceTarget {
    Qualifier(String),
    VariableValue { variable: String, value: String },
    Schema(String),
    ContextAttribute(String),
}

struct ReferenceTargetCandidate {
    priority: u8,
    span_size: usize,
    target: ReferenceTarget,
}

fn reference_target_at_position(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
) -> Option<ReferenceTarget> {
    let mut candidates = Vec::new();
    push_reference_targets_from_manifest(index, path, position, &mut candidates);
    push_reference_targets_from_qualifiers(index, path, position, &mut candidates);
    push_reference_targets_from_variables(index, path, position, &mut candidates);
    push_reference_targets_from_schema_documents(index, path, &mut candidates);
    sort_reference_target_candidates(&mut candidates);
    candidates
        .into_iter()
        .next()
        .map(|candidate| candidate.target)
}

fn push_reference_targets_from_manifest(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    let Some(manifest) = &index.manifest else {
        return;
    };
    let Some(context) = &manifest.context_schema else {
        return;
    };
    let ProjectField::Present(schema) = &context.schema else {
        return;
    };
    if location_contains_position(&schema.location, path, position)
        && let Some(schema_path) = resolve_workspace_root_path(&schema.value)
    {
        candidates.push(ReferenceTargetCandidate {
            priority: 0,
            span_size: schema
                .location
                .range
                .map(source_range_size)
                .unwrap_or(usize::MAX),
            target: ReferenceTarget::Schema(schema_path),
        });
    }
}

fn push_reference_targets_from_qualifiers(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    for qualifier in index.qualifiers.values() {
        if qualifier.location.path == path {
            candidates.push(ReferenceTargetCandidate {
                priority: 5,
                span_size: usize::MAX,
                target: ReferenceTarget::Qualifier(qualifier.id.clone()),
            });
        }

        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };
        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if !location_contains_position(&attribute.location, path, position) {
                continue;
            }
            match qualifier_reference(&attribute.value) {
                Some(qualifier_id) => candidates.push(ReferenceTargetCandidate {
                    priority: 0,
                    span_size: attribute
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::Qualifier(qualifier_id.to_owned()),
                }),
                None => candidates.push(ReferenceTargetCandidate {
                    priority: 0,
                    span_size: attribute
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::ContextAttribute(attribute.value.clone()),
                }),
            }
        }
    }
}

fn push_reference_targets_from_variables(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    for variable in index.variables.values() {
        if let TypeSourceNode::Schema(schema) = &variable.type_source
            && location_contains_position(&schema.location, path, position)
            && let Some(schema_path) =
                resolve_workspace_relative_path(&variable.location.path, &schema.value)
        {
            candidates.push(ReferenceTargetCandidate {
                priority: 0,
                span_size: schema
                    .location
                    .range
                    .map(source_range_size)
                    .unwrap_or(usize::MAX),
                target: ReferenceTarget::Schema(schema_path),
            });
        }

        for value in variable.values.inline_values.values() {
            if location_contains_position(&value.location, path, position) {
                candidates.push(ReferenceTargetCandidate {
                    priority: 1,
                    span_size: value
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::VariableValue {
                        variable: variable.id.clone(),
                        value: value.key.clone(),
                    },
                });
            }
        }

        if let Some(values) = index.external_values.get(&variable.id) {
            for value in values.values() {
                if location_contains_position(&value.location, path, position) {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 1,
                        span_size: value
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        target: ReferenceTarget::VariableValue {
                            variable: variable.id.clone(),
                            value: value.key.clone(),
                        },
                    });
                }
            }
        }

        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };
        for block in environments.values() {
            if let ProjectField::Present(value) = &block.value
                && location_contains_position(&value.location, path, position)
            {
                candidates.push(ReferenceTargetCandidate {
                    priority: 0,
                    span_size: value
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::VariableValue {
                        variable: variable.id.clone(),
                        value: value.value.clone(),
                    },
                });
            }

            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            for rule in rules {
                if let ProjectField::Present(qualifier) = &rule.qualifier
                    && location_contains_position(&qualifier.location, path, position)
                {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 0,
                        span_size: qualifier
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        target: ReferenceTarget::Qualifier(qualifier.value.clone()),
                    });
                }

                if let ProjectField::Present(value) = &rule.value
                    && location_contains_position(&value.location, path, position)
                {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 0,
                        span_size: value
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        target: ReferenceTarget::VariableValue {
                            variable: variable.id.clone(),
                            value: value.value.clone(),
                        },
                    });
                }
            }
        }
    }
}

fn push_reference_targets_from_schema_documents(
    index: &SemanticIndex,
    path: &str,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    if schema_path_is_referenced(index, path) {
        candidates.push(ReferenceTargetCandidate {
            priority: 5,
            span_size: usize::MAX,
            target: ReferenceTarget::Schema(path.to_owned()),
        });
    }
}

fn schema_path_is_referenced(index: &SemanticIndex, path: &str) -> bool {
    context_schema_reference_path(index).as_deref() == Some(path)
        || index.variables.values().any(|variable| {
            matches!(
                &variable.type_source,
                TypeSourceNode::Schema(schema)
                    if resolve_workspace_relative_path(&variable.location.path, &schema.value)
                        .as_deref()
                        == Some(path)
            )
        })
}

fn context_schema_reference_path(index: &SemanticIndex) -> Option<String> {
    let manifest = index.manifest.as_ref()?;
    let context = manifest.context_schema.as_ref()?;
    let ProjectField::Present(schema) = &context.schema else {
        return None;
    };
    resolve_workspace_root_path(&schema.value)
}

fn sort_reference_target_candidates(candidates: &mut [ReferenceTargetCandidate]) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.span_size.cmp(&right.span_size))
    });
}

fn reference_locations_for_target(
    index: &SemanticIndex,
    target: &ReferenceTarget,
) -> Vec<DiagnosticLocation> {
    match target {
        ReferenceTarget::Qualifier(qualifier) => qualifier_reference_locations(index, qualifier),
        ReferenceTarget::VariableValue { variable, value } => {
            variable_value_reference_locations(index, variable, value)
        }
        ReferenceTarget::Schema(schema_path) => schema_reference_locations(index, schema_path),
        ReferenceTarget::ContextAttribute(attribute) => {
            context_attribute_reference_locations(index, attribute)
        }
    }
}

fn reference_target_declaration(
    index: &SemanticIndex,
    target: &ReferenceTarget,
) -> Option<DiagnosticLocation> {
    match target {
        ReferenceTarget::Qualifier(qualifier) => index
            .qualifiers
            .get(qualifier)
            .map(|qualifier| qualifier.location.clone()),
        ReferenceTarget::VariableValue { variable, value } => index
            .variables
            .get(variable)
            .and_then(|variable| variable_value_definition_location(index, variable, value)),
        ReferenceTarget::Schema(schema_path) => {
            Some(DiagnosticLocation::document(DocId(0), schema_path.clone()))
        }
        ReferenceTarget::ContextAttribute(_) => None,
    }
}

fn qualifier_reference_locations(
    index: &SemanticIndex,
    qualifier_id: &str,
) -> Vec<DiagnosticLocation> {
    let mut locations = Vec::new();
    for qualifier in index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };
        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if qualifier_reference(&attribute.value) == Some(qualifier_id) {
                locations.push(attribute.location.clone());
            }
        }
    }

    for variable in index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };
        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            for rule in rules {
                if let ProjectField::Present(qualifier) = &rule.qualifier
                    && qualifier.value == qualifier_id
                {
                    locations.push(qualifier.location.clone());
                }
            }
        }
    }
    locations
}

fn variable_value_reference_locations(
    index: &SemanticIndex,
    variable_id: &str,
    value_key: &str,
) -> Vec<DiagnosticLocation> {
    let Some(variable) = index.variables.get(variable_id) else {
        return Vec::new();
    };
    let mut locations = Vec::new();
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return locations;
    };
    for block in environments.values() {
        if let ProjectField::Present(value) = &block.value
            && value.value == value_key
        {
            locations.push(value.location.clone());
        }

        let RuleCollection::Rules(rules) = &block.rules else {
            continue;
        };
        for rule in rules {
            if let ProjectField::Present(value) = &rule.value
                && value.value == value_key
            {
                locations.push(value.location.clone());
            }
        }
    }
    locations
}

fn schema_reference_locations(index: &SemanticIndex, schema_path: &str) -> Vec<DiagnosticLocation> {
    let mut locations = Vec::new();
    if context_schema_reference_path(index).as_deref() == Some(schema_path)
        && let Some(manifest) = &index.manifest
        && let Some(context) = &manifest.context_schema
        && let ProjectField::Present(schema) = &context.schema
    {
        locations.push(schema.location.clone());
    }

    for variable in index.variables.values() {
        if let TypeSourceNode::Schema(schema) = &variable.type_source
            && resolve_workspace_relative_path(&variable.location.path, &schema.value).as_deref()
                == Some(schema_path)
        {
            locations.push(schema.location.clone());
        }
    }
    locations
}

fn context_attribute_reference_locations(
    index: &SemanticIndex,
    attribute: &str,
) -> Vec<DiagnosticLocation> {
    let mut locations = Vec::new();
    for qualifier in index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };
        for predicate in predicates {
            let ProjectField::Present(predicate_attribute) = &predicate.attribute else {
                continue;
            };
            if qualifier_reference(&predicate_attribute.value).is_none()
                && predicate_attribute.value == attribute
            {
                locations.push(predicate_attribute.location.clone());
            }
        }
    }
    locations
}

fn sort_and_deduplicate_workspace_references(references: &mut Vec<WorkspaceReference>) {
    references.sort_by(|left, right| {
        left.uri.cmp(&right.uri).then_with(|| {
            source_location_sort_key(&left.location).cmp(&source_location_sort_key(&right.location))
        })
    });
    references.dedup_by(|left, right| {
        left.uri == right.uri
            && source_location_sort_key(&left.location) == source_location_sort_key(&right.location)
    });
}

fn source_location_sort_key(location: &DiagnosticLocation) -> (usize, usize, usize, usize) {
    location
        .range
        .map(|range| {
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
            )
        })
        .unwrap_or((0, 0, 0, 0))
}
