use std::collections::BTreeMap;

use serde_json::Value as JsonValue;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, DocId, EntityId, LintDiagnostic,
    LintStage, RototoRuleId,
};
use crate::lua_lint;

use super::builtins::{declared_workspace_environments, workspace_custom_rule_definitions};
use super::engine::{
    LintContext, push_register_diagnostic, push_stage_diagnostic, variable_values,
};
use super::nodes::*;
use super::project::json_from_toml_value;
use super::source::{DocumentKind, SourceDocument};
use super::syntax::item_location;

#[derive(Clone)]
pub(super) struct RegisteredCustomLint {
    file_path: String,
    script: String,
    stage: LintStage,
    selector: RegisteredLintSelector,
    definition: CustomRuleDefinition,
    handler: String,
}

#[derive(Clone)]
struct RegisteredLintSelector {
    entity: RegisteredLintEntity,
    field: Option<RegisteredLintField>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegisteredLintEntity {
    Workspace,
    Qualifier,
    Variable,
    Value,
    Schema,
}

#[derive(Clone)]
enum RegisteredLintField {
    Workspace(WorkspaceLintField),
    Qualifier(QualifierLintField),
    Variable(VariableLintField),
    Value(ValueLintField),
    Schema(SchemaLintField),
}

#[derive(Clone)]
enum WorkspaceLintField {
    Environments,
    ContextSchema,
}

#[derive(Clone)]
enum QualifierLintField {
    Id,
    Description,
    Predicates,
}

#[derive(Clone)]
enum VariableLintField {
    Id,
    Description,
    Type,
    Schema,
    Values,
    Environments,
}

#[derive(Clone)]
enum ValueLintField {
    Key,
    Value,
    JsonPath(Vec<String>),
}

#[derive(Clone)]
enum SchemaLintField {
    Json,
    JsonPath(Vec<String>),
}

pub(super) async fn register_custom_lints(ctx: &mut LintContext) {
    let workspace_rules = workspace_custom_rule_definitions(ctx);
    let documents = ctx
        .source
        .documents
        .values()
        .filter(|document| matches!(&document.kind, DocumentKind::CustomLint))
        .cloned()
        .collect::<Vec<_>>();

    for document in documents {
        if let Some(read_error) = &document.read_error {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintFailed,
                EntityId::CustomLint {
                    path: document.path.clone(),
                },
                document.document_location(),
                format!("failed to read custom lint {}: {read_error}", document.path),
            );
            continue;
        }

        let input = lua_lint::RegisterLintInput {
            lint_path: ctx.source.root.join(&document.path),
            script: document.text.clone(),
        };
        let registrations = match lua_lint::register_pipeline_lint(input).await {
            Ok(registrations) => registrations,
            Err(err) => {
                push_register_diagnostic(
                    &mut ctx.diagnostics,
                    RototoRuleId::CustomLintFailed,
                    EntityId::CustomLint {
                        path: document.path.clone(),
                    },
                    document.document_location(),
                    err.to_string(),
                );
                continue;
            }
        };

        for registration in registrations {
            match validate_custom_registration(&workspace_rules, &registration) {
                Ok((stage, selector, definition)) => {
                    ctx.registered_custom_lints.push(RegisteredCustomLint {
                        file_path: document.path.clone(),
                        script: document.text.clone(),
                        stage,
                        selector,
                        definition,
                        handler: registration.handler,
                    });
                }
                Err((rule, message)) => push_register_diagnostic(
                    &mut ctx.diagnostics,
                    rule,
                    EntityId::CustomLint {
                        path: document.path.clone(),
                    },
                    document.document_location(),
                    message,
                ),
            }
        }
    }
}

fn validate_custom_registration(
    workspace_rules: &BTreeMap<CustomRuleId, CustomRuleDefinition>,
    registration: &lua_lint::RawCustomLintRegistration,
) -> std::result::Result<
    (LintStage, RegisteredLintSelector, CustomRuleDefinition),
    (RototoRuleId, String),
> {
    let stage = parse_registered_lint_stage(&registration.stage)?;
    let selector =
        parse_registered_lint_selector(&registration.entity, registration.field.as_deref())?;
    if !registration.handler_exists {
        return Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!(
                "custom lint registration handler is not callable: {}",
                registration.handler
            ),
        ));
    }

    let rule = CustomRuleId::parse(&registration.rule).map_err(|err| {
        (
            RototoRuleId::CustomLintRegistrationInvalid,
            format!(
                "custom lint registration rule id is invalid: {}: {err}",
                registration.rule
            ),
        )
    })?;
    let definition = workspace_rules.get(&rule).cloned().ok_or_else(|| {
        (
            RototoRuleId::CustomLintUnknownRule,
            format!("custom lint registration references undeclared rule: {rule}"),
        )
    })?;

    Ok((stage, selector, definition))
}

