use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, EntityId, RototoRuleId};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::{ReferenceSource, ReferenceTarget};
use super::super::source::resolve_workspace_root_path;
use super::super::stages::{push_project_diagnostic, push_reference_diagnostic};

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
        EntityId::Manifest,
        err.location,
        err.message,
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
            edge.entity.clone(),
            edge.location.clone(),
            format!("context attribute is not declared by the context schema: {attribute}"),
        );
    }
    ctx.diagnostics.extend(diagnostics);
}

fn valid_context_schema(
    ctx: &LintContext,
) -> std::result::Result<Option<&SchemaNode>, Box<ContextSchemaError>> {
    let Some(manifest) = &ctx.index.manifest else {
        return Ok(None);
    };
    let Some(context) = &manifest.context_schema else {
        return Ok(None);
    };

    if context.invalid_shape {
        return Err(Box::new(ContextSchemaError {
            location: context.location.clone(),
            message: "[context] must be a table".to_owned(),
        }));
    }

    let ProjectField::Present(schema_ref) = &context.schema else {
        return Err(Box::new(ContextSchemaError {
            location: context.schema.location(),
            message: "[context] must declare schema".to_owned(),
        }));
    };

    let schema_path = resolve_workspace_root_path(&schema_ref.value).ok_or_else(|| {
        Box::new(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: "context schema path must be a relative path inside the workspace".to_owned(),
        })
    })?;
    let _schema_document = ctx.source.document_by_path(&schema_path).ok_or_else(|| {
        Box::new(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema file not found: {schema_path}"),
        })
    })?;
    if !ctx.index.schemas.contains_key(&schema_path) {
        return Err(Box::new(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema path is not a schema document: {schema_path}"),
        }));
    }
    let schema = ctx.index.schemas.get(&schema_path).ok_or_else(|| {
        Box::new(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema file not found: {schema_path}"),
        })
    })?;

    if schema.json.is_none() {
        return Err(Box::new(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema file could not be parsed: {schema_path}"),
        }));
    };
    if schema.validator.is_none() {
        return Err(Box::new(ContextSchemaError {
            location: schema_ref.location.clone(),
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
    if attribute.is_empty() {
        return false;
    }

    let mut current = schema;
    for segment in attribute.split('.') {
        let Some(properties) = current.get("properties").and_then(JsonValue::as_object) else {
            return false;
        };
        let Some(next) = properties.get(segment) else {
            return false;
        };
        current = next;
    }
    true
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
            EntityId::Schema {
                path: schema.path.clone(),
            },
            schema.location.clone(),
            format!("schema is invalid: {message}"),
        );
    }
    ctx.diagnostics.extend(diagnostics);
}
