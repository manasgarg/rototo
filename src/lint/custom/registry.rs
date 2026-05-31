use std::collections::BTreeMap;

use crate::diagnostics::{CustomRuleId, EntityId, LintStage, RototoRuleId};
use crate::lua_lint;

use super::super::engine::LintContext;
use super::super::index::{CustomLintRegistration, CustomRuleDefinitionNode};
use super::super::stages::push_register_diagnostic;
use super::runner;
use super::{
    QualifierLintField, RegisteredLintEntity, RegisteredLintField, RegisteredLintSelector,
    SchemaLintField, ValueLintField, VariableLintField, WorkspaceLintField,
};

pub(crate) async fn register_custom_lints(ctx: &mut LintContext) {
    let files = ctx
        .index
        .custom_lints
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>();

    for file in files {
        let Some(document) = ctx.source.documents.get(&file.doc).cloned() else {
            continue;
        };
        if let Some(read_error) = &document.read_error {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintFailed,
                EntityId::CustomLint {
                    path: file.path.clone(),
                },
                file.location.clone(),
                format!("failed to read custom lint {}: {read_error}", file.path),
            );
            continue;
        }

        let registrations = match runner::register_pipeline_lint(
            ctx.source.root.join(&file.path),
            document.text.clone(),
        )
        .await
        {
            Ok(registrations) => registrations,
            Err(err) => {
                push_register_diagnostic(
                    &mut ctx.diagnostics,
                    RototoRuleId::CustomLintFailed,
                    EntityId::CustomLint {
                        path: file.path.clone(),
                    },
                    file.location.clone(),
                    err.to_string(),
                );
                continue;
            }
        };

        for registration in registrations {
            match validate_custom_registration(&ctx.index.custom_lints.rules, &registration) {
                Ok((stage, selector, rule)) => {
                    ctx.index
                        .custom_lints
                        .registrations
                        .push(CustomLintRegistration {
                            file_path: file.path.clone(),
                            rule,
                            stage,
                            selector,
                            handler: registration.handler,
                            location: file.location.clone(),
                        });
                }
                Err((rule, message)) => push_register_diagnostic(
                    &mut ctx.diagnostics,
                    rule,
                    EntityId::CustomLint {
                        path: file.path.clone(),
                    },
                    file.location.clone(),
                    message,
                ),
            }
        }
    }
}

fn validate_custom_registration(
    workspace_rules: &BTreeMap<CustomRuleId, CustomRuleDefinitionNode>,
    registration: &lua_lint::RawCustomLintRegistration,
) -> std::result::Result<(LintStage, RegisteredLintSelector, CustomRuleId), (RototoRuleId, String)>
{
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

    Ok((stage, selector, definition.definition.rule))
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
