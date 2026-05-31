use crate::diagnostics::{DiagnosticLocation, SourcePosition};

use super::super::WorkspaceLintSnapshot;
use super::WorkspaceReference;

pub(crate) fn references(
    snapshot: &WorkspaceLintSnapshot,
    path: &str,
    position: SourcePosition,
    include_declaration: bool,
) -> Vec<WorkspaceReference> {
    let Some(target) = snapshot.references.target_at_position(path, position) else {
        return Vec::new();
    };

    let mut locations = snapshot.references.reference_locations(&target);
    if include_declaration && let Some(declaration) = snapshot.references.declaration(&target) {
        locations.push(declaration);
    }

    references_from_locations(snapshot, locations)
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
