use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, DocId, EntityId};

use super::super::builtins::declared_workspace_environments;
use super::super::engine::{LintContext, variable_values};
use super::super::index::*;
use super::super::source::{DocumentKind, SourceDocument};
use super::super::syntax::item_location;
use super::marshal::{
    expanded_variable_toml_json, parsed_toml_json, selected_schema_field, selected_value_field,
};
use super::{
    QualifierLintField, RegisteredLintEntity, RegisteredLintField, RegisteredLintSelector,
    VariableLintField, WorkspaceLintField,
};

pub(super) struct RegisteredLintTargetInstance {
    pub(super) entity: EntityId,
    pub(super) location: DiagnosticLocation,
    pub(super) data: JsonValue,
}

pub(super) fn registered_lint_targets(
    ctx: &LintContext,
    selector: &RegisteredLintSelector,
) -> Vec<RegisteredLintTargetInstance> {
    match selector.entity {
        RegisteredLintEntity::Workspace => {
            registered_workspace_targets(ctx, selector.field.as_ref())
        }
        RegisteredLintEntity::Qualifier => {
            registered_qualifier_targets(ctx, selector.field.as_ref())
        }
        RegisteredLintEntity::Variable => registered_variable_targets(ctx, selector.field.as_ref()),
        RegisteredLintEntity::Value => registered_value_targets(ctx, selector.field.as_ref()),
        RegisteredLintEntity::Schema => registered_schema_targets(ctx, selector.field.as_ref()),
    }
}

fn registered_workspace_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    let Some(manifest) = &ctx.index.manifest else {
        return Vec::new();
    };
    let Some(document) = ctx.source.documents.get(&manifest.doc) else {
        return Vec::new();
    };

    let environments = declared_workspace_environments(ctx)
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
    let context_schema =
        manifest
            .context_schema
            .as_ref()
            .and_then(|context| match &context.schema {
                ProjectField::Present(schema) => Some(schema.value.clone()),
                _ => None,
            });

    vec![RegisteredLintTargetInstance {
        entity: EntityId::Workspace,
        location: registered_workspace_location(ctx, manifest, field),
        data: serde_json::json!({
            "kind": "workspace",
            "root": ctx.source.root.display().to_string(),
            "manifest": {
                "uri": document.uri,
                "path": document.path,
                "toml": parsed_toml_json(ctx, manifest.doc),
            },
            "environments": environments,
            "context_schema": context_schema,
        }),
    }]
}

fn registered_qualifier_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    ctx.index
        .qualifiers
        .values()
        .filter_map(|qualifier| {
            let document = ctx.source.documents.get(&qualifier.doc)?;
            Some(RegisteredLintTargetInstance {
                entity: EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                location: registered_qualifier_location(ctx, qualifier, field),
                data: serde_json::json!({
                    "kind": "qualifier",
                    "id": qualifier.id,
                    "uri": document.uri,
                    "path": document.path,
                    "toml": parsed_toml_json(ctx, qualifier.doc),
                }),
            })
        })
        .collect()
}

fn registered_variable_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    ctx.index
        .variables
        .values()
        .filter_map(|variable| {
            let document = ctx.source.documents.get(&variable.doc)?;
            Some(RegisteredLintTargetInstance {
                entity: EntityId::Variable {
                    id: variable.id.clone(),
                },
                location: registered_variable_location(ctx, variable, field),
                data: serde_json::json!({
                    "kind": "variable",
                    "id": variable.id,
                    "uri": document.uri,
                    "path": document.path,
                    "toml": expanded_variable_toml_json(ctx, variable),
                }),
            })
        })
        .collect()
}

fn registered_value_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    let mut targets = Vec::new();
    for variable in ctx.index.variables.values() {
        let Some(variable_document) = ctx.source.documents.get(&variable.doc) else {
            continue;
        };
        for value in variable_values(ctx, variable) {
            targets.push(RegisteredLintTargetInstance {
                entity: EntityId::Value {
                    variable: value.variable_id.clone(),
                    key: value.key.clone(),
                },
                location: registered_value_location(value, field),
                data: serde_json::json!({
                    "kind": "value",
                    "name": value.key,
                    "value": value.value,
                    "origin": value_origin_json(value),
                    "selected": selected_value_field(&value.value, field),
                    "variable": {
                        "id": variable.id,
                        "uri": variable_document.uri,
                        "path": variable_document.path,
                    },
                }),
            });
        }
    }
    targets
}

