use super::*;

pub(super) fn cursor_line_prefix<'a>(
    snapshot: &'a PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<&'a str> {
    let text = snapshot.source_text(path)?;
    let line = source_line(text, position.line)?;
    let cursor = byte_index_for_utf16_column(line, position.character);
    Some(&line[..cursor])
}

pub(super) fn trailing_bare_key(prefix: &str) -> &str {
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    &prefix[start..]
}

/// The trailing run of `&`/`|` before the cursor, which an operator completion
/// replaces (empty when the cursor follows whitespace).
pub(super) fn trailing_operator_token(prefix: &str) -> &str {
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !matches!(ch, '&' | '|'))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    &prefix[start..]
}

pub(super) fn expression_cursor_at_position(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Option<ExpressionCursor> {
    let text = snapshot.source_text(path)?;
    let line = source_line(text, position.line)?;
    let cursor = byte_index_for_utf16_column(line, position.character);
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

pub(super) fn source_line(text: &str, line: usize) -> Option<&str> {
    text.split('\n')
        .nth(line)
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// Map an LSP character offset (UTF-16 code units) to a byte index in `line`.
pub(super) fn byte_index_for_utf16_column(line: &str, column: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in line.char_indices() {
        if utf16 >= column {
            return byte;
        }
        utf16 += ch.len_utf16();
    }
    line.len()
}

pub(super) fn expression_key_before_equals(before_equals: &str) -> Option<ExpressionKey> {
    let key = before_equals
        .trim_end()
        .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
        .next()?;
    match key {
        "when" => Some(ExpressionKey::When),
        "filter" | "sort" => Some(ExpressionKey::Query),
        _ => None,
    }
}

pub(super) fn first_string_quote(value_prefix: &str) -> Option<(usize, char)> {
    value_prefix
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .and_then(|(index, ch)| matches!(ch, '"' | '\'').then_some((index, ch)))
}

pub(super) fn contains_unescaped_quote(value: &str, quote: char) -> bool {
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

pub(super) fn expression_token(prefix: &str) -> &str {
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_expression_token_char(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    &prefix[start..]
}

pub(super) fn is_expression_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

pub(super) fn variable_reference_prefix(prefix: &str) -> Option<&str> {
    ["variables[\"", "variables['"]
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

/// The dotted path on the left of the comparison the cursor's operand answers:
/// for `context.account.plan == <cursor>` this is `("context",
/// ["account", "plan"])`. Only equality and membership comparisons pin a
/// closed set of literals.
pub(super) fn comparison_lhs_path<'a>(
    prefix: &'a str,
    token: &str,
) -> Option<(&'a str, Vec<&'a str>)> {
    let before_token = prefix.strip_suffix(token).unwrap_or(prefix).trim_end();
    let lhs = if let Some(lhs) = before_token
        .strip_suffix("==")
        .or_else(|| before_token.strip_suffix("!="))
    {
        lhs
    } else if let Some(lhs) = before_token.strip_suffix("in")
        && lhs.ends_with(|ch: char| ch.is_whitespace())
    {
        lhs
    } else {
        return None;
    };
    let lhs = lhs.trim_end();
    let start = lhs
        .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'))
        .map(|index| index + 1)
        .unwrap_or(0);
    let mut parts = lhs[start..].split('.');
    let root = parts.next()?;
    if !matches!(root, "context" | "entry") {
        return None;
    }
    let segments: Vec<&str> = parts.collect();
    if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    Some((root, segments))
}

/// The innermost unclosed function call the cursor's prefix sits inside, with
/// the zero-based index of the argument under the cursor. `None` when the
/// cursor is not inside a call, the call has no bare-identifier callee, or
/// the cursor sits inside a string literal.
pub(super) fn enclosing_call_argument(prefix: &str) -> Option<(String, usize)> {
    let mut stack: Vec<(Option<String>, usize)> = Vec::new();
    let mut in_string: Option<char> = None;
    let mut last_token: Option<(usize, usize)> = None;
    for (index, ch) in prefix.char_indices() {
        if let Some(quote) = in_string {
            if ch == quote {
                in_string = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => in_string = Some(ch),
            '(' => {
                let name = last_token
                    .map(|(start, end)| &prefix[start..end])
                    .filter(|name| !name.contains('.'))
                    .map(str::to_owned);
                stack.push((name, 0));
                last_token = None;
            }
            ')' => {
                stack.pop();
                last_token = None;
            }
            ',' => {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                last_token = None;
            }
            ch if is_expression_token_char(ch) => {
                last_token = match last_token {
                    Some((start, end)) if end == index => Some((start, index + ch.len_utf8())),
                    _ => Some((index, index + ch.len_utf8())),
                };
            }
            ch if ch.is_whitespace() => {}
            _ => last_token = None,
        }
    }
    if in_string.is_some() {
        return None;
    }
    let (name, argument) = stack.pop()?;
    Some((name?, argument))
}
