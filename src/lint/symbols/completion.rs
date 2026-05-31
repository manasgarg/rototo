use super::super::nodes::*;
use super::{WorkspaceCompletionItem, WorkspaceCompletionItemKind};

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

pub(crate) fn completion_items(index: &SemanticIndex, path: &str) -> Vec<WorkspaceCompletionItem> {
    let mut items = Vec::new();

    if let Some(manifest) = &index.manifest {
        items.extend(workspace_environment_completion_items(
            &manifest.environments,
        ));
    }
    items.extend(qualifier_completion_items(index));
    items.extend(current_variable_value_completion_items(index, path));
    items.extend(predicate_operator_completion_items());
    items.extend(custom_lint_field_selector_completion_items());

    sort_and_deduplicate_workspace_completion_items(&mut items);
    items
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
