//! Renders [`ResolveInvocation`]s into runnable `rototo resolve` command lines.
//!
//! The core correctness property is that rendering is the inverse of the CLI's
//! `--context` parser: the `--context` arguments produced here, fed back
//! through `parse_context` in `src/main.rs`, must reconstruct the original
//! context JSON exactly.

use serde_json::Value as JsonValue;

use super::{MatchedBy, ResolveExpectation, ResolveInvocation};

/// How context is rendered into the printed command.
#[derive(Clone, Copy, Debug, Default)]
pub enum ContextForm {
    /// Decompose the context object into `--context a.b=value` leaf arguments.
    #[default]
    Path,
    /// Emit a single `--context '<json>'` argument for the whole context.
    Json,
}

/// Renders the full `rototo resolve ...` command for an invocation.
pub fn render_command(
    package_source: &str,
    invocation: &ResolveInvocation,
    form: ContextForm,
) -> String {
    let mut parts = vec![
        "rototo".to_owned(),
        "resolve".to_owned(),
        shell_quote(package_source),
    ];
    parts.push(invocation.target.selector_flag().to_owned());
    parts.push(shell_quote(invocation.target.id()));
    for arg in context_args(&invocation.context, form) {
        parts.push("--context".to_owned());
        parts.push(shell_quote(&arg));
    }
    parts.join(" ")
}

/// Renders the trailing `# => ...` comment describing the expected result.
pub fn render_comment(invocation: &ResolveInvocation) -> String {
    match &invocation.expect {
        ResolveExpectation::Variable { value, matched } => {
            let value = compact(value);
            match matched {
                MatchedBy::Default => format!("# => {value} (default)"),
                MatchedBy::Rule { index, condition } => {
                    format!("# => {value} (rule {index}: {condition})")
                }
            }
        }
    }
}

fn context_args(context: &JsonValue, form: ContextForm) -> Vec<String> {
    let empty = context
        .as_object()
        .map(serde_json::Map::is_empty)
        .unwrap_or(true);
    if empty {
        return Vec::new();
    }
    match form {
        ContextForm::Json => vec![compact(context)],
        ContextForm::Path => {
            let mut leaves = Vec::new();
            collect_leaves(context, String::new(), &mut leaves);
            leaves
                .into_iter()
                .map(|(path, value)| format!("{path}={}", value_token(&value)))
                .collect()
        }
    }
}

/// Walks the context to its leaves, emitting `(dotted.path, leaf)` pairs in a
/// deterministic (sorted-key) order. Descends through non-empty objects; every
/// scalar, array, or empty object is a leaf.
fn collect_leaves(value: &JsonValue, path: String, out: &mut Vec<(String, JsonValue)>) {
    match value {
        JsonValue::Object(map) if !map.is_empty() => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                collect_leaves(&map[key], child_path, out);
            }
        }
        _ => out.push((path, value.clone())),
    }
}

/// Renders the value side of a `path=value` argument so it round-trips through
/// the resolve parser, which tries `serde_json::from_str` and falls back to a
/// raw string. A bare string is printed raw when it parses back unchanged;
/// otherwise (numbers, bools, arrays, strings that look like JSON) the value is
/// JSON-encoded.
fn value_token(value: &JsonValue) -> String {
    if let JsonValue::String(raw) = value {
        let round_trips_raw = match serde_json::from_str::<JsonValue>(raw) {
            Ok(parsed) => &parsed == value,
            Err(_) => true,
        };
        if round_trips_raw {
            return raw.clone();
        }
    }
    compact(value)
}

/// POSIX single-quote escaping. Tokens made only of safe characters are left
/// bare; anything else is wrapped in single quotes with embedded quotes escaped.
fn shell_quote(token: &str) -> String {
    let safe = !token.is_empty()
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b'/' | b'=' | b':' | b'-' | b'@' | b'+')
        });
    if safe {
        token.to_owned()
    } else {
        format!("'{}'", token.replace('\'', "'\\''"))
    }
}

