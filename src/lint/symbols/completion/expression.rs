use super::*;

/// The functions completion advertises. These are the single canonical
/// camelCase spellings of rototo's expression surface plus the CEL `has` macro.
/// The evaluator also accepts snake_case and shorthand aliases
/// (`starts_with`, `prefix`, `regex`, `time_before`, …), but suggesting every
/// alias is the "too eager / odd suggestions" smell, so completion offers only
/// the documented spelling.
pub(super) const EXPRESSION_FUNCTIONS: &[&str] = &[
    "bucket",
    "cidr",
    "contains",
    "endsWith",
    "glob",
    "has",
    "matches",
    "missing",
    "path",
    "present",
    "semver",
    "size",
    "startsWith",
    "timeAfter",
    "timeAtOrAfter",
    "timeAtOrBefore",
    "timeBefore",
    "timeBetween",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExpressionOperator {
    And,
    Or,
    Equals,
    NotEquals,
    Less,
    LessEquals,
    Greater,
    GreaterEquals,
    In,
}

/// Operators offered after a complete boolean operand — the expression can only
/// be continued by composing it with another boolean.
pub(super) const LOGICAL_OPERATORS: &[ExpressionOperator] =
    &[ExpressionOperator::And, ExpressionOperator::Or];

/// Operators offered after a complete value operand (a path, literal, or
/// value-returning function) — the natural next step is to compare it.
pub(super) const COMPARISON_OPERATORS: &[ExpressionOperator] = &[
    ExpressionOperator::Equals,
    ExpressionOperator::NotEquals,
    ExpressionOperator::Less,
    ExpressionOperator::LessEquals,
    ExpressionOperator::Greater,
    ExpressionOperator::GreaterEquals,
    ExpressionOperator::In,
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ExpressionKey {
    When,
    Query,
}

pub(super) struct ExpressionCursor {
    pub(super) key: ExpressionKey,
    pub(super) prefix: String,
    pub(super) token: String,
}

pub(super) fn expression_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<Vec<PackageCompletionItem>> {
    let source_kind = document_kind(snapshot, path)?;
    if !matches!(source_kind, SourceKind::Variable) {
        return None;
    }

    let cursor = expression_cursor_at_position(snapshot, path, position)?;

    // The path, variable-reference, and operand completions all replace the
    // dotted/identifier token under the cursor; only the operator completions
    // replace a trailing `&`/`|` instead.
    let token_range =
        |token: &str| single_line_replace_range(position, token.encode_utf16().count());

    if variable_reference_prefix(&cursor.prefix).is_some() {
        let mut items = variable_completion_items(&snapshot.index);
        stamp_replace_range(&mut items, token_range(&cursor.token));
        return Some(items);
    }

    if cursor.token.starts_with("env.") {
        let mut items = env_member_completion_items();
        stamp_replace_range(&mut items, token_range(&cursor.token));
        return Some(items);
    }

    if cursor.token.starts_with("lists.") {
        let mut items = list_reference_completion_items(&snapshot.index);
        stamp_replace_range(&mut items, token_range(&cursor.token));
        return Some(items);
    }

    if cursor.token.starts_with("context.") {
        let mut items = context_path_completion_items(snapshot, &cursor.token);
        stamp_replace_range(&mut items, token_range(&cursor.token));
        return Some(items);
    }

    if cursor.key == ExpressionKey::Query && cursor.token.starts_with("entry.") {
        let mut items = entry_path_completion_items(snapshot, path, &cursor.token);
        stamp_replace_range(&mut items, token_range(&cursor.token));
        return Some(items);
    }

    match expression_completion_state(&cursor.prefix) {
        ExpressionCompletionState::Operand => {
            let mut items = call_argument_completion_items(&snapshot.index, &cursor.prefix);
            items.extend(typed_operand_completion_items(snapshot, path, &cursor));
            items.extend(expression_root_completion_items(
                cursor.key == ExpressionKey::Query,
            ));
            items.extend(expression_function_completion_items());
            stamp_replace_range(&mut items, token_range(&cursor.token));
            Some(items)
        }
        ExpressionCompletionState::Operators(operators) => {
            let mut items = expression_operator_completion_items(&operators);
            stamp_replace_range(
                &mut items,
                token_range(trailing_operator_token(&cursor.prefix)),
            );
            Some(items)
        }
    }
}

pub(super) enum ExpressionCompletionState {
    Operand,
    Operators(Vec<ExpressionOperator>),
}

pub(super) fn expression_completion_state(raw_prefix: &str) -> ExpressionCompletionState {
    let prefix = raw_prefix.trim_end();
    if prefix.is_empty() || expression_prefix_expects_operand(prefix) {
        return ExpressionCompletionState::Operand;
    }

    // The user is mid-typing an operand or function name only when the cursor
    // sits directly after an identifier character, with no separating
    // whitespace. cel parses a bare/partial identifier as a valid expression, so
    // a parse error can't reveal this; the raw cursor boundary can. When there
    // *is* trailing whitespace the operand is complete, so fall through to the
    // operator suggestions instead of re-offering operands.
    if raw_prefix.ends_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_') {
        return ExpressionCompletionState::Operand;
    }

    if let Some(operator) = partial_logical_operator_completion(prefix) {
        return ExpressionCompletionState::Operators(vec![operator]);
    }

    // A complete operand decides what can follow from its result type: a boolean
    // composes with `&&`/`||`, a value invites a comparison.
    match Expression::parse(prefix) {
        Ok(expression) => match expression.result_hint() {
            ExpressionResultHint::Bool => {
                ExpressionCompletionState::Operators(LOGICAL_OPERATORS.to_vec())
            }
            ExpressionResultHint::Value => {
                ExpressionCompletionState::Operators(COMPARISON_OPERATORS.to_vec())
            }
        },
        Err(_) => ExpressionCompletionState::Operand,
    }
}

pub(super) fn partial_logical_operator_completion(prefix: &str) -> Option<ExpressionOperator> {
    let (candidate, operator) = if let Some(candidate) = prefix.strip_suffix('&') {
        (candidate, ExpressionOperator::And)
    } else if let Some(candidate) = prefix.strip_suffix('|') {
        (candidate, ExpressionOperator::Or)
    } else {
        return None;
    };

    expression_prefix_is_boolean(candidate.trim_end()).then_some(operator)
}

pub(super) fn expression_prefix_is_boolean(prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    Expression::parse(prefix)
        .is_ok_and(|expression| expression.result_hint() == ExpressionResultHint::Bool)
}

pub(super) fn expression_prefix_expects_operand(prefix: &str) -> bool {
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

pub(super) fn expression_ends_with_word_operator(prefix: &str, operator: &str) -> bool {
    let Some(candidate) = prefix.strip_suffix(operator) else {
        return false;
    };
    candidate
        .chars()
        .next_back()
        .is_none_or(|ch| !is_expression_token_char(ch))
}

pub(super) fn expression_root_completion_items(include_entry: bool) -> Vec<PackageCompletionItem> {
    let mut roots = vec!["context.", "env.", "lists."];
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

/// The declared lists, referenced through the `lists` root as their member
/// lists. Namespaced ids need the bracket spelling; plain ids read as dot
/// access.
pub(super) fn list_reference_completion_items(
    index: &crate::lint::index::SemanticIndex,
) -> Vec<PackageCompletionItem> {
    index
        .lists
        .keys()
        .map(|id| {
            let label = if id.contains('/') {
                format!("lists[\"{id}\"]")
            } else {
                format!("lists.{id}")
            };
            PackageCompletionItem::new(
                label,
                PackageCompletionItemKind::FieldSelector,
                "list members",
            )
        })
        .collect()
}

/// Members of the `env` root: the evaluation timestamp.
pub(super) fn env_member_completion_items() -> Vec<PackageCompletionItem> {
    ["env.now"]
        .into_iter()
        .map(|member| {
            PackageCompletionItem::new(
                member,
                PackageCompletionItemKind::FieldSelector,
                "env member",
            )
        })
        .collect()
}

pub(super) fn expression_function_completion_items() -> Vec<PackageCompletionItem> {
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

pub(super) fn expression_operator_completion_items(
    operators: &[ExpressionOperator],
) -> Vec<PackageCompletionItem> {
    operators
        .iter()
        .map(|operator| {
            let label = match operator {
                ExpressionOperator::And => "&&",
                ExpressionOperator::Or => "||",
                ExpressionOperator::Equals => "==",
                ExpressionOperator::NotEquals => "!=",
                ExpressionOperator::Less => "<",
                ExpressionOperator::LessEquals => "<=",
                ExpressionOperator::Greater => ">",
                ExpressionOperator::GreaterEquals => ">=",
                ExpressionOperator::In => "in",
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

pub(super) fn variable_expression_at_position(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
) -> bool {
    let Some(variable) = current_variable_for_path(index, path) else {
        return false;
    };
    let ResolveNode::Resolve { rules, query, .. } = &variable.resolve else {
        return false;
    };

    if let Some(query) = query
        && [&query.filter, &query.sort]
            .into_iter()
            .flatten()
            .any(|field| location_contains_position(&field.location(), path, position))
    {
        return true;
    }

    let RuleCollection::Rules(rules) = rules else {
        return false;
    };

    rules.iter().any(|rule| {
        rule.when
            .as_ref()
            .is_some_and(|field| location_contains_position(&field.location(), path, position))
    })
}
