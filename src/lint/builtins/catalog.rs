use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, LintDiagnostic, RototoRuleId, SemanticField};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::source::resolve_workspace_relative_path;
use super::super::stages::{
    push_project_diagnostic, push_reference_diagnostic, push_value_diagnostic,
};
use super::{field_is_integer, field_is_not_present};

pub(super) fn lint_catalog_shapes(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for catalog in ctx.index.catalogs.values() {
        if !field_is_integer(&catalog.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::CatalogSchemaVersion,
                catalog.field_target(SemanticField::SchemaVersion),
                catalog.schema_version.location(),
                "catalog must declare schema_version = 1",
            );
        }

        if field_is_not_present(&catalog.schema) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::CatalogSchemaRef,
                catalog.field_target(SemanticField::CatalogSchema),
                catalog.schema.location(),
                "catalog must declare schema",
            );
        }
    }
}

pub(super) fn lint_catalog_references(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for catalog in ctx.index.catalogs.values() {
        let ProjectField::Present(schema_ref) = &catalog.schema else {
            continue;
        };

        if let Err(err) = resolve_catalog_schema_node(ctx, catalog, schema_ref) {
            push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::CatalogSchemaRef,
                catalog.field_target(SemanticField::CatalogSchema),
                err.location,
                err.message,
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_catalog_entries(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for catalog in ctx.index.catalogs.values() {
        let ProjectField::Present(schema_ref) = &catalog.schema else {
            continue;
        };
        let Ok(schema) = resolve_catalog_schema_node(ctx, catalog, schema_ref) else {
            continue;
        };
        let Some(validator) = &schema.validator else {
            continue;
        };
        let Some(schema_json) = &schema.json else {
            continue;
        };

        for entry in ctx
            .index
            .catalog_entries
            .get(&catalog.id)
            .into_iter()
            .flat_map(|entries| entries.values())
        {
            if let Err(err) = validator.validate(&entry.value) {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::CatalogEntrySchemaMismatch,
                    entry.field_target(SemanticField::CatalogEntry),
                    entry.location.clone(),
                    format!("catalog value {} does not match schema: {err}", entry.key),
                );
            }

            lint_rototo_catalog_references(
                &mut diagnostics,
                ctx,
                entry,
                schema_json,
                &entry.value,
                "$",
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_rototo_catalog_references(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    entry: &CatalogEntryNode,
    schema: &JsonValue,
    value: &JsonValue,
    path: &str,
) {
    if let Some(target_catalog) = schema.get("x-rototo-catalog").and_then(JsonValue::as_str)
        && let Some(target_entry) = value.as_str()
    {
        if !ctx.index.catalogs.contains_key(target_catalog) {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::CatalogEntryUnknownReference,
                entry.field_target(SemanticField::CatalogEntry),
                entry.location.clone(),
                format!("{path} references unknown catalog: {target_catalog}"),
            );
        } else if !ctx
            .index
            .catalog_entries
            .get(target_catalog)
            .is_some_and(|entries| entries.contains_key(target_entry))
        {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::CatalogEntryUnknownReference,
                entry.field_target(SemanticField::CatalogEntry),
                entry.location.clone(),
                format!("{path} references unknown {target_catalog} entry: {target_entry}"),
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
            lint_rototo_catalog_references(diagnostics, ctx, entry, subschema, child, &child_path);
        }
    }

    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, child) in array.iter().enumerate() {
            let child_path = format!("{path}[{index}]");
            lint_rototo_catalog_references(diagnostics, ctx, entry, items, child, &child_path);
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(schemas) = schema.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for subschema in schemas {
            lint_rototo_catalog_references(diagnostics, ctx, entry, subschema, value, path);
        }
    }
}

struct CatalogSchemaReferenceError {
    location: DiagnosticLocation,
    message: String,
}

fn resolve_catalog_schema_node<'a>(
    ctx: &'a LintContext,
    catalog: &CatalogNode,
    schema_ref: &Spanned<String>,
) -> std::result::Result<&'a SchemaNode, Box<CatalogSchemaReferenceError>> {
    let Some(schema_path) =
        resolve_workspace_relative_path(&catalog.location.path, &schema_ref.value)
    else {
        return Err(Box::new(CatalogSchemaReferenceError {
            location: schema_ref.location.clone(),
            message: format!(
                "catalog schema reference is invalid: {} is not a relative path inside the workspace",
                schema_ref.value
            ),
        }));
    };

    let _document = ctx.source.document_by_path(&schema_path).ok_or_else(|| {
        Box::new(CatalogSchemaReferenceError {
            location: schema_ref.location.clone(),
            message: format!(
                "catalog schema reference is invalid: schema file not found: {schema_path}"
            ),
        })
    })?;

    ctx.index.schemas.get(&schema_path).ok_or_else(|| {
        Box::new(CatalogSchemaReferenceError {
            location: schema_ref.location.clone(),
            message: format!(
                "catalog schema reference is invalid: path is not a schema document: {schema_path}"
            ),
        })
    })
}
