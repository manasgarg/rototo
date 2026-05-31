use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, EntityId, RototoRuleId};

use super::super::engine::LintContext;
use super::super::nodes::*;
use super::super::source::{DocumentKind, resolve_workspace_root_path};
use super::super::stages::{push_reference_diagnostic, push_value_diagnostic};
use super::qualifier_reference;

struct ContextSchemaError {
    location: DiagnosticLocation,
    message: String,
}

pub(super) fn lint_context_schema_reference(ctx: &mut LintContext) {
    let Err(err) = valid_context_schema(ctx) else {
        return;
    };

    push_reference_diagnostic(
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
    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if qualifier_reference(&attribute.value).is_some()
                || context_schema_declares_path(schema, &attribute.value)
            {
                continue;
            }

            push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::WorkspaceContextSchemaAttribute,
                EntityId::Predicate {
                    qualifier: qualifier.id.clone(),
                    index: predicate.index,
                },
                attribute.location.clone(),
                format!(
                    "context attribute is not declared by the context schema: {}",
                    attribute.value
                ),
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn valid_context_schema(
    ctx: &LintContext,
) -> std::result::Result<Option<&JsonValue>, ContextSchemaError> {
    let Some(manifest) = &ctx.index.manifest else {
        return Ok(None);
    };
    let Some(context) = &manifest.context_schema else {
        return Ok(None);
    };

    if context.invalid_shape {
        return Err(ContextSchemaError {
            location: context.location.clone(),
            message: "[context] must be a table".to_owned(),
        });
    }

    let ProjectField::Present(schema_ref) = &context.schema else {
        return Err(ContextSchemaError {
            location: context.schema.location(),
            message: "[context] must declare schema".to_owned(),
        });
    };

    let schema_path =
        resolve_workspace_root_path(&schema_ref.value).ok_or_else(|| ContextSchemaError {
            location: schema_ref.location.clone(),
            message: "context schema path must be a relative path inside the workspace".to_owned(),
        })?;
    let schema_document =
        ctx.source
            .document_by_path(&schema_path)
            .ok_or_else(|| ContextSchemaError {
                location: schema_ref.location.clone(),
                message: format!("context schema file not found: {schema_path}"),
            })?;
    if !matches!(&schema_document.kind, DocumentKind::Schema) {
        return Err(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema path is not a schema document: {schema_path}"),
        });
    }

    let schema = ctx
        .syntax
        .json
        .get(&schema_document.id)
        .ok_or_else(|| ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema file could not be parsed: {schema_path}"),
        })?;
    jsonschema::validator_for(schema).map_err(|err| ContextSchemaError {
        location: schema_ref.location.clone(),
        message: format!("context schema is invalid: {err}"),
    })?;

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
    for document in ctx.source.documents.values() {
        if !matches!(&document.kind, DocumentKind::Schema) {
            continue;
        }
        let Some(schema) = ctx.syntax.json.get(&document.id) else {
            continue;
        };

        if let Err(err) = jsonschema::validator_for(schema) {
            push_value_diagnostic(
                &mut diagnostics,
                RototoRuleId::SchemaInvalid,
                EntityId::Schema {
                    path: document.path.clone(),
                },
                document.document_location(),
                format!("schema is invalid: {err}"),
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}
