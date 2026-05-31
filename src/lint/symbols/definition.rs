use crate::diagnostics::{DiagnosticLocation, DocId, SourcePosition};

use super::super::WorkspaceLintSnapshot;
use super::super::builtins::qualifier_reference;
use super::super::engine::{resolve_workspace_relative_path, resolve_workspace_root_path};
use super::super::nodes::*;
use super::WorkspaceDefinition;
use super::common::{
    location_contains_position, source_range_size, variable_value_definition_location,
};

pub(crate) fn definition(
    snapshot: &WorkspaceLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<WorkspaceDefinition> {
    let mut candidates = Vec::new();
    push_manifest_definition_candidates(&snapshot.index, path, position, &mut candidates);
    push_qualifier_definition_candidates(&snapshot.index, path, position, &mut candidates);
    push_variable_definition_candidates(&snapshot.index, path, position, &mut candidates);
    sort_definition_candidates(&mut candidates);
    candidates
        .into_iter()
        .next()
        .and_then(|candidate| definition_for_location(snapshot, candidate.location))
}

fn definition_for_location(
    snapshot: &WorkspaceLintSnapshot,
    mut location: DiagnosticLocation,
) -> Option<WorkspaceDefinition> {
    let document = snapshot
        .lint
        .documents
        .iter()
        .find(|document| document.path == location.path)?;
    location.doc = Some(document.id);
    let uri = document.uri.clone();
    Some(WorkspaceDefinition { uri, location })
}

struct DefinitionCandidate {
    priority: u8,
    span_size: usize,
    location: DiagnosticLocation,
}

fn push_manifest_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
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
    if !location_contains_position(&schema.location, path, position) {
        return;
    }
    let Some(schema_path) = resolve_workspace_root_path(&schema.value) else {
        return;
    };
    candidates.push(DefinitionCandidate {
        priority: 2,
        span_size: schema
            .location
            .range
            .map(source_range_size)
            .unwrap_or(usize::MAX),
        location: DiagnosticLocation::document(DocId(0), schema_path),
    });
}

fn push_qualifier_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    for qualifier in index.qualifiers.values() {
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
            let Some(target_id) = qualifier_reference(&attribute.value) else {
                continue;
            };
            let Some(target) = index.qualifiers.get(target_id) else {
                continue;
            };
            candidates.push(DefinitionCandidate {
                priority: 0,
                span_size: attribute
                    .location
                    .range
                    .map(source_range_size)
                    .unwrap_or(usize::MAX),
                location: target.location.clone(),
            });
        }
    }
}

fn push_variable_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    for variable in index.variables.values() {
        if let TypeSourceNode::Schema(schema) = &variable.type_source
            && location_contains_position(&schema.location, path, position)
            && let Some(schema_path) =
                resolve_workspace_relative_path(&variable.location.path, &schema.value)
        {
            candidates.push(DefinitionCandidate {
                priority: 1,
                span_size: schema
                    .location
                    .range
                    .map(source_range_size)
                    .unwrap_or(usize::MAX),
                location: DiagnosticLocation::document(DocId(0), schema_path),
            });
        }

        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };

        for block in environments.values() {
            if let ProjectField::Present(value) = &block.value
                && location_contains_position(&value.location, path, position)
                && let Some(target) =
                    variable_value_definition_location(index, variable, &value.value)
            {
                candidates.push(DefinitionCandidate {
                    priority: 0,
                    span_size: value
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    location: target,
                });
            }

            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };

            for rule in rules {
                if let ProjectField::Present(qualifier) = &rule.qualifier
                    && location_contains_position(&qualifier.location, path, position)
                    && let Some(target) = index.qualifiers.get(&qualifier.value)
                {
                    candidates.push(DefinitionCandidate {
                        priority: 0,
                        span_size: qualifier
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        location: target.location.clone(),
                    });
                }

                if let ProjectField::Present(value) = &rule.value
                    && location_contains_position(&value.location, path, position)
                    && let Some(target) =
                        variable_value_definition_location(index, variable, &value.value)
                {
                    candidates.push(DefinitionCandidate {
                        priority: 0,
                        span_size: value
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        location: target,
                    });
                }
            }
        }
    }
}

fn sort_definition_candidates(candidates: &mut [DefinitionCandidate]) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.span_size.cmp(&right.span_size))
            .then_with(|| left.location.path.cmp(&right.location.path))
    });
}
