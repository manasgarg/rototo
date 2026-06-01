use super::super::index::*;
use super::common::location_contains_position;
use super::{WorkspaceCompletionItem, WorkspaceCompletionItemKind};
use crate::diagnostics::SourcePosition;

const CUSTOM_LINT_FIELD_SELECTORS: &[&str] = &[
    "context_schema",
    "description",
    "environments",
    "id",
    "json",
    "json.",
    "key",
    "predicates",
    "schema",
    "type",
    "value",
    "value.",
    "values",
];

pub(crate) fn completion_items(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
) -> Vec<WorkspaceCompletionItem> {
    let mut items = Vec::new();

    match completion_context(index, path, position) {
        CompletionContext::Manifest => {
            if let Some(manifest) = &index.manifest {
                items.extend(workspace_environment_completion_items(
                    &manifest.environments,
                ));
            }
        }
        CompletionContext::Qualifier => {
            items.extend(qualifier_completion_items(index));
            items.extend(predicate_operator_completion_items());
        }
        CompletionContext::Variable { environment_header } => {
            if environment_header && let Some(manifest) = &index.manifest {
                items.extend(workspace_environment_completion_items(
                    &manifest.environments,
                ));
            }
            items.extend(qualifier_completion_items(index));
            items.extend(current_variable_value_completion_items(index, path));
        }
        CompletionContext::CustomLint => {
            items.extend(custom_lint_field_selector_completion_items());
        }
        CompletionContext::Other => {}
    }

    sort_and_deduplicate_workspace_completion_items(&mut items);
    items
}

enum CompletionContext {
    Manifest,
    Qualifier,
    Variable { environment_header: bool },
    CustomLint,
    Other,
}

fn completion_context(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
) -> CompletionContext {
    if path == super::super::WORKSPACE_MANIFEST {
        return CompletionContext::Manifest;
    }
    if index
        .custom_lints
        .files
        .values()
        .any(|file| file.path == path)
    {
        return CompletionContext::CustomLint;
    }
    if index
        .qualifiers
        .values()
        .any(|qualifier| qualifier.location.path == path)
    {
        return CompletionContext::Qualifier;
    }
    if let Some(variable) = current_variable_for_path(index, path) {
        return CompletionContext::Variable {
            environment_header: position_is_environment_header(variable, path, position),
        };
    }
    CompletionContext::Other
}

fn position_is_environment_header(
    variable: &VariableNode,
    path: &str,
    position: SourcePosition,
) -> bool {
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return false;
    };
    environments.values().any(|environment| {
        location_contains_position(&environment.location, path, position)
            && environment
                .location
                .range
                .is_some_and(|range| range.start.line == position.line)
    })
}

fn workspace_environment_completion_items(
    environments: &WorkspaceEnvironmentCollection,
) -> Vec<WorkspaceCompletionItem> {
    let WorkspaceEnvironmentCollection::Environments { values, .. } = environments else {
        return Vec::new();
    };

    values
        .iter()
        .map(|environment| {
            WorkspaceCompletionItem::new(
                environment.name.clone(),
                WorkspaceCompletionItemKind::Environment,
                "workspace environment",
            )
        })
        .collect()
}

fn qualifier_completion_items(index: &SemanticIndex) -> Vec<WorkspaceCompletionItem> {
    index
        .qualifiers
        .keys()
        .map(|qualifier| {
            WorkspaceCompletionItem::new(
                qualifier.clone(),
                WorkspaceCompletionItemKind::Qualifier,
                "qualifier",
            )
        })
        .collect()
}

fn current_variable_value_completion_items(
    index: &SemanticIndex,
    path: &str,
) -> Vec<WorkspaceCompletionItem> {
    let Some(variable) = current_variable_for_path(index, path) else {
        return Vec::new();
    };

    variable
        .values
        .inline_keys
        .iter()
        .chain(variable.values.external_keys.iter())
        .map(|value| {
            WorkspaceCompletionItem::new(
                value.clone(),
                WorkspaceCompletionItemKind::Value,
                "variable value",
            )
        })
        .collect()
}

fn current_variable_for_path<'a>(index: &'a SemanticIndex, path: &str) -> Option<&'a VariableNode> {
    index
        .variables
        .values()
        .find(|variable| variable.location.path == path)
        .or_else(|| current_variable_for_external_value_path(index, path))
}

fn current_variable_for_external_value_path<'a>(
    index: &'a SemanticIndex,
    path: &str,
) -> Option<&'a VariableNode> {
    let variable_id = index
        .external_values
        .iter()
        .find_map(|(variable_id, values)| {
            values
                .values()
                .any(|value| value.location.path == path)
                .then_some(variable_id)
        })?;
    index.variables.get(variable_id)
}

fn predicate_operator_completion_items() -> Vec<WorkspaceCompletionItem> {
    PredicateOp::COMPLETION_LABELS
        .iter()
        .copied()
        .map(|op| {
            WorkspaceCompletionItem::new(
                op,
                WorkspaceCompletionItemKind::PredicateOperator,
                "predicate operator",
            )
        })
        .collect()
}

fn custom_lint_field_selector_completion_items() -> Vec<WorkspaceCompletionItem> {
    CUSTOM_LINT_FIELD_SELECTORS
        .iter()
        .copied()
        .map(|field| {
            WorkspaceCompletionItem::new(
                field,
                WorkspaceCompletionItemKind::FieldSelector,
                "custom lint field selector",
            )
        })
        .collect()
}

fn sort_and_deduplicate_workspace_completion_items(items: &mut Vec<WorkspaceCompletionItem>) {
    items.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then_with(|| {
                completion_item_kind_rank(left.kind).cmp(&completion_item_kind_rank(right.kind))
            })
            .then_with(|| left.detail.cmp(right.detail))
    });
    items.dedup_by(|left, right| {
        left.label == right.label && left.kind == right.kind && left.detail == right.detail
    });
}

fn completion_item_kind_rank(kind: WorkspaceCompletionItemKind) -> u8 {
    match kind {
        WorkspaceCompletionItemKind::Environment => 0,
        WorkspaceCompletionItemKind::Qualifier => 1,
        WorkspaceCompletionItemKind::Value => 2,
        WorkspaceCompletionItemKind::PredicateOperator => 3,
        WorkspaceCompletionItemKind::FieldSelector => 4,
    }
}
