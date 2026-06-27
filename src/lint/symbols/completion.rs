use std::collections::BTreeSet;

use serde_json::Value as JsonValue;

use super::super::PackageLintSnapshot;
use super::super::index::*;
use super::common::location_contains_position;
use super::{PackageCompletionItem, PackageCompletionItemKind};
use crate::diagnostics::SourcePosition;
use crate::expression::{Expression, ExpressionResultHint};
use crate::model::SourceKind;

struct TomlCompletionSpec {
    label: &'static str,
    detail: &'static str,
    insert_text: &'static str,
}

const QUALIFIER_TOP_LEVEL_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "schema_version",
        detail: "qualifier field",
        insert_text: "schema_version = 1",
    },
    TomlCompletionSpec {
        label: "description",
        detail: "qualifier field",
        insert_text: "description = \"\"",
    },
    TomlCompletionSpec {
        label: "when",
        detail: "qualifier field",
        insert_text: "when = \"\"",
    },
];

const VARIABLE_TOP_LEVEL_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "schema_version",
        detail: "variable field",
        insert_text: "schema_version = 1",
    },
    TomlCompletionSpec {
        label: "description",
        detail: "variable field",
        insert_text: "description = \"\"",
    },
    TomlCompletionSpec {
        label: "type",
        detail: "variable field",
        insert_text: "type = \"string\"",
    },
    TomlCompletionSpec {
        label: "[resolve]",
        detail: "variable block",
        insert_text: "[resolve]\ndefault = ",
    },
    TomlCompletionSpec {
        label: "[[resolve.rule]]",
        detail: "variable block",
        insert_text: "[[resolve.rule]]\nwhen = \"\"\nvalue = ",
    },
];

const VARIABLE_RESOLVE_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "default",
        detail: "variable field",
        insert_text: "default = ",
    },
    TomlCompletionSpec {
        label: "[[resolve.rule]]",
        detail: "variable block",
        insert_text: "[[resolve.rule]]\nwhen = \"\"\nvalue = ",
    },
];

const VARIABLE_RULE_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "when",
        detail: "variable field",
        insert_text: "when = \"\"",
    },
    TomlCompletionSpec {
        label: "query",
        detail: "variable field",
        insert_text: "query = \"\"",
    },
    TomlCompletionSpec {
        label: "value",
        detail: "variable field",
        insert_text: "value = ",
    },
    TomlCompletionSpec {
        label: "[[resolve.rule]]",
        detail: "variable block",
        insert_text: "[[resolve.rule]]\nwhen = \"\"\nvalue = ",
    },
];

