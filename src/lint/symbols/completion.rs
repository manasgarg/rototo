use super::super::index::*;
use super::{WorkspaceCompletionItem, WorkspaceCompletionItemKind};
use crate::diagnostics::SourcePosition;

const CUSTOM_LINT_FIELD_SELECTORS: &[&str] = &[
    "description",
    "extends",
    "id",
    "json",
    "json.",
    "key",
    "predicates",
    "resolve",
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
        CompletionContext::Manifest => {}
        CompletionContext::Qualifier => {
            items.extend(qualifier_completion_items(index));
            items.extend(predicate_operator_completion_items());
        }
        CompletionContext::Variable => {
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
    Variable,
    CustomLint,
    Other,
}

fn completion_context(
    index: &SemanticIndex,
    path: &str,
    _position: SourcePosition,
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
        let _ = variable;
        return CompletionContext::Variable;
    }
    CompletionContext::Other
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

    match &variable.type_source {
        TypeSourceNode::Resource(resource) => index
            .resource_objects
            .get(&resource.value)
            .into_iter()
            .flat_map(|objects| objects.keys())
            .map(|value| {
                WorkspaceCompletionItem::new(
                    value.clone(),
                    WorkspaceCompletionItemKind::Value,
                    "resource object",
                )
            })
            .collect(),
        _ => variable
            .values
            .inline_values
            .keys()
            .map(|value| {
                WorkspaceCompletionItem::new(
                    value.clone(),
                    WorkspaceCompletionItemKind::Value,
                    "variable value",
                )
            })
            .collect(),
    }
}

fn current_variable_for_path<'a>(index: &'a SemanticIndex, path: &str) -> Option<&'a VariableNode> {
    index
        .variables
        .values()
        .find(|variable| variable.location.path == path)
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
        WorkspaceCompletionItemKind::Qualifier => 0,
        WorkspaceCompletionItemKind::Value => 1,
        WorkspaceCompletionItemKind::PredicateOperator => 2,
        WorkspaceCompletionItemKind::FieldSelector => 3,
    }
}
