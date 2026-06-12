use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde_json::Value as JsonValue;

use crate::diagnostics::{
    DiagnosticLocation, RelatedLocation, RototoRuleId, SemanticEntity, SemanticField,
    SemanticTarget, Severity,
};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::stages::{push_project_diagnostic, push_reference_diagnostic};

const CONTEXT_SCHEMA_PATH: &str = "schemas/context.schema.json";

struct ContextSchemaError {
    location: DiagnosticLocation,
    message: String,
}

pub(super) fn lint_context_schema_reference(ctx: &mut LintContext) {
    let Err(err) = valid_context_schema(ctx) else {
        return;
    };

    push_project_diagnostic(
        &mut ctx.diagnostics,
        RototoRuleId::WorkspaceContextSchemaRef,
        SemanticTarget::field(
            SemanticEntity::Schema {
                path: CONTEXT_SCHEMA_PATH.to_owned(),
            },
            SemanticField::SchemaJson,
        ),
        err.location,
        err.message,
    );
}

pub(super) fn lint_context_schema_reserved_fields(ctx: &mut LintContext) {
    let Ok(Some(schema)) = valid_context_schema(ctx) else {
        return;
    };
    let Some(schema_json) = schema.json.as_ref() else {
        return;
    };
    if !context_schema_declares_top_level_property(schema_json, "qualifier") {
        return;
    }
    let schema_target = schema.field_target(SemanticField::SchemaJson);
    let schema_location = schema.location.clone();

    push_project_diagnostic(
        &mut ctx.diagnostics,
        RototoRuleId::WorkspaceContextSchemaReservedField,
        schema_target,
        schema_location,
        "context schema declares reserved top-level field: qualifier",
    );
}

pub(super) fn lint_qualifier_context_schema_attributes(ctx: &mut LintContext) {
    let Ok(Some(schema)) = valid_context_schema(ctx) else {
        return;
    };

    let mut diagnostics = Vec::new();
    for edge in ctx.references.edges() {
        let ReferenceSource::QualifierPredicateContextAttribute { .. } = &edge.source else {
            continue;
        };
        let ReferenceTarget::ContextAttribute(attribute) = &edge.target else {
            continue;
        };
        let Some(schema_json) = schema.json.as_ref() else {
            continue;
        };
        if context_schema_declares_path(schema_json, attribute) {
            continue;
        }

        push_reference_diagnostic(
            &mut diagnostics,
            RototoRuleId::WorkspaceContextSchemaAttribute,
            edge.semantic_target.clone(),
            edge.location.clone(),
            format!("context attribute is not declared by the context schema: {attribute}"),
        );
    }
    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_qualifier_context_schema_types(ctx: &mut LintContext) {
    let Ok(Some(schema)) = valid_context_schema(ctx) else {
        return;
    };
    let Some(schema_json) = schema.json.as_ref() else {
        return;
    };

    let mut diagnostics = Vec::new();
    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if attribute.value.starts_with("qualifier.") {
                continue;
            }
            let Some(attribute_schema) = context_schema_field(schema_json, &attribute.value) else {
                continue;
            };
            let Some(schema_types) = schema_value_types(attribute_schema) else {
                continue;
            };
            let ProjectField::Present(op) = &predicate.op else {
                continue;
            };
            if matches!(op.value, PredicateOp::Unknown(_)) {
                continue;
            }

            let Some((field, location, message)) = predicate_context_type_mismatch(
                &attribute.value,
                &op.value,
                predicate,
                &schema_types,
            ) else {
                continue;
            };

            push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::QualifierPredicateContextTypeMismatch,
                predicate.field_target(&qualifier.id, field),
                location,
                message,
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_missing_context_schema_for_qualifier_attributes(ctx: &mut LintContext) {
    if ctx.index.schemas.contains_key(CONTEXT_SCHEMA_PATH) {
        return;
    }
    if ctx
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return;
    }
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };

    let referenced = ctx.references.referenced_qualifier_ids();
    let mut diagnostics = Vec::new();
    let first_context_attribute = ctx.references.edges().iter().find_map(|edge| {
        let ReferenceSource::QualifierPredicateContextAttribute { qualifier, .. } = &edge.source
        else {
            return None;
        };
        if !referenced.contains(qualifier) || qualifier_has_existing_error(ctx, qualifier) {
            return None;
        }
        let ReferenceTarget::ContextAttribute(attribute) = &edge.target else {
            return None;
        };
        Some((attribute, edge))
    });
    let Some((attribute, edge)) = first_context_attribute else {
        return;
    };

    push_reference_diagnostic(
        &mut diagnostics,
        RototoRuleId::WorkspaceContextSchemaMissing,
        manifest.target(),
        manifest.location.clone(),
        format!(
            "workspace does not declare {CONTEXT_SCHEMA_PATH} for qualifier context attribute: {attribute}"
        ),
    );
    if let Some(diagnostic) = diagnostics.last_mut() {
        diagnostic.related.push(RelatedLocation {
            location: edge.location.clone(),
            message: format!("context attribute read here: {attribute}"),
        });
    }
    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_has_existing_error(ctx: &LintContext, qualifier_id: &str) -> bool {
    ctx.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == Severity::Error
            && match &diagnostic.target.entity {
                SemanticEntity::Qualifier { id } => id == qualifier_id,
                SemanticEntity::Predicate { qualifier, .. } => qualifier == qualifier_id,
                _ => false,
            }
    })
}