fn compact(value: &JsonValue) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Mirrors the resolve CLI's context-part parsing (`context_assignment` and
    /// `parse_context_json` in `src/main.rs`) so we can assert the rendered
    /// arguments reconstruct the original context.
    fn parse_context_args(args: &[String]) -> JsonValue {
        let mut root = serde_json::Map::new();
        for arg in args {
            if arg.trim_start().starts_with('{') {
                let JsonValue::Object(map) = serde_json::from_str(arg).unwrap() else {
                    panic!("context json must be an object");
                };
                merge(&mut root, map);
                continue;
            }
            let (path, value) = arg.split_once('=').expect("path=value");
            let value =
                serde_json::from_str(value).unwrap_or_else(|_| JsonValue::String(value.to_owned()));
            insert_path(&mut root, path, value);
        }
        JsonValue::Object(root)
    }

    fn merge(
        target: &mut serde_json::Map<String, JsonValue>,
        source: serde_json::Map<String, JsonValue>,
    ) {
        for (key, value) in source {
            target.insert(key, value);
        }
    }

    fn insert_path(object: &mut serde_json::Map<String, JsonValue>, path: &str, value: JsonValue) {
        let mut segments = path.split('.').peekable();
        let mut current = object;
        while let Some(segment) = segments.next() {
            if segments.peek().is_none() {
                current.insert(segment.to_owned(), value);
                return;
            }
            let entry = current
                .entry(segment.to_owned())
                .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
            current = entry.as_object_mut().unwrap();
        }
    }

    #[test]
    fn shell_quote_leaves_safe_tokens_bare() {
        assert_eq!(shell_quote("user.tier=premium"), "user.tier=premium");
        assert_eq!(shell_quote("examples/basic"), "examples/basic");
        assert_eq!(shell_quote("max-output-tokens"), "max-output-tokens");
    }

    #[test]
    fn shell_quote_wraps_unsafe_tokens() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("tags=[\"a\"]"), "'tags=[\"a\"]'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn value_token_keeps_plain_strings_raw() {
        assert_eq!(value_token(&json!("premium")), "premium");
        assert_eq!(value_token(&json!("fixture-other")), "fixture-other");
    }

    #[test]
    fn value_token_json_encodes_ambiguous_strings() {
        assert_eq!(value_token(&json!("250")), "\"250\"");
        assert_eq!(value_token(&json!("true")), "\"true\"");
    }

    #[test]
    fn value_token_json_encodes_non_strings() {
        assert_eq!(value_token(&json!(250)), "250");
        assert_eq!(value_token(&json!(true)), "true");
        assert_eq!(value_token(&json!(["a", "b"])), "[\"a\",\"b\"]");
    }

    #[test]
    fn path_args_round_trip_through_resolve_parser() {
        let context = json!({
            "user": {"tier": "premium", "session_count": 3},
            "account": {"plan": "enterprise"},
            "flags": ["a", "b"],
            "literal_number_string": "250",
        });
        let args = context_args(&context, ContextForm::Path);
        // Deterministic, sorted ordering.
        assert_eq!(
            args,
            vec![
                "account.plan=enterprise".to_owned(),
                "flags=[\"a\",\"b\"]".to_owned(),
                "literal_number_string=\"250\"".to_owned(),
                "user.session_count=3".to_owned(),
                "user.tier=premium".to_owned(),
            ]
        );
        assert_eq!(parse_context_args(&args), context);
    }

    #[test]
    fn json_form_round_trips_through_resolve_parser() {
        let context = json!({"user": {"tier": "premium"}, "n": 3});
        let args = context_args(&context, ContextForm::Json);
        assert_eq!(args.len(), 1);
        assert_eq!(parse_context_args(&args), context);
    }

    #[test]
    fn empty_context_produces_no_args() {
        assert!(context_args(&json!({}), ContextForm::Path).is_empty());
        assert!(context_args(&json!({}), ContextForm::Json).is_empty());
    }
}