const EXPRESSION_FUNCTIONS: &[&str] = &[
    "bucket",
    "cidr",
    "contains",
    "endsWith",
    "ends_with",
    "glob",
    "has",
    "matches",
    "missing",
    "path",
    "prefix",
    "present",
    "regex",
    "semver",
    "size",
    "startsWith",
    "starts_with",
    "suffix",
    "timeAfter",
    "timeAtOrAfter",
    "timeAtOrBefore",
    "timeBefore",
    "timeBetween",
    "time_after",
    "time_at_or_after",
    "time_at_or_before",
    "time_before",
    "time_between",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpressionOperator {
    And,
    Or,
}

const CUSTOM_LINT_FIELD_SELECTORS: &[&str] = &[
    "description",
    "extends",
    "id",
    "json",
    "json.",
    "key",
    "not",
    "predicates",
    "resolve",
    "schema",
    "type",
    "value",
    "value.",
    "values",
];

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
        CompletionContext::Qualifier => {
            items.extend(qualifier_field_completion_items(snapshot, path, position));
            true
        }
        CompletionContext::QualifierExpression => {
            items.extend(qualifier_completion_items(&snapshot.index));
            false
        }
        CompletionContext::Variable => {
            items.extend(variable_field_completion_items(snapshot, path, position));
            true
        }
        CompletionContext::VariableExpression => {
            items.extend(qualifier_completion_items(&snapshot.index));
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

    if preserve_order {
        deduplicate_package_completion_items_preserving_order(&mut items);
    } else {
        sort_and_deduplicate_package_completion_items(&mut items);
    }
    items
}

enum CompletionContext {
    Manifest,
    Qualifier,
    QualifierExpression,
    Variable,
    VariableExpression,
    CatalogEntry,
    CustomLint,
    Other,
}

fn completion_context(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> CompletionContext {
    if path == super::super::PACKAGE_MANIFEST {
        return CompletionContext::Manifest;
    }

    if snapshot
        .index
        .qualifiers
        .values()
        .any(|qualifier| location_contains_position(&qualifier.when.location(), path, position))
    {
        return CompletionContext::QualifierExpression;
    }
    if variable_expression_at_position(&snapshot.index, path, position) {
        return CompletionContext::VariableExpression;
    }

    match document_kind(snapshot, path) {
        Some(SourceKind::CustomLint) => return CompletionContext::CustomLint,
        Some(SourceKind::Qualifier) => return CompletionContext::Qualifier,
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
    if snapshot
        .index
        .qualifiers
        .values()
        .any(|qualifier| qualifier.location.path == path)
    {
        return CompletionContext::Qualifier;
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

fn document_kind(snapshot: &PackageLintSnapshot, path: &str) -> Option<SourceKind> {
    snapshot
        .lint
        .documents
        .iter()
        .find(|document| document.path == path)
        .map(|document| document.kind.clone())
}

fn qualifier_field_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Vec<PackageCompletionItem> {
    let context = toml_completion_context(snapshot, path, position);
    if context.table.is_some() {
        return Vec::new();
    }
    toml_completion_items(QUALIFIER_TOP_LEVEL_COMPLETIONS, &context)
}

fn variable_field_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Vec<PackageCompletionItem> {
    let context = toml_completion_context(snapshot, path, position);
    match context.table.as_deref() {
        None => toml_completion_items(VARIABLE_TOP_LEVEL_COMPLETIONS, &context),
        Some("resolve") => toml_completion_items(VARIABLE_RESOLVE_COMPLETIONS, &context),
        Some("resolve.rule") => toml_completion_items(VARIABLE_RULE_COMPLETIONS, &context),
        Some(_) => Vec::new(),
    }
}

fn toml_completion_items(
    specs: &[TomlCompletionSpec],
    context: &TomlCompletionContext,
) -> Vec<PackageCompletionItem> {
    specs
        .iter()
        .filter(|spec| toml_completion_spec_is_available(spec, context))
        .map(|spec| {
            PackageCompletionItem::new(
                spec.label,
                PackageCompletionItemKind::FieldSelector,
                spec.detail,
            )
            .with_insert_text(spec.insert_text)
        })
        .collect()
}

struct TomlCompletionContext {
    table: Option<String>,
    keys: BTreeSet<String>,
    tables: BTreeSet<String>,
}

fn toml_completion_context(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> TomlCompletionContext {
    let Some(text) = snapshot.source_text(path) else {
        return TomlCompletionContext {
            table: None,
            keys: BTreeSet::new(),
            tables: BTreeSet::new(),
        };
    };
    let lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    let tables = lines
        .iter()
        .filter_map(|line| toml_table_header(line))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let (table, start_line) = toml_table_before_position(&lines, position.line);
    let end_line = toml_table_end_line(&lines, start_line);
    let keys = lines
        .iter()
        .enumerate()
        .skip(start_line)
        .take(end_line.saturating_sub(start_line))
        .filter(|(line, _)| *line != position.line)
        .filter_map(|(_, line)| toml_key(line))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    TomlCompletionContext {
        table,
        keys,
        tables,
    }
}

fn toml_completion_spec_is_available(
    spec: &TomlCompletionSpec,
    context: &TomlCompletionContext,
) -> bool {
    if spec.label == "[[resolve.rule]]" {
        return true;
    }
    if spec.label == "[resolve]" {
        return !context.tables.contains("resolve");
    }

    let key = spec.label;
    if context.keys.contains(key) {
        return false;
    }
    true
}

fn toml_table_before_position(lines: &[&str], position_line: usize) -> (Option<String>, usize) {
    let mut table = None;
    let mut start_line = 0;
    for (line_number, line) in lines.iter().enumerate().take(position_line) {
        if let Some(header) = toml_table_header(line) {
            table = Some(header.to_owned());
            start_line = line_number + 1;
        }
    }
    (table, start_line)
}

fn toml_table_end_line(lines: &[&str], start_line: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(start_line)
        .find_map(|(line_number, line)| toml_table_header(line).map(|_| line_number))
        .unwrap_or(lines.len())
}

fn toml_table_header(line: &str) -> Option<&str> {
    let line = line.split('#').next().unwrap_or(line).trim();
    if let Some(rest) = line.strip_prefix("[[") {
        return rest
            .find("]]")
            .map(|end| rest[..end].trim())
            .filter(|name| !name.is_empty());
    }
    if let Some(rest) = line.strip_prefix('[') {
        if rest.starts_with('[') {
            return None;
        }
        return rest
            .find(']')
            .map(|end| rest[..end].trim())
            .filter(|name| !name.is_empty());
    }
    None
}

fn toml_key(line: &str) -> Option<&str> {
    let line = line.split('#').next().unwrap_or(line).trim();
    if line.starts_with('[') {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() { None } else { Some(key) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpressionKey {
    When,
    Query,
}

struct ExpressionCursor {
    key: ExpressionKey,
    prefix: String,
    token: String,
}

fn expression_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<Vec<PackageCompletionItem>> {
    let source_kind = document_kind(snapshot, path)?;
    if !matches!(source_kind, SourceKind::Qualifier | SourceKind::Variable) {
        return None;
    }

    let cursor = expression_cursor_at_position(snapshot, path, position)?;
    if matches!(source_kind, SourceKind::Qualifier) && cursor.key != ExpressionKey::When {
        return None;
    }

    if qualifier_reference_prefix(&cursor.prefix).is_some() {
        return Some(qualifier_completion_items(&snapshot.index));
    }

    if cursor.token.starts_with("context.") {
        return Some(context_path_completion_items(snapshot, &cursor.token));
    }

    if cursor.key == ExpressionKey::Query && cursor.token.starts_with("entry.") {
        return Some(entry_path_completion_items(snapshot, path, &cursor.token));
    }

    match expression_completion_state(&cursor.prefix) {
        ExpressionCompletionState::Operand => {
            let mut items = expression_root_completion_items(cursor.key == ExpressionKey::Query);
            items.extend(expression_function_completion_items());
            Some(items)
        }
        ExpressionCompletionState::LogicalOperators(operators) => {
            Some(expression_operator_completion_items(&operators))
        }
        ExpressionCompletionState::None => Some(Vec::new()),
    }
}

enum ExpressionCompletionState {
    Operand,
    LogicalOperators(Vec<ExpressionOperator>),
    None,
}

fn expression_completion_state(prefix: &str) -> ExpressionCompletionState {
    let prefix = prefix.trim_end();
    if prefix.is_empty() || expression_prefix_expects_operand(prefix) {
        return ExpressionCompletionState::Operand;
    }

    if let Some(operator) = partial_logical_operator_completion(prefix) {
        return ExpressionCompletionState::LogicalOperators(vec![operator]);
    }

    match Expression::parse(prefix) {
        Ok(expression) if expression.result_hint() == ExpressionResultHint::Bool => {
            ExpressionCompletionState::LogicalOperators(vec![
                ExpressionOperator::And,
                ExpressionOperator::Or,
            ])
        }
        Ok(_) => ExpressionCompletionState::None,
        Err(_) => ExpressionCompletionState::Operand,
    }
}

fn partial_logical_operator_completion(prefix: &str) -> Option<ExpressionOperator> {
    let (candidate, operator) = if let Some(candidate) = prefix.strip_suffix('&') {
        (candidate, ExpressionOperator::And)
    } else if let Some(candidate) = prefix.strip_suffix('|') {
        (candidate, ExpressionOperator::Or)
    } else {
        return None;
    };

    expression_prefix_is_boolean(candidate.trim_end()).then_some(operator)
}

fn expression_prefix_is_boolean(prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    Expression::parse(prefix)
        .is_ok_and(|expression| expression.result_hint() == ExpressionResultHint::Bool)
}

fn expression_prefix_expects_operand(prefix: &str) -> bool {
    prefix.ends_with("&&")
        || prefix.ends_with("||")
        || prefix.ends_with("==")
        || prefix.ends_with("!=")
        || prefix.ends_with("<=")
        || prefix.ends_with(">=")
        || prefix.ends_with('<')
        || prefix.ends_with('>')
        || prefix.ends_with('!')
        || prefix.ends_with('(')
        || prefix.ends_with('[')
        || prefix.ends_with(',')
        || expression_ends_with_word_operator(prefix, "in")
}

fn expression_ends_with_word_operator(prefix: &str, operator: &str) -> bool {
    let Some(candidate) = prefix.strip_suffix(operator) else {
        return false;
    };
    candidate
        .chars()
        .next_back()
        .is_none_or(|ch| !is_expression_token_char(ch))
}

fn expression_cursor_at_position(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<ExpressionCursor> {
    let text = snapshot.source_text(path)?;
    let line = source_line(text, position.line)?;
    let cursor = byte_index_for_character(line, position.character);
    let before_cursor = &line[..cursor];
    let equals = before_cursor.find('=')?;
    let key = expression_key_before_equals(&before_cursor[..equals])?;
    let value_prefix = &before_cursor[equals + 1..];
    let (quote_index, quote) = first_string_quote(value_prefix)?;
    let expression_prefix = &value_prefix[quote_index + quote.len_utf8()..];
    if contains_unescaped_quote(expression_prefix, quote) {
        return None;
    }

    Some(ExpressionCursor {
        key,
        prefix: expression_prefix.to_owned(),
        token: expression_token(expression_prefix).to_owned(),
    })
}

fn source_line(text: &str, line: usize) -> Option<&str> {
    text.split('\n')
        .nth(line)
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

fn byte_index_for_character(line: &str, character: usize) -> usize {
    line.char_indices()
        .nth(character)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

fn expression_key_before_equals(before_equals: &str) -> Option<ExpressionKey> {
    let key = before_equals
        .trim_end()
        .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
        .next()?;
    match key {
        "when" => Some(ExpressionKey::When),
        "query" => Some(ExpressionKey::Query),
        _ => None,
    }
}

fn first_string_quote(value_prefix: &str) -> Option<(usize, char)> {
    value_prefix
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .and_then(|(index, ch)| matches!(ch, '"' | '\'').then_some((index, ch)))
}

fn contains_unescaped_quote(value: &str, quote: char) -> bool {
    let mut escaped = false;
    for ch in value.chars() {
        if quote == '"' && escaped {
            escaped = false;
            continue;
        }
        if quote == '"' && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return true;
        }
        escaped = false;
    }
    false
}

fn expression_token(prefix: &str) -> &str {
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_expression_token_char(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    &prefix[start..]
}

fn is_expression_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

fn qualifier_reference_prefix(prefix: &str) -> Option<&str> {
    ["qualifier[\"", "qualifier['"]
        .into_iter()
        .filter_map(|needle| {
            prefix
                .rfind(needle)
                .map(|index| &prefix[index + needle.len()..])
        })
        .find(|tail| {
            !tail
                .chars()
                .any(|ch| matches!(ch, '"' | '\'' | ']' | ')' | '('))
        })
}

fn context_path_completion_items(
    snapshot: &PackageLintSnapshot,
    token: &str,
) -> Vec<PackageCompletionItem> {
    let Some(path) = token.strip_prefix("context.") else {
        return Vec::new();
    };
    let parent = parent_path_segments(path);
    let mut fields = BTreeSet::new();

    for context in snapshot.index.evaluation_contexts.values() {
        if let Some(properties) = context
            .json
            .as_ref()
            .and_then(|schema| schema_properties_at_path(schema, &parent))
        {
            fields.extend(properties.keys().cloned());
        }
    }

    path_completion_items("context", &parent, fields, "context field")
}

fn entry_path_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    token: &str,
) -> Vec<PackageCompletionItem> {
    let Some(path_suffix) = token.strip_prefix("entry.") else {
        return Vec::new();
    };
    let Some(catalog_id) = current_variable_query_catalog_id(&snapshot.index, path) else {
        return Vec::new();
    };
    let Some(catalog) = snapshot.index.catalogs.get(&catalog_id) else {
        return Vec::new();
    };
    let parent = parent_path_segments(path_suffix);
    let Some(properties) = catalog
        .json
        .as_ref()
        .and_then(|schema| schema_properties_at_path(schema, &parent))
    else {
        return Vec::new();
    };

    path_completion_items(
        "entry",
        &parent,
        properties.keys().cloned().collect(),
        "entry field",
    )
}

fn parent_path_segments(path: &str) -> Vec<&str> {
    let mut segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if !path.ends_with('.') {
        segments.pop();
    }
    segments
}

fn schema_properties_at_path<'a>(
    schema: &'a JsonValue,
    parent: &[&str],
) -> Option<&'a serde_json::Map<String, JsonValue>> {
    let mut current = schema;
    for segment in parent {
        let properties = current.get("properties").and_then(JsonValue::as_object)?;
        current = properties.get(*segment)?;
    }
    current.get("properties").and_then(JsonValue::as_object)
}

fn path_completion_items(
    root: &str,
    parent: &[&str],
    fields: BTreeSet<String>,
    detail: &'static str,
) -> Vec<PackageCompletionItem> {
    let prefix = if parent.is_empty() {
        format!("{root}.")
    } else {
        format!("{}.{}.", root, parent.join("."))
    };

    fields
        .into_iter()
        .map(|field| {
            PackageCompletionItem::new(
                format!("{prefix}{field}"),
                PackageCompletionItemKind::FieldSelector,
                detail,
            )
        })
        .collect()
}

fn current_variable_query_catalog_id(index: &SemanticIndex, path: &str) -> Option<String> {
    let variable = current_variable_for_path(index, path)?;
    let type_kind = variable_type_kind(&variable.type_source)?;
    type_kind.value.list_catalog().map(ToOwned::to_owned)
}

fn expression_root_completion_items(include_entry: bool) -> Vec<PackageCompletionItem> {
    let mut roots = vec!["context.", "qualifier[\""];
    if include_entry {
        roots.push("entry.");
    }
    roots
        .into_iter()
        .map(|root| {
            PackageCompletionItem::new(
                root,
                PackageCompletionItemKind::FieldSelector,
                "expression root",
            )
        })
        .collect()
}

fn expression_function_completion_items() -> Vec<PackageCompletionItem> {
    EXPRESSION_FUNCTIONS
        .iter()
        .copied()
        .map(|function| {
            PackageCompletionItem::new(
                format!("{function}("),
                PackageCompletionItemKind::Function,
                "expression function",
            )
        })
        .collect()
}

fn expression_operator_completion_items(
    operators: &[ExpressionOperator],
) -> Vec<PackageCompletionItem> {
    operators
        .iter()
        .map(|operator| {
            let label = match operator {
                ExpressionOperator::And => "&&",
                ExpressionOperator::Or => "||",
            };
            PackageCompletionItem::new(
                label,
                PackageCompletionItemKind::Operator,
                "expression operator",
            )
            .with_insert_text(format!("{label} "))
        })
        .collect()
}

fn catalog_entry_field_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Vec<PackageCompletionItem> {
    let Some(catalog_id) = catalog_id_for_entry_path(path) else {
        return Vec::new();
    };
    let context = toml_completion_context(snapshot, path, position);
    if context.table.is_some() {
        return Vec::new();
    }
    let Some(properties) = snapshot
        .index
        .catalogs
        .get(catalog_id)
        .and_then(|catalog| catalog.json.as_ref())
        .and_then(|json| json.get("properties"))
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };

    properties
        .keys()
        .filter(|field| !context.keys.contains(field.as_str()))
        .map(|field| {
            PackageCompletionItem::new(
                field.clone(),
                PackageCompletionItemKind::FieldSelector,
                "catalog entry field",
            )
            .with_insert_text(format!("{field} = "))
        })
        .collect()
}

fn catalog_id_for_entry_path(path: &str) -> Option<&str> {
    let path = path.strip_prefix("catalogs/")?;
    let (directory, entry) = path.split_once('/')?;
    if entry.is_empty() || !entry.ends_with(".toml") {
        return None;
    }
    directory
        .strip_suffix("-entries")
        .filter(|id| !id.is_empty())
}

fn variable_expression_at_position(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
) -> bool {
    let Some(variable) = current_variable_for_path(index, path) else {
        return false;
    };
    let ResolveNode::Resolve { rules, .. } = &variable.resolve else {
        return false;
    };
    let RuleCollection::Rules(rules) = rules else {
        return false;
    };

    rules.iter().any(|rule| {
        [rule.when.as_ref(), rule.query.as_ref()]
            .into_iter()
            .flatten()
            .any(|field| location_contains_position(&field.location(), path, position))
    })
}

fn qualifier_completion_items(index: &SemanticIndex) -> Vec<PackageCompletionItem> {
    index
        .qualifiers
        .keys()
        .map(|qualifier| {
            PackageCompletionItem::new(
                qualifier.clone(),
                PackageCompletionItemKind::Qualifier,
                "qualifier",
            )
        })
        .collect()
}

fn current_variable_value_completion_items(
    index: &SemanticIndex,
    path: &str,
) -> Vec<PackageCompletionItem> {
    let Some(variable) = current_variable_for_path(index, path) else {
        return Vec::new();
    };

    match &variable.type_source {
        TypeSourceNode::Catalog(catalog) => index
            .catalog_entries
            .get(&catalog.value)
            .into_iter()
            .flat_map(|entries| entries.keys())
            .map(|value| {
                PackageCompletionItem::new(
                    value.clone(),
                    PackageCompletionItemKind::Value,
                    "catalog value",
                )
            })
            .collect(),
        _ => variable
            .values
            .inline_values
            .keys()
            .map(|value| {
                PackageCompletionItem::new(
                    value.clone(),
                    PackageCompletionItemKind::Value,
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

fn custom_lint_field_selector_completion_items() -> Vec<PackageCompletionItem> {
    CUSTOM_LINT_FIELD_SELECTORS
        .iter()
        .copied()
        .map(|field| {
            PackageCompletionItem::new(
                field,
                PackageCompletionItemKind::FieldSelector,
                "custom lint field selector",
            )
        })
        .collect()
}

fn sort_and_deduplicate_package_completion_items(items: &mut Vec<PackageCompletionItem>) {
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

fn deduplicate_package_completion_items_preserving_order(items: &mut Vec<PackageCompletionItem>) {
    let mut seen = BTreeSet::new();
    items.retain(|item| {
        seen.insert((
            item.label.clone(),
            completion_item_kind_rank(item.kind),
            item.detail,
        ))
    });
}

fn completion_item_kind_rank(kind: PackageCompletionItemKind) -> u8 {
    match kind {
        PackageCompletionItemKind::Qualifier => 0,
        PackageCompletionItemKind::Value => 1,
        PackageCompletionItemKind::FieldSelector => 2,
        PackageCompletionItemKind::Function => 3,
        PackageCompletionItemKind::Operator => 4,
    }
}
