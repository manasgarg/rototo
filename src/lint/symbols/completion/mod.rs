use std::collections::BTreeSet;

use serde_json::Value as JsonValue;

use super::super::PackageLintSnapshot;
use super::super::index::*;
use super::common::location_contains_position;
use super::{PackageCompletionItem, PackageCompletionItemKind};
use crate::diagnostics::{SourcePosition, SourceRange};
use crate::expression::{Expression, ExpressionResultHint};
use crate::model::SourceKind;

mod arguments;
mod cursor;
mod expression;
mod references;
mod toml;

use arguments::*;
use cursor::*;
use expression::*;
use references::*;
use toml::*;

pub(crate) fn completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Vec<PackageCompletionItem> {
    let mut items = Vec::new();

    if let Some(expression_items) = expression_completion_items(snapshot, path, position) {
        items.extend(expression_items);
        sort_and_deduplicate_package_completion_items(&mut items);
        return items;
    }

    let preserve_order = match completion_context(snapshot, path, position) {
        CompletionContext::Manifest => false,
        CompletionContext::Variable => {
            items.extend(variable_field_completion_items(snapshot, path, position));
            true
        }
        CompletionContext::VariableExpression => {
            items.extend(variable_completion_items(&snapshot.index));
            items.extend(current_variable_value_completion_items(
                &snapshot.index,
                path,
            ));
            false
        }
        CompletionContext::CatalogEntry => {
            items.extend(catalog_entry_field_completion_items(
                snapshot, path, position,
            ));
            true
        }
        CompletionContext::CustomLint => {
            items.extend(custom_lint_field_selector_completion_items());
            false
        }
        CompletionContext::Other => false,
    };

    // Outside expressions the cursor sits on a TOML key or identifier; the
    // completion replaces that partial token (empty on a blank line, which makes
    // it a plain insert).
    let range = identifier_replace_range(snapshot, path, position);
    stamp_replace_range(&mut items, range);

    if preserve_order {
        deduplicate_package_completion_items_preserving_order(&mut items);
    } else {
        sort_and_deduplicate_package_completion_items(&mut items);
    }
    items
}

pub(super) fn stamp_replace_range(items: &mut [PackageCompletionItem], range: SourceRange) {
    for item in items {
        item.replace = Some(range);
    }
}

/// A zero-or-more character range on `position`'s line ending at the cursor,
/// covering the last `token_utf16_len` UTF-16 code units before it.
pub(super) fn single_line_replace_range(
    position: SourcePosition,
    token_utf16_len: usize,
) -> SourceRange {
    SourceRange {
        start: SourcePosition {
            line: position.line,
            character: position.character.saturating_sub(token_utf16_len),
        },
        end: position,
    }
}

/// The replace range for a TOML key or bare identifier: the trailing run of
/// `[A-Za-z0-9_-]` before the cursor.
pub(super) fn identifier_replace_range(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> SourceRange {
    let token = cursor_line_prefix(snapshot, path, position)
        .map(trailing_bare_key)
        .unwrap_or_default();
    single_line_replace_range(position, token.encode_utf16().count())
}

pub(super) enum CompletionContext {
    Manifest,
    Variable,
    VariableExpression,
    CatalogEntry,
    CustomLint,
    Other,
}

pub(super) fn completion_context(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> CompletionContext {
    if path == super::super::PACKAGE_MANIFEST {
        return CompletionContext::Manifest;
    }

    if variable_expression_at_position(&snapshot.index, path, position) {
        return CompletionContext::VariableExpression;
    }

    match document_kind(snapshot, path) {
        Some(SourceKind::CustomLint) => return CompletionContext::CustomLint,
        Some(SourceKind::Variable) => return CompletionContext::Variable,
        Some(SourceKind::CatalogEntry) => return CompletionContext::CatalogEntry,
        _ => {}
    }

    if snapshot
        .index
        .custom_lints
        .files
        .values()
        .any(|file| file.path == path)
    {
        return CompletionContext::CustomLint;
    }
    if let Some(variable) = current_variable_for_path(&snapshot.index, path) {
        let _ = variable;
        return CompletionContext::Variable;
    }
    if catalog_id_for_entry_path(path).is_some() {
        return CompletionContext::CatalogEntry;
    }
    CompletionContext::Other
}

pub(super) fn document_kind(snapshot: &PackageLintSnapshot, path: &str) -> Option<SourceKind> {
    snapshot
        .lint
        .documents
        .iter()
        .find(|document| document.path == path)
        .map(|document| document.kind.clone())
}

pub(super) fn sort_and_deduplicate_package_completion_items(
    items: &mut Vec<PackageCompletionItem>,
) {
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

pub(super) fn deduplicate_package_completion_items_preserving_order(
    items: &mut Vec<PackageCompletionItem>,
) {
    let mut seen = BTreeSet::new();
    items.retain(|item| {
        seen.insert((
            item.label.clone(),
            completion_item_kind_rank(item.kind),
            item.detail,
        ))
    });
}

pub(super) fn completion_item_kind_rank(kind: PackageCompletionItemKind) -> u8 {
    match kind {
        PackageCompletionItemKind::Variable => 0,
        PackageCompletionItemKind::Value => 1,
        PackageCompletionItemKind::FieldSelector => 2,
        PackageCompletionItemKind::Function => 3,
        PackageCompletionItemKind::Operator => 4,
    }
}
