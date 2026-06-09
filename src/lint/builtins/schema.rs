use std::collections::BTreeSet;

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