pub(super) fn lint_unreferenced_schemas(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for schema in ctx.index.schemas.values() {
        if schema.json.is_none() || schema.invalid_message.is_some() {
            continue;
        }
        if schema.path == CONTEXT_SCHEMA_PATH {
            continue;
        }
        let target = ReferenceTarget::Schema(schema.path.clone());
        if ctx.references.has_references(&target) {
            continue;
        }
        push_reference_diagnostic(
            &mut diagnostics,
            RototoRuleId::SchemaUnreferenced,
            schema.target(),
            schema.location.clone(),
            format!("schema is not referenced: {}", schema.path),
        );
    }
    ctx.diagnostics.extend(diagnostics);
}

fn valid_context_schema(
    ctx: &LintContext,
) -> std::result::Result<Option<&SchemaNode>, Box<ContextSchemaError>> {
    let Some(schema) = ctx.index.schemas.get(CONTEXT_SCHEMA_PATH) else {
        return Ok(None);
    };

    if schema.json.is_none() {
        return Err(Box::new(ContextSchemaError {
            location: schema.location.clone(),
            message: format!("context schema file could not be parsed: {CONTEXT_SCHEMA_PATH}"),
        }));
    };
    if schema.validator.is_none() {
        return Err(Box::new(ContextSchemaError {
            location: schema.location.clone(),
            message: format!(
                "context schema is invalid: {}",
                schema
                    .invalid_message
                    .as_deref()
                    .unwrap_or("schema did not compile")
            ),
        }));
    }

    Ok(Some(schema))
}

fn context_schema_declares_path(schema: &JsonValue, attribute: &str) -> bool {
    context_schema_field(schema, attribute).is_some()
}

fn context_schema_declares_top_level_property(schema: &JsonValue, property: &str) -> bool {
    schema
        .get("properties")
        .and_then(JsonValue::as_object)
        .is_some_and(|properties| properties.contains_key(property))
}

fn context_schema_field<'a>(schema: &'a JsonValue, attribute: &str) -> Option<&'a JsonValue> {
    if attribute.is_empty() {
        return None;
    }

    let mut current = schema;
    for segment in attribute.split('.') {
        let properties = current.get("properties").and_then(JsonValue::as_object)?;
        let next = properties.get(segment)?;
        current = next;
    }
    Some(current)
}

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

fn predicate_context_type_mismatch(
    attribute: &str,
    op: &PredicateOp,
    predicate: &PredicateNode,
    schema_types: &BTreeSet<JsonSchemaSimpleType>,
) -> Option<(SemanticField, DiagnosticLocation, String)> {
    match op {
        PredicateOp::Eq | PredicateOp::Neq => {
            let value = predicate.value.as_ref()?;
            if value_is_compatible_with_schema(&value.value, schema_types) {
                return None;
            }
            Some((
                SemanticField::PredicateValue,
                value.location.clone(),
                format!(
                    "{} predicate value type {} is incompatible with context attribute {attribute} declared as {}",
                    op.as_str(),
                    json_value_type(&value.value).label(),
                    schema_types_label(schema_types)
                ),
            ))
        }
        PredicateOp::In | PredicateOp::NotIn => {
            let value = predicate.value.as_ref()?;
            let values = value.value.as_array()?;
            let incompatible = values
                .iter()
                .find(|value| !value_is_compatible_with_schema(value, schema_types))?;
            Some((
                SemanticField::PredicateValue,
                value.location.clone(),
                format!(
                    "{} predicate contains value type {} incompatible with context attribute {attribute} declared as {}",
                    op.as_str(),
                    json_value_type(incompatible).label(),
                    schema_types_label(schema_types)
                ),
            ))
        }
        PredicateOp::Gt | PredicateOp::Gte | PredicateOp::Lt | PredicateOp::Lte => {
            if schema_types.iter().all(|ty| {
                matches!(
                    ty,
                    JsonSchemaSimpleType::Integer | JsonSchemaSimpleType::Number
                )
            }) {
                return None;
            }
            Some((
                SemanticField::PredicateOp,
                predicate.op.location(),
                format!(
                    "{} predicate requires numeric context attribute, but {attribute} is declared as {}",
                    op.as_str(),
                    schema_types_label(schema_types)
                ),
            ))
        }
        PredicateOp::Bucket => {
            if schema_types.iter().all(|ty| {
                matches!(
                    ty,
                    JsonSchemaSimpleType::Boolean
                        | JsonSchemaSimpleType::Integer
                        | JsonSchemaSimpleType::Number
                        | JsonSchemaSimpleType::String
                )
            }) {
                return None;
            }
            Some((
                SemanticField::PredicateOp,
                predicate.op.location(),
                format!(
                    "bucket predicate requires scalar context attribute, but {attribute} is declared as {}",
                    schema_types_label(schema_types)
                ),
            ))
        }
        PredicateOp::Unknown(_) => None,
    }
}

