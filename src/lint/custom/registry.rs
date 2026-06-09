use std::collections::BTreeMap;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticRule, LintStage, RototoRuleId, SemanticEntity,
    Severity,
};
use crate::lua_lint;

use super::super::engine::LintContext;
use super::super::index::{CustomLintRegistration, CustomRuleDefinitionNode, GateEntity};
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
            ctx.index.gates.block(
                GateEntity::CustomLintFile(file.path.clone()),
                LintStage::Register,
                Some(DiagnosticRule::Rototo(RototoRuleId::CustomLintFailed)),
            );
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintFailed,
                SemanticEntity::CustomLint {
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
                ctx.index.gates.block(
                    GateEntity::CustomLintFile(file.path.clone()),
                    LintStage::Register,
                    Some(DiagnosticRule::Rototo(RototoRuleId::CustomLintFailed)),
                );
                push_register_diagnostic(
                    &mut ctx.diagnostics,
                    RototoRuleId::CustomLintFailed,
                    SemanticEntity::CustomLint {
                        path: file.path.clone(),
                    },
                    file.location.clone(),
                    err.to_string(),
                );
                continue;
            }
        };

        if registrations.is_empty() {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintFileUnregistered,
                SemanticEntity::CustomLint {
                    path: file.path.clone(),
                },
                file.location.clone(),
                format!("custom lint file registers no handlers: {}", file.path),
            );
        }

        for registration in registrations {
            match validate_custom_registration(&registration) {
                Ok((stage, selector, definition)) => {
                    let rule = definition.rule.clone();
                    match ctx.index.custom_lints.rules.get(&rule) {
                        Some(existing) if !existing.definition.same_metadata(&definition) => {
                            push_register_diagnostic(
                                &mut ctx.diagnostics,
                                RototoRuleId::CustomLintRuleConflict,
                                SemanticEntity::CustomLint {
                                    path: file.path.clone(),
                                },
                                file.location.clone(),
                                format!("custom lint rule metadata conflicts: {rule}"),
                            );
                            continue;
                        }
                        Some(_) => {}
                        None => {
                            ctx.index.custom_lints.rules.insert(
                                rule.clone(),
                                CustomRuleDefinitionNode {
                                    definition,
                                    location: file.location.clone(),
                                },
                            );
                        }
                    }
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
                    SemanticEntity::CustomLint {
                        path: file.path.clone(),
                    },
                    file.location.clone(),
                    message,
                ),
            }
        }
    }

    lint_duplicate_custom_registrations(ctx);
}

fn lint_duplicate_custom_registrations(ctx: &mut LintContext) {
    let mut seen: BTreeMap<String, &CustomLintRegistration> = BTreeMap::new();

    for registration in &ctx.index.custom_lints.registrations {
        let key = registration_key(registration);
        if let Some(first) = seen.get(&key) {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintRegistrationDuplicate,
                SemanticEntity::CustomLint {
                    path: registration.file_path.clone(),
                },
                registration.location.clone(),
                format!(
                    "custom lint registration duplicates an earlier registration: {}",
                    registration.handler
                ),
            );
            if let Some(diagnostic) = ctx.diagnostics.last_mut() {
                diagnostic
                    .related
                    .push(crate::diagnostics::RelatedLocation {
                        location: first.location.clone(),
                        message: "first matching registration".to_owned(),
                    });
            }
        } else {
            seen.insert(key, registration);
        }
    }
}

fn registration_key(registration: &CustomLintRegistration) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        registration.file_path,
        lint_stage_key(registration.stage),
        selector_key(&registration.selector),
        registration.rule,
        registration.handler
    )
}

fn lint_stage_key(stage: LintStage) -> &'static str {
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

fn selector_key(selector: &RegisteredLintSelector) -> String {
    format!(
        "{}:{}",
        registered_entity_key(selector.entity),
        selector
            .field
            .as_ref()
            .map(registered_field_key)
            .unwrap_or_else(|| "*".to_owned())
    )
}

fn registered_entity_key(entity: RegisteredLintEntity) -> &'static str {
    match entity {
        RegisteredLintEntity::Workspace => "workspace",
        RegisteredLintEntity::Qualifier => "qualifier",
        RegisteredLintEntity::Variable => "variable",
        RegisteredLintEntity::Value => "value",
        RegisteredLintEntity::Schema => "schema",
    }
}

fn registered_field_key(field: &RegisteredLintField) -> String {
    match field {
        RegisteredLintField::Workspace(field) => match field {
            WorkspaceLintField::Extends => "workspace.extends".to_owned(),
        },
        RegisteredLintField::Qualifier(field) => match field {
            QualifierLintField::Id => "qualifier.id".to_owned(),
            QualifierLintField::Description => "qualifier.description".to_owned(),
            QualifierLintField::Predicates => "qualifier.predicates".to_owned(),
        },
        RegisteredLintField::Variable(field) => match field {
            VariableLintField::Id => "variable.id".to_owned(),
            VariableLintField::Description => "variable.description".to_owned(),
            VariableLintField::Type => "variable.type".to_owned(),
            VariableLintField::Schema => "variable.schema".to_owned(),
            VariableLintField::Values => "variable.values".to_owned(),
            VariableLintField::Resolve => "variable.resolve".to_owned(),
        },
        RegisteredLintField::Value(field) => match field {
            ValueLintField::Key => "value.key".to_owned(),
            ValueLintField::Value => "value.value".to_owned(),
            ValueLintField::JsonPath(path) => format!("value.json_path.{}", path.join(".")),
        },
        RegisteredLintField::Schema(field) => match field {
            SchemaLintField::Json => "schema.json".to_owned(),
            SchemaLintField::JsonPath(path) => format!("schema.json_path.{}", path.join(".")),
        },
    }
}

pub(super) fn parse_registered_lint_output_field(
    entity: RegisteredLintEntity,
    field: &str,
) -> Option<RegisteredLintField> {
    match entity {
        RegisteredLintEntity::Workspace => parse_workspace_lint_field(Some(field)),
        RegisteredLintEntity::Qualifier => parse_qualifier_lint_field(Some(field)),
        RegisteredLintEntity::Variable => parse_variable_lint_field(Some(field)),
        RegisteredLintEntity::Value => parse_value_lint_field(Some(field)),
        RegisteredLintEntity::Schema => parse_schema_lint_field(Some(field)),
    }
    .ok()
    .flatten()
}

fn validate_custom_registration(
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

    let rule = CustomRuleId::parse(&registration.rule.id).map_err(|err| {
        (
            RototoRuleId::CustomLintRegistrationInvalid,
            format!(
                "custom lint registration rule id is invalid: {}: {err}",
                registration.rule.id
            ),
        )
    })?;
    let severity = match registration.rule.severity.as_deref() {
        None | Some("error") => Severity::Error,
        Some("warning") => Severity::Warning,
        Some(severity) => {
            return Err((
                RototoRuleId::CustomLintRegistrationInvalid,
                format!("custom lint registration rule severity is unsupported: {severity}"),
            ));
        }
    };
    let definition = CustomRuleDefinition::with_severity(
        rule,
        severity,
        registration.rule.title.clone(),
        registration.rule.help.clone(),
    );

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
        Some("extends") => Ok(Some(RegisteredLintField::Workspace(
            WorkspaceLintField::Extends,
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
        Some("resolve") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Resolve,
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
