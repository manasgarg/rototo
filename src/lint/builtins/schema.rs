use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, RototoRuleId, SemanticTarget};

use super::super::stages::push_project_diagnostic;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum JsonSchemaSimpleType {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}

impl JsonSchemaSimpleType {
    fn label(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }
}

fn schema_value_types(schema: &JsonValue) -> Option<BTreeSet<JsonSchemaSimpleType>> {
    let schema_type = schema.get("type")?;
    match schema_type {
        JsonValue::String(value) => {
            let ty = parse_schema_type(value)?;
            Some(BTreeSet::from([ty]))
        }
        JsonValue::Array(values) => {
            let mut types = BTreeSet::new();
            for value in values {
                let value = value.as_str()?;
                types.insert(parse_schema_type(value)?);
            }
            if types.is_empty() { None } else { Some(types) }
        }
        _ => None,
    }
}

fn parse_schema_type(value: &str) -> Option<JsonSchemaSimpleType> {
    match value {
        "null" => Some(JsonSchemaSimpleType::Null),
        "boolean" => Some(JsonSchemaSimpleType::Boolean),
        "integer" => Some(JsonSchemaSimpleType::Integer),
        "number" => Some(JsonSchemaSimpleType::Number),
        "string" => Some(JsonSchemaSimpleType::String),
        "array" => Some(JsonSchemaSimpleType::Array),
        "object" => Some(JsonSchemaSimpleType::Object),
        _ => None,
    }
}

fn schema_types_label(types: &BTreeSet<JsonSchemaSimpleType>) -> String {
    let labels = types.iter().map(|ty| ty.label()).collect::<Vec<_>>();
    match labels.as_slice() {
        [] => "unknown".to_owned(),
        [one] => (*one).to_owned(),
        [first, second] => format!("{first} or {second}"),
        _ => {
            let (last, rest) = labels.split_last().expect("labels is not empty");
            format!("{}, or {last}", rest.join(", "))
        }
    }
}

const UI_HINT_KEY: &str = "x-rototo-ui";

/// The pre-registered widget vocabulary shared with UI clients.
/// `ui-widgets.json` (next to this module) is the single source of truth.
fn ui_widget_vocabulary() -> &'static serde_json::Map<String, JsonValue> {
    static VOCABULARY: OnceLock<JsonValue> = OnceLock::new();
    VOCABULARY
        .get_or_init(|| {
            serde_json::from_str(include_str!("ui-widgets.json"))
                .expect("ui-widgets.json is valid JSON")
        })
        .get("widgets")
        .and_then(JsonValue::as_object)
        .expect("ui-widgets.json declares widgets")
}

fn collect_ui_hints<'a>(
    value: &'a JsonValue,
    pointer: String,
    out: &mut Vec<(String, &'a JsonValue)>,
) {
    match value {
        JsonValue::Object(object) => {
            if object.contains_key(UI_HINT_KEY) {
                out.push((pointer.clone(), value));
            }
            for (key, child) in object {
                if key == UI_HINT_KEY {
                    continue;
                }
                collect_ui_hints(child, format!("{pointer}/{key}"), out);
            }
        }
        JsonValue::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_ui_hints(child, format!("{pointer}/{index}"), out);
            }
        }
        _ => {}
    }
}

pub(super) fn lint_schema_ui_hints_for_target(
    diagnostics: &mut Vec<crate::diagnostics::LintDiagnostic>,
    target: SemanticTarget,
    location: &DiagnosticLocation,
    json: &JsonValue,
) {
    let mut hints = Vec::new();
    collect_ui_hints(json, "#".to_owned(), &mut hints);
    for (pointer, node) in hints {
        check_ui_hint(diagnostics, target.clone(), location, &pointer, node);
    }
}