fn value_is_compatible_with_schema(
    value: &JsonValue,
    schema_types: &BTreeSet<JsonSchemaSimpleType>,
) -> bool {
    let value_type = json_value_type(value);
    match value_type {
        JsonSchemaSimpleType::Integer => schema_types.iter().any(|ty| {
            matches!(
                ty,
                JsonSchemaSimpleType::Integer | JsonSchemaSimpleType::Number
            )
        }),
        ty => schema_types.contains(&ty),
    }
}

fn json_value_type(value: &JsonValue) -> JsonSchemaSimpleType {
    match value {
        JsonValue::Null => JsonSchemaSimpleType::Null,
        JsonValue::Bool(_) => JsonSchemaSimpleType::Boolean,
        JsonValue::Number(number) if number.is_i64() || number.is_u64() => {
            JsonSchemaSimpleType::Integer
        }
        JsonValue::Number(_) => JsonSchemaSimpleType::Number,
        JsonValue::String(_) => JsonSchemaSimpleType::String,
        JsonValue::Array(_) => JsonSchemaSimpleType::Array,
        JsonValue::Object(_) => JsonSchemaSimpleType::Object,
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
/// `spec/ui-widgets.json` is the single source of truth.
fn ui_widget_vocabulary() -> &'static serde_json::Map<String, JsonValue> {
    static VOCABULARY: OnceLock<JsonValue> = OnceLock::new();
    VOCABULARY
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../spec/ui-widgets.json"))
                .expect("spec/ui-widgets.json is valid JSON")
        })
        .get("widgets")
        .and_then(JsonValue::as_object)
        .expect("spec/ui-widgets.json declares widgets")
}

pub(super) fn lint_schema_ui_hints(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for schema in ctx.index.schemas.values() {
        if schema.invalid_message.is_some() {
            continue;
        }
        let Some(json) = schema.json.as_ref() else {
            continue;
        };
        let mut hints = Vec::new();
        collect_ui_hints(json, "#".to_owned(), &mut hints);
        for (pointer, node) in hints {
            check_ui_hint(&mut diagnostics, schema, &pointer, node);
        }
    }
    ctx.diagnostics.extend(diagnostics);
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

fn check_ui_hint(
    diagnostics: &mut Vec<crate::diagnostics::LintDiagnostic>,
    schema: &SchemaNode,
    pointer: &str,
    node: &JsonValue,
) {
    let push = |diagnostics: &mut Vec<crate::diagnostics::LintDiagnostic>,
                rule: RototoRuleId,
                message: String| {
        push_project_diagnostic(
            diagnostics,
            rule,
            schema.field_target(SemanticField::SchemaJson),
            schema.location.clone(),
            message,
        );
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
                    "{widget_name} widget at {pointer} requires an enum on the property or its items"
                ),
            );
        }
    }
}

pub(super) fn lint_schema_documents(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for schema in ctx.index.schemas.values() {
        let Some(message) = &schema.invalid_message else {
            continue;
        };
        push_project_diagnostic(
            &mut diagnostics,
            RototoRuleId::SchemaInvalid,
            schema.field_target(SemanticField::SchemaJson),
            schema.location.clone(),
            format!("schema is invalid: {message}"),
        );
    }
    ctx.diagnostics.extend(diagnostics);
}