fn value_origin_json(value: &ValueNode) -> JsonValue {
    match &value.origin {
        ValueOrigin::Inline { variable_doc } => serde_json::json!({
            "kind": "inline",
            "doc": variable_doc,
        }),
        ValueOrigin::External { doc, path } => serde_json::json!({
            "kind": "external",
            "doc": doc,
            "path": path,
        }),
    }
}

fn registered_schema_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    ctx.source
        .documents
        .values()
        .filter(|document| matches!(&document.kind, DocumentKind::Schema))
        .filter_map(|document| {
            let schema = ctx.index.schemas.get(&document.path)?;
            let json = schema.json.as_ref()?;
            Some(RegisteredLintTargetInstance {
                entity: EntityId::Schema {
                    path: document.path.clone(),
                },
                location: registered_schema_location(document, field),
                data: serde_json::json!({
                    "kind": "schema",
                    "uri": document.uri,
                    "path": document.path,
                    "json": json,
                    "selected": selected_schema_field(json, field),
                }),
            })
        })
        .collect()
}

fn registered_workspace_location(
    ctx: &LintContext,
    manifest: &ManifestNode,
    field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    match field {
        Some(RegisteredLintField::Workspace(WorkspaceLintField::Environments)) => {
            toml_root_item_location(ctx, manifest.doc, "environments")
                .unwrap_or_else(|| manifest.location.clone())
        }
        Some(RegisteredLintField::Workspace(WorkspaceLintField::ContextSchema)) => manifest
            .context_schema
            .as_ref()
            .map(|context| context.location.clone())
            .unwrap_or_else(|| manifest.location.clone()),
        _ => manifest.location.clone(),
    }
}

fn registered_qualifier_location(
    ctx: &LintContext,
    qualifier: &QualifierNode,
    field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    match field {
        Some(RegisteredLintField::Qualifier(QualifierLintField::Description)) => {
            toml_root_item_location(ctx, qualifier.doc, "description")
                .unwrap_or_else(|| qualifier.location.clone())
        }
        Some(RegisteredLintField::Qualifier(QualifierLintField::Predicates)) => {
            toml_root_item_location(ctx, qualifier.doc, "predicate")
                .unwrap_or_else(|| qualifier.location.clone())
        }
        _ => qualifier.location.clone(),
    }
}

fn registered_variable_location(
    ctx: &LintContext,
    variable: &VariableNode,
    field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    match field {
        Some(RegisteredLintField::Variable(VariableLintField::Description)) => {
            toml_root_item_location(ctx, variable.doc, "description")
                .unwrap_or_else(|| variable.location.clone())
        }
        Some(RegisteredLintField::Variable(VariableLintField::Type))
            if matches!(&variable.type_source, TypeSourceNode::Primitive(_)) =>
        {
            variable.type_source.location()
        }
        Some(RegisteredLintField::Variable(VariableLintField::Schema))
            if matches!(&variable.type_source, TypeSourceNode::Schema(_)) =>
        {
            variable.type_source.location()
        }
        Some(RegisteredLintField::Variable(VariableLintField::Values)) => {
            variable.values.location.clone()
        }
        Some(RegisteredLintField::Variable(VariableLintField::Environments)) => {
            toml_root_item_location(ctx, variable.doc, "env").unwrap_or_else(|| {
                environment_collection_location(&variable.environments, variable.location.clone())
            })
        }
        _ => variable.location.clone(),
    }
}

fn registered_value_location(
    value: &ValueNode,
    _field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    value.location.clone()
}

fn registered_schema_location(
    document: &SourceDocument,
    _field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    document.document_location()
}

fn toml_root_item_location(ctx: &LintContext, doc: DocId, key: &str) -> Option<DiagnosticLocation> {
    let document = ctx.source.documents.get(&doc)?;
    let parsed = ctx.syntax.toml.get(&doc)?;
    parsed
        .root_table()?
        .get(key)
        .map(|item| item_location(document, item))
}

fn environment_collection_location(
    environments: &EnvironmentCollection,
    fallback: DiagnosticLocation,
) -> DiagnosticLocation {
    match environments {
        EnvironmentCollection::Missing { location }
        | EnvironmentCollection::Invalid { location } => location.clone(),
        EnvironmentCollection::Environments(_) => fallback,
    }
}