fn parse_registered_lint_stage(
    stage: &str,
) -> std::result::Result<LintStage, (RototoRuleId, String)> {
    match stage {
        "project" => Ok(LintStage::Project),
        "reference" => Ok(LintStage::Reference),
        "value" => Ok(LintStage::Value),
        "graph" => Ok(LintStage::Graph),
        "policy" => Ok(LintStage::Policy),
        _ => Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!("custom lint registration has unsupported stage: {stage}"),
        )),
    }
}

fn parse_registered_lint_selector(
    entity: &str,
    field: Option<&str>,
) -> std::result::Result<RegisteredLintSelector, (RototoRuleId, String)> {
    match entity {
        "workspace" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Workspace,
            field: parse_workspace_lint_field(field)?,
        }),
        "qualifier" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Qualifier,
            field: parse_qualifier_lint_field(field)?,
        }),
        "variable" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Variable,
            field: parse_variable_lint_field(field)?,
        }),
        "value" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Value,
            field: parse_value_lint_field(field)?,
        }),
        "schema" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Schema,
            field: parse_schema_lint_field(field)?,
        }),
        _ => Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!("custom lint registration has unsupported entity: {entity}"),
        )),
    }
}

fn parse_workspace_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("environments") => Ok(Some(RegisteredLintField::Workspace(
            WorkspaceLintField::Environments,
        ))),
        Some("context_schema") => Ok(Some(RegisteredLintField::Workspace(
            WorkspaceLintField::ContextSchema,
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_qualifier_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("id") => Ok(Some(RegisteredLintField::Qualifier(QualifierLintField::Id))),
        Some("description") => Ok(Some(RegisteredLintField::Qualifier(
            QualifierLintField::Description,
        ))),
        Some("predicates") => Ok(Some(RegisteredLintField::Qualifier(
            QualifierLintField::Predicates,
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_variable_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("id") => Ok(Some(RegisteredLintField::Variable(VariableLintField::Id))),
        Some("description") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Description,
        ))),
        Some("type") => Ok(Some(RegisteredLintField::Variable(VariableLintField::Type))),
        Some("schema") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Schema,
        ))),
        Some("values") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Values,
        ))),
        Some("environments") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Environments,
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_value_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("key") => Ok(Some(RegisteredLintField::Value(ValueLintField::Key))),
        Some("value") => Ok(Some(RegisteredLintField::Value(ValueLintField::Value))),
        Some(field) if field.starts_with("value.") => Ok(Some(RegisteredLintField::Value(
            ValueLintField::JsonPath(parse_json_path_selector(field, "value.")?),
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_schema_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("json") => Ok(Some(RegisteredLintField::Schema(SchemaLintField::Json))),
        Some(field) if field.starts_with("json.") => Ok(Some(RegisteredLintField::Schema(
            SchemaLintField::JsonPath(parse_json_path_selector(field, "json.")?),
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_json_path_selector(
    field: &str,
    prefix: &str,
) -> std::result::Result<Vec<String>, (RototoRuleId, String)> {
    let path = field.strip_prefix(prefix).unwrap_or_default();
    let segments = path.split('.').map(str::to_owned).collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| !valid_json_path_segment(segment))
    {
        return Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!("custom lint registration has unsupported field: {field}"),
        ));
    }
    Ok(segments)
}

fn valid_json_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn unsupported_registration_field<T>(
    field: &str,
) -> std::result::Result<Option<T>, (RototoRuleId, String)> {
    Err((
        RototoRuleId::CustomLintRegistrationInvalid,
        format!("custom lint registration has unsupported field: {field}"),
    ))
}

fn expanded_variable_toml_json(ctx: &LintContext, variable: &VariableNode) -> JsonValue {
    let mut toml = ctx
        .syntax
        .toml
        .get(&variable.doc)
        .map(|parsed| json_from_toml_value(&parsed.plain))
        .unwrap_or_else(|| JsonValue::Object(serde_json::Map::new()));
    let mut values = serde_json::Map::new();
    for value in variable_values(ctx, variable) {
        values.insert(value.key.clone(), value.value.clone());
    }

    if let JsonValue::Object(object) = &mut toml {
        object.insert("values".to_owned(), JsonValue::Object(values));
    }
    toml
}

struct RegisteredLintTargetInstance {
    entity: EntityId,
    location: DiagnosticLocation,
    data: JsonValue,
}

pub(super) async fn run_registered_custom_lints(ctx: &mut LintContext, stage: LintStage) {
    let registrations = ctx
        .registered_custom_lints
        .iter()
        .filter(|registration| registration.stage == stage)
        .cloned()
        .collect::<Vec<_>>();

    for registration in registrations {
        let targets = registered_lint_targets(ctx, &registration.selector);
        for target in targets {
            let input = lua_lint::RegisteredLintInput {
                stage: lint_stage_label(stage).to_owned(),
                target: lua_lint::RegisteredLintTarget {
                    entity: registered_lint_entity_label(registration.selector.entity).to_owned(),
                    data: target.data,
                },
                lint_path: ctx.source.root.join(&registration.file_path),
                script: registration.script.clone(),
                handler: registration.handler.clone(),
            };

            match lua_lint::lint_registered_target(input).await {
                Ok(outputs) => {
                    for output in outputs {
                        ctx.diagnostics.push(LintDiagnostic::custom(
                            &registration.definition,
                            stage,
                            target.entity.clone(),
                            target.location.clone(),
                            output.message,
                        ));
                    }
                }
                Err(err) => push_stage_diagnostic(
                    &mut ctx.diagnostics,
                    stage,
                    RototoRuleId::CustomLintFailed,
                    target.entity.clone(),
                    target.location.clone(),
                    format!(
                        "custom lint handler failed in {}: {err}",
                        registration.file_path
                    ),
                ),
            }
        }
    }
}

fn registered_lint_targets(
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
                    variable: variable.id.clone(),
                    key: value.key.clone(),
                },
                location: registered_value_location(value, field),
                data: serde_json::json!({
                    "kind": "value",
                    "name": value.key,
                    "value": value.value,
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

fn registered_schema_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    ctx.source
        .documents
        .values()
        .filter(|document| matches!(&document.kind, DocumentKind::Schema))
        .filter_map(|document| {
            let schema = ctx.syntax.json.get(&document.id)?;
            Some(RegisteredLintTargetInstance {
                entity: EntityId::Schema {
                    path: document.path.clone(),
                },
                location: registered_schema_location(document, field),
                data: serde_json::json!({
                    "kind": "schema",
                    "uri": document.uri,
                    "path": document.path,
                    "json": schema,
                    "selected": selected_schema_field(schema, field),
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
        .edit
        .as_table()
        .get(key)
        .map(|item| item_location(document, item))
}

fn parsed_toml_json(ctx: &LintContext, doc: DocId) -> JsonValue {
    ctx.syntax
        .toml
        .get(&doc)
        .map(|parsed| json_from_toml_value(&parsed.plain))
        .unwrap_or(JsonValue::Null)
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

fn selected_value_field(value: &JsonValue, field: Option<&RegisteredLintField>) -> JsonValue {
    match field {
        Some(RegisteredLintField::Value(ValueLintField::JsonPath(path))) => {
            json_value_at_path(value, path)
                .cloned()
                .unwrap_or(JsonValue::Null)
        }
        _ => value.clone(),
    }
}

fn selected_schema_field(schema: &JsonValue, field: Option<&RegisteredLintField>) -> JsonValue {
    match field {
        Some(RegisteredLintField::Schema(SchemaLintField::JsonPath(path))) => {
            json_value_at_path(schema, path)
                .cloned()
                .unwrap_or(JsonValue::Null)
        }
        _ => schema.clone(),
    }
}

fn json_value_at_path<'a>(value: &'a JsonValue, path: &[String]) -> Option<&'a JsonValue> {
    let mut current = value;
    for segment in path {
        current = current.get(segment)?;
    }
    Some(current)
}

fn lint_stage_label(stage: LintStage) -> &'static str {
    match stage {
        LintStage::Discover => "discover",
        LintStage::Parse => "parse",
        LintStage::Project => "project",
        LintStage::Register => "register",
        LintStage::Reference => "reference",
        LintStage::Value => "value",
        LintStage::Graph => "graph",
        LintStage::Policy => "policy",
    }
}

fn registered_lint_entity_label(entity: RegisteredLintEntity) -> &'static str {
    match entity {
        RegisteredLintEntity::Workspace => "workspace",
        RegisteredLintEntity::Qualifier => "qualifier",
        RegisteredLintEntity::Variable => "variable",
        RegisteredLintEntity::Value => "value",
        RegisteredLintEntity::Schema => "schema",
    }
}
