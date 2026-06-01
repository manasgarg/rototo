use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, RototoRuleId};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::source::resolve_workspace_relative_path;
use super::super::stages::{
    push_project_diagnostic, push_reference_diagnostic, push_value_diagnostic,
};
use super::{field_is_integer, field_is_not_present};

pub(super) fn lint_resource_shapes(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for resource in ctx.index.resources.values() {
        if !field_is_integer(&resource.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::ResourceSchemaVersion,
                EntityId::Resource {
                    id: resource.id.clone(),
                },
                resource.schema_version.location(),
                "resource must declare schema_version = 1",
            );
        }

        if field_is_not_present(&resource.schema) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::ResourceSchemaRef,
                EntityId::Resource {
                    id: resource.id.clone(),
                },
                resource.schema.location(),
                "resource must declare schema",
            );
        }
    }
}

pub(super) fn lint_resource_references(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for resource in ctx.index.resources.values() {
        let ProjectField::Present(schema_ref) = &resource.schema else {
            continue;
        };

        if let Err(err) = resolve_resource_schema_node(ctx, resource, schema_ref) {
            push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::ResourceSchemaRef,
                EntityId::Resource {
                    id: resource.id.clone(),
                },
                err.location,
                err.message,
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_resource_objects(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for resource in ctx.index.resources.values() {
        let ProjectField::Present(schema_ref) = &resource.schema else {
            continue;
        };
        let Ok(schema) = resolve_resource_schema_node(ctx, resource, schema_ref) else {
            continue;
        };
        let Some(validator) = &schema.validator else {
            continue;
        };
        let Some(schema_json) = &schema.json else {
            continue;
        };

        for object in ctx
            .index
            .resource_objects
            .get(&resource.id)
            .into_iter()
            .flat_map(|objects| objects.values())
        {
            if let Err(err) = validator.validate(&object.value) {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::ResourceObjectSchemaMismatch,
                    EntityId::ResourceObject {
                        resource: resource.id.clone(),
                        key: object.key.clone(),
                    },
                    object.location.clone(),
                    format!(
                        "resource object {} does not match schema: {err}",
                        object.key
                    ),
                );
            }

            lint_rototo_resource_references(
                &mut diagnostics,
                ctx,
                resource,
                object,
                schema_json,
                &object.value,
                "$",
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_rototo_resource_references(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    resource: &ResourceNode,
    object: &ResourceObjectNode,
    schema: &JsonValue,
    value: &JsonValue,
    path: &str,
) {
    if let Some(target_resource) = schema.get("x-rototo-resource").and_then(JsonValue::as_str)
        && let Some(target_object) = value.as_str()
    {
        if !ctx.index.resources.contains_key(target_resource) {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::ResourceObjectUnknownReference,
                EntityId::ResourceObject {
                    resource: resource.id.clone(),
                    key: object.key.clone(),
                },
                object.location.clone(),
                format!("{path} references unknown resource: {target_resource}"),
            );
        } else if !ctx
            .index
            .resource_objects
            .get(target_resource)
            .is_some_and(|objects| objects.contains_key(target_object))
        {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::ResourceObjectUnknownReference,
                EntityId::ResourceObject {
                    resource: resource.id.clone(),
                    key: object.key.clone(),
                },
                object.location.clone(),
                format!("{path} references unknown {target_resource} object: {target_object}"),
            );
        }
    }

    if let (Some(properties), Some(object_value)) = (
        schema.get("properties").and_then(JsonValue::as_object),
        value.as_object(),
    ) {
        for (key, subschema) in properties {
            let Some(child) = object_value.get(key) else {
                continue;
            };
            let child_path = format!("{path}.{key}");
            lint_rototo_resource_references(
                diagnostics,
                ctx,
                resource,
                object,
                subschema,
                child,
                &child_path,
            );
        }
    }

    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, child) in array.iter().enumerate() {
            let child_path = format!("{path}[{index}]");
            lint_rototo_resource_references(
                diagnostics,
                ctx,
                resource,
                object,
                items,
                child,
                &child_path,
            );
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(schemas) = schema.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for subschema in schemas {
            lint_rototo_resource_references(
                diagnostics,
                ctx,
                resource,
                object,
                subschema,
                value,
                path,
            );
        }
    }
}

struct ResourceSchemaReferenceError {
    location: DiagnosticLocation,
    message: String,
}

fn resolve_resource_schema_node<'a>(
    ctx: &'a LintContext,
    resource: &ResourceNode,
    schema_ref: &Spanned<String>,
) -> std::result::Result<&'a SchemaNode, Box<ResourceSchemaReferenceError>> {
    let Some(schema_path) =
        resolve_workspace_relative_path(&resource.location.path, &schema_ref.value)
    else {
        return Err(Box::new(ResourceSchemaReferenceError {
            location: schema_ref.location.clone(),
            message: format!(
                "resource schema reference is invalid: {} is not a relative path inside the workspace",
                schema_ref.value
            ),
        }));
    };

    let _document = ctx.source.document_by_path(&schema_path).ok_or_else(|| {
        Box::new(ResourceSchemaReferenceError {
            location: schema_ref.location.clone(),
            message: format!(
                "resource schema reference is invalid: schema file not found: {schema_path}"
            ),
        })
    })?;

    ctx.index.schemas.get(&schema_path).ok_or_else(|| {
        Box::new(ResourceSchemaReferenceError {
            location: schema_ref.location.clone(),
            message: format!(
                "resource schema reference is invalid: path is not a schema document: {schema_path}"
            ),
        })
    })
}