fn check_ui_hint(
    diagnostics: &mut Vec<crate::diagnostics::LintDiagnostic>,
    target: SemanticTarget,
    location: &DiagnosticLocation,
    pointer: &str,
    node: &JsonValue,
) {
    let push = |diagnostics: &mut Vec<crate::diagnostics::LintDiagnostic>,
                rule: RototoRuleId,
                message: String| {
        push_project_diagnostic(diagnostics, rule, target.clone(), location.clone(), message);
    };

    let hint = node
        .get(UI_HINT_KEY)
        .expect("collected nodes contain the ui hint key");
    let Some(hint_object) = hint.as_object() else {
        push(
            diagnostics,
            RototoRuleId::SchemaUiWidgetParams,
            format!("{UI_HINT_KEY} must be an object at {pointer}"),
        );
        return;
    };
    let Some(widget_name) = hint_object.get("widget").and_then(JsonValue::as_str) else {
        push(
            diagnostics,
            RototoRuleId::SchemaUiWidgetParams,
            format!("{UI_HINT_KEY} must declare a widget string at {pointer}"),
        );
        return;
    };

    let vocabulary = ui_widget_vocabulary();
    let Some(widget) = vocabulary.get(widget_name) else {
        let known = vocabulary.keys().cloned().collect::<Vec<_>>().join(", ");
        push(
            diagnostics,
            RototoRuleId::SchemaUiUnknownWidget,
            format!("unknown ui widget {widget_name} at {pointer}; known widgets: {known}"),
        );
        return;
    };

    let allowed_types: BTreeSet<&str> = widget
        .get("types")
        .and_then(JsonValue::as_array)
        .map(|types| types.iter().filter_map(JsonValue::as_str).collect())
        .unwrap_or_default();
    if let Some(types) = schema_value_types(node)
        && !types.iter().any(|ty| allowed_types.contains(ty.label()))
    {
        let allowed = allowed_types.iter().copied().collect::<Vec<_>>().join(", ");
        push(
            diagnostics,
            RototoRuleId::SchemaUiWidgetTypeMismatch,
            format!(
                "ui widget {widget_name} supports {allowed}, but the property at {pointer} is declared as {}",
                schema_types_label(&types)
            ),
        );
    }

    let params = widget
        .get("params")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    for (key, value) in hint_object {
        if key == "widget" {
            continue;
        }
        let Some(param_type) = params.get(key).and_then(JsonValue::as_str) else {
            push(
                diagnostics,
                RototoRuleId::SchemaUiWidgetParams,
                format!(
                    "unknown {UI_HINT_KEY} parameter {key} for widget {widget_name} at {pointer}"
                ),
            );
            continue;
        };
        let valid = match param_type {
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            _ => true,
        };
        if !valid {
            push(
                diagnostics,
                RototoRuleId::SchemaUiWidgetParams,
                format!(
                    "{UI_HINT_KEY} parameter {key} for widget {widget_name} must be a {param_type} at {pointer}"
                ),
            );
        }
    }

    let requires_bounds = widget
        .get("requires_bounds")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    if requires_bounds {
        let min = hint_object
            .get("min")
            .or_else(|| node.get("minimum"))
            .filter(|value| value.is_number());
        let max = hint_object
            .get("max")
            .or_else(|| node.get("maximum"))
            .filter(|value| value.is_number());
        if min.is_none() || max.is_none() {
            push(
                diagnostics,
                RototoRuleId::SchemaUiWidgetParams,
                format!(
                    "{widget_name} widget at {pointer} needs bounds: set {UI_HINT_KEY} min and max or schema minimum and maximum"
                ),
            );
        }
    }

    let requires_enum = widget
        .get("requires_enum")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    if requires_enum {
        let has_enum = node
            .get("enum")
            .or_else(|| node.get("items").and_then(|items| items.get("enum")))
            .and_then(JsonValue::as_array)
            .is_some_and(|values| !values.is_empty());
        if !has_enum {
            push(
                diagnostics,
                RototoRuleId::SchemaUiWidgetParams,
                format!(
                    "{widget_name} widget at {pointer} requires a JSON Schema enum on the property or its items"
                ),
            );
        }
    }
}
