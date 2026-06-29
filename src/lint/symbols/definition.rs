use crate::diagnostics::{DiagnosticLocation, SourcePosition};

use super::super::PackageLintSnapshot;
use super::super::index::*;
use super::PackageDefinition;
use super::common::{
    location_contains_position, source_range_size, variable_value_definition_location,
};

pub(crate) fn definition(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<PackageDefinition> {
    let mut candidates = Vec::new();
    push_reference_definition_candidates(snapshot, path, position, &mut candidates);
    push_qualifier_definition_candidates(&snapshot.index, path, position, &mut candidates);
    push_variable_definition_candidates(&snapshot.index, path, position, &mut candidates);
    sort_definition_candidates(&mut candidates);
    candidates
        .into_iter()
        .next()
        .and_then(|candidate| definition_for_location(snapshot, candidate.location))
}

fn definition_for_location(
    snapshot: &PackageLintSnapshot,
    mut location: DiagnosticLocation,
) -> Option<PackageDefinition> {
    let document = snapshot
        .lint
        .documents
        .iter()
        .find(|document| document.path == location.path)?;
    location.doc = Some(document.id);
    let uri = document.uri.clone();
    Some(PackageDefinition { uri, location })
}

fn push_reference_definition_candidates(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    if let Some(target) = snapshot.references.target_at_position(path, position)
        && let Some(location) = snapshot.references.declaration(&target)
    {
        candidates.push(DefinitionCandidate {
            priority: 0,
            span_size: location.range.map(source_range_size).unwrap_or(usize::MAX),
            location,
        });
    }
}

struct DefinitionCandidate {
    priority: u8,
    span_size: usize,
    location: DiagnosticLocation,
}

fn push_qualifier_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    let _ = (index, path, position, candidates);
}

fn push_variable_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    for variable in index.variables.values() {
        match &variable.type_source {
            TypeSourceNode::Catalog(catalog)
                if location_contains_position(&catalog.location, path, position) =>
            {
                if let Some(catalog_node) = index.catalogs.get(&catalog.value) {
                    candidates.push(DefinitionCandidate {
                        priority: 1,
                        span_size: catalog
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        location: catalog_node.location.clone(),
                    });
                }
            }
            _ => {}
        }

        let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
            continue;
        };

        if let ProjectField::Present(value) = default.as_ref()
            && location_contains_position(&value.location, path, position)
            && let Some(target) = variable_value_definition_location(index, variable, &value.value)
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

        let RuleCollection::Rules(rules) = rules else {
            continue;
        };

        for rule in rules {
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

fn sort_definition_candidates(candidates: &mut [DefinitionCandidate]) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.span_size.cmp(&right.span_size))
            .then_with(|| left.location.path.cmp(&right.location.path))
    });
}
