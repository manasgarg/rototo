use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};

pub const PRIMITIVE_VARIABLE_TYPES: &[&str] = &["bool", "int", "number", "string", "list"];

/// Result of changing a primitive variable's default value in TOML text.
///
/// The friendly editor produces this after parsing the source file and the
/// user's input. The branch route writes `text` through the selected backend
/// and uses the literals to skip no-op writes.
#[derive(Clone, Debug)]
pub struct VariableDefaultUpdate {
    pub text: String,
    pub before_literal: String,
    pub after_literal: String,
}

/// Parsed default value plus enough source location to rewrite it.
///
/// This is scratch state for one save operation; it is discarded after
/// `VariableDefaultUpdate` is built.
struct ParsedDefault {
    literal: String,
    line_index: usize,
}

/// Minimal parse of the variable TOML needed by the friendly default editor.
///
/// It intentionally understands only the fields required for primitive default
/// replacement and does not become a second workspace parser. The full lint and
/// semantic model remain owned by rototo's Rust workspace loader.
struct VariableParse {
    description: Option<String>,
    variable_type: Option<String>,
    default: Option<ParsedDefault>,
}

pub fn update_primitive_variable_default(text: &str, value: &str) -> Result<VariableDefaultUpdate> {
    let parsed = parse_variable_file(text);
    let Some(variable_type) = parsed.variable_type.clone() else {
        return Err(RototoError::new(
            "Only primitive variables can be edited in this view.",
        ));
    };
    let Some(existing) = parsed.default else {
        return Err(RototoError::new(
            "Variable does not declare a resolve default.",
        ));
    };

    let after = parse_input_value(value, &variable_type)?;
    let after_literal = format_toml_literal(&after, &variable_type);
    let mut lines: Vec<String> = text.split(['\n']).map(str::to_owned).collect();
    let line = &lines[existing.line_index];
    let line = line.strip_suffix('\r').unwrap_or(line);
    let prefix_len = line
        .find('=')
        .map(|eq| {
            line[eq + 1..]
                .char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(at, _)| eq + 1 + at)
                .unwrap_or(line.len())
        })
        .unwrap_or(0);
    lines[existing.line_index] = format!("{}{}", &line[..prefix_len], after_literal);

    Ok(VariableDefaultUpdate {
        text: lines.join("\n"),
        before_literal: existing.literal.clone(),
        after_literal,
    })
}

fn parse_variable_file(text: &str) -> VariableParse {
    let mut parse = VariableParse {
        description: None,
        variable_type: None,
        default: None,
    };
    let mut section = String::new();

    for (line_index, line) in text.split('\n').enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            section = trimmed.to_owned();
            continue;
        }

        if section.is_empty() {
            if let Some(value) = quoted_field(trimmed, "type")
                && PRIMITIVE_VARIABLE_TYPES.contains(&value.as_str())
            {
                parse.variable_type = Some(value);
                continue;
            }
            if let Some(rest) = field_literal(trimmed, "description")
                && rest.starts_with('"')
                && let Ok(JsonValue::String(description)) = serde_json::from_str(&rest)
            {
                parse.description = Some(description);
            }
            continue;
        }

        if section == "[resolve]" {
            if let Some(literal) = field_literal(trimmed, "default") {
                parse.default = Some(ParsedDefault {
                    literal,
                    line_index,
                });
            }
            continue;
        }
    }

    parse
}

fn key_value_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return None;
    }
    let literal = value.trim();
    if literal.is_empty() {
        return None;
    }
    Some((key.to_owned(), literal.to_owned()))
}

fn field_literal(line: &str, field: &str) -> Option<String> {
    let (key, literal) = key_value_line(line)?;
    (key == field).then_some(literal)
}

fn quoted_field(line: &str, field: &str) -> Option<String> {
    let literal = field_literal(line, field)?;
    let inner = literal.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('"')).then(|| inner.to_owned())
}

fn parse_input_value(value: &str, variable_type: &str) -> Result<JsonValue> {
    let trimmed = value.trim();
    match variable_type {
        "bool" => match trimmed {
            "true" => Ok(JsonValue::Bool(true)),
            "false" => Ok(JsonValue::Bool(false)),
            _ => Err(RototoError::new("Boolean values must be true or false.")),
        },
        "int" => trimmed
            .parse::<i64>()
            .map(JsonValue::from)
            .map_err(|_| RototoError::new("Integer values must be whole numbers.")),
        "number" => {
            let number: f64 = trimmed
                .parse()
                .map_err(|_| RototoError::new("Number values must be finite."))?;
            if !number.is_finite() {
                return Err(RototoError::new("Number values must be finite."));
            }
            Ok(serde_json::Number::from_f64(number)
                .map(JsonValue::Number)
                .expect("finite numbers convert to JSON"))
        }
        "string" => Ok(JsonValue::String(trimmed.to_owned())),
        "list" => {
            let parsed: JsonValue = serde_json::from_str(trimmed)
                .map_err(|_| RototoError::new("List values must be a JSON array."))?;
            if !parsed.is_array() {
                return Err(RototoError::new("List values must be a JSON array."));
            }
            Ok(parsed)
        }
        other => Err(RototoError::new(format!(
            "unsupported primitive variable type: {other}"
        ))),
    }
}

fn format_toml_literal(value: &JsonValue, variable_type: &str) -> String {
    match variable_type {
        "bool" => {
            if value.as_bool().unwrap_or(false) {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        "int" | "number" => match value {
            JsonValue::Number(number) => number.to_string(),
            other => other.to_string(),
        },
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIABLE: &str = r#"schema_version = 1

description = "Shows the launch banner"
type = "bool"

[resolve]
default = false

[[resolve.rule]]
qualifier = "premium-users"
value = true
"#;

    #[test]
    fn updates_only_the_default_line() {
        let update = update_primitive_variable_default(VARIABLE, "true").unwrap();
        assert_eq!(update.before_literal, "false");
        assert_eq!(update.after_literal, "true");
        assert!(update.text.contains("default = true"));
        assert!(update.text.contains("value = true"));
        assert!(update.text.contains("qualifier = \"premium-users\""));
        // Only the default line changed.
        let changed: Vec<(&str, &str)> = VARIABLE
            .lines()
            .zip(update.text.lines())
            .filter(|(before, after)| before != after)
            .collect();
        assert_eq!(changed, vec![("default = false", "default = true")]);
    }

    #[test]
    fn rejects_bad_inputs_per_type() {
        assert!(update_primitive_variable_default(VARIABLE, "definitely").is_err());
        let int_variable = VARIABLE
            .replace("type = \"bool\"", "type = \"int\"")
            .replace("default = false", "default = 4");
        assert!(update_primitive_variable_default(&int_variable, "4.5").is_err());
        let update = update_primitive_variable_default(&int_variable, "7").unwrap();
        assert!(update.text.contains("default = 7"));
    }

    #[test]
    fn non_primitive_variables_are_not_editable() {
        let schema_variable = "schema_version = 1\nschema = \"schemas/x.json\"\n";
        assert!(update_primitive_variable_default(schema_variable, "1").is_err());
    }
}
