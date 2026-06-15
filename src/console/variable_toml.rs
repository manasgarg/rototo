use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};

pub const PRIMITIVE_VARIABLE_TYPES: &[&str] = &["bool", "int", "number", "string", "list"];

/// Result of changing a primitive variable's default value in TOML text.
///
/// The friendly editor produces this after parsing the source file and the
/// user's input. The draft route writes `text` through the selected backend and
/// records `before`/`after` as the semantic draft change.
#[derive(Clone, Debug)]
pub struct VariableDefaultUpdate {
    pub text: String,
    pub before: JsonValue,
    pub after: JsonValue,
    pub before_literal: String,
    pub after_literal: String,
    pub value_key: String,
}

/// Parsed `[values]` entry plus enough source location to rewrite it.
///
/// This is scratch state for one save operation; it is discarded after
/// `VariableDefaultUpdate` is built.
struct ParsedValue {
    literal: String,
    value: JsonValue,
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
    default_key: Option<String>,
    values: Vec<(String, ParsedValue)>,
}

impl VariableParse {
    fn value(&self, key: &str) -> Option<&ParsedValue> {
        self.values
            .iter()
            .find(|(value_key, _)| value_key == key)
            .map(|(_, value)| value)
    }
}

pub fn update_primitive_variable_default(text: &str, value: &str) -> Result<VariableDefaultUpdate> {
    let parsed = parse_variable_file(text);
    let Some(variable_type) = parsed.variable_type.clone() else {
        return Err(RototoError::new(
            "Only primitive variables can be edited in this view.",
        ));
    };
    let Some(default_key) = parsed.default_key.clone() else {
        return Err(RototoError::new(
            "Variable does not declare a resolve default.",
        ));
    };
    let Some(existing) = parsed.value(&default_key) else {
        return Err(RototoError::new(format!(
            "Variable default value {default_key} is not declared under [values]."
        )));
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
        before: existing.value.clone(),
        after,
        before_literal: existing.literal.clone(),
        after_literal,
        value_key: default_key,
    })
}

fn parse_variable_file(text: &str) -> VariableParse {
    let mut parse = VariableParse {
        description: None,
        variable_type: None,
        default_key: None,
        values: Vec::new(),
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
            if let Some(value) = quoted_field(trimmed, "default") {
                parse.default_key = Some(value);
            }
            continue;
        }

        if section == "[values]"
            && let Some((key, literal)) = key_value_line(trimmed)
        {
            parse.values.push((
                key.clone(),
                ParsedValue {
                    value: parse_toml_literal(&literal),
                    literal,
                    line_index,
                },
            ));
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

fn parse_toml_literal(literal: &str) -> JsonValue {
    let trimmed = literal.trim();
    if trimmed.starts_with('"') {
        if let Ok(value) = serde_json::from_str::<JsonValue>(trimmed) {
            return value;
        }
        return JsonValue::String(trimmed.trim_matches('"').to_owned());
    }
    match trimmed {
        "true" => return JsonValue::Bool(true),
        "false" => return JsonValue::Bool(false),
        _ => {}
    }
    if trimmed.starts_with('[')
        && let Ok(value) = serde_json::from_str::<JsonValue>(trimmed)
    {
        return value;
    }
    let plain = trimmed.replace('_', "");
    if let Ok(int) = plain.parse::<i64>() {
        return JsonValue::from(int);
    }
    if let Ok(number) = plain.parse::<f64>()
        && number.is_finite()
        && let Some(number) = serde_json::Number::from_f64(number)
    {
        return JsonValue::Number(number);
    }
    JsonValue::String(trimmed.to_owned())
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

[values]
control = false
treatment = true

[resolve]
default = "control"

[[resolve.rule]]
qualifier = "premium-users"
value = "treatment"
"#;

    #[test]
    fn updates_only_the_default_line() {
        let update = update_primitive_variable_default(VARIABLE, "true").unwrap();
        assert_eq!(update.value_key, "control");
        assert_eq!(update.before, JsonValue::Bool(false));
        assert_eq!(update.after, JsonValue::Bool(true));
        assert!(update.text.contains("control = true"));
        assert!(update.text.contains("treatment = true"));
        assert!(update.text.contains("qualifier = \"premium-users\""));
        // Only the control line changed.
        let changed: Vec<(&str, &str)> = VARIABLE
            .lines()
            .zip(update.text.lines())
            .filter(|(before, after)| before != after)
            .collect();
        assert_eq!(changed, vec![("control = false", "control = true")]);
    }

    #[test]
    fn rejects_bad_inputs_per_type() {
        assert!(update_primitive_variable_default(VARIABLE, "definitely").is_err());
        let int_variable = VARIABLE
            .replace("type = \"bool\"", "type = \"int\"")
            .replace("control = false", "control = 4");
        assert!(update_primitive_variable_default(&int_variable, "4.5").is_err());
        let update = update_primitive_variable_default(&int_variable, "7").unwrap();
        assert!(update.text.contains("control = 7"));
    }

    #[test]
    fn non_primitive_variables_are_not_editable() {
        let schema_variable = "schema_version = 1\nschema = \"schemas/x.json\"\n";
        assert!(update_primitive_variable_default(schema_variable, "1").is_err());
    }
}
