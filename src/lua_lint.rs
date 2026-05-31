use std::path::{Path, PathBuf};

use mlua::{Lua, LuaSerdeExt, Table, Value as LuaValue};
use serde_json::Value as JsonValue;
use toml::Value;

use crate::diagnostics::{CustomRuleDefinition, CustomRuleId, Diagnostic, RototoRuleId};
use crate::error::{Result, RototoError};
use crate::model::VariableInspection;

#[derive(Clone)]
pub struct RegisterLintInput {
    pub lint_path: PathBuf,
    pub script: String,
}

#[derive(Clone)]
pub struct RawCustomLintRegistration {
    pub stage: String,
    pub entity: String,
    pub field: Option<String>,
    pub rule: String,
    pub handler: String,
    pub handler_exists: bool,
}

#[derive(Clone)]
pub struct RegisteredLintInput {
    pub stage: String,
    pub target: RegisteredLintTarget,
    pub lint_path: PathBuf,
    pub script: String,
    pub handler: String,
}

#[derive(Clone)]
pub struct RegisteredLintTarget {
    pub entity: String,
    pub data: JsonValue,
}

pub struct RegisteredCustomLintOutput {
    pub message: String,
    pub field: Option<String>,
}

pub async fn register_pipeline_lint(
    input: RegisterLintInput,
) -> Result<Vec<RawCustomLintRegistration>> {
    tokio::task::spawn_blocking(move || register_pipeline_lint_script(input))
        .await
        .map_err(|err| RototoError::new(format!("custom lint registration task failed: {err}")))?
}

fn register_pipeline_lint_script(
    input: RegisterLintInput,
) -> Result<Vec<RawCustomLintRegistration>> {
    let lua = Lua::new();
    lua.load(&input.script)
        .set_name(input.lint_path.display().to_string())
        .exec()
        .map_err(|err| RototoError::new(format!("failed to execute custom lint: {err}")))?;

    let globals = lua.globals();
    let register = globals
        .get::<Option<mlua::Function>>("register")
        .map_err(|err| {
            RototoError::new(format!("custom lint has invalid register function: {err}"))
        })?;
    let Some(register) = register else {
        return Ok(Vec::new());
    };

    let registrations = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let on_registrations = registrations.clone();
    let on = lua
        .create_function_mut(move |_, args: mlua::Variadic<LuaValue>| {
            let table = registration_arg_table(args)?;
            on_registrations
                .borrow_mut()
                .push(registration_from_lua_table(table)?);
            Ok(())
        })
        .map_err(|err| RototoError::new(format!("failed to prepare custom lint API: {err}")))?;
    let lint_api = lua
        .create_table()
        .map_err(|err| RototoError::new(format!("failed to prepare custom lint API: {err}")))?;
    lint_api
        .set("on", on)
        .map_err(|err| RototoError::new(format!("failed to prepare custom lint API: {err}")))?;

    register
        .call::<()>(lint_api)
        .map_err(|err| RototoError::new(format!("custom lint registration failed: {err}")))?;
    let mut registrations = registrations.borrow().clone();
    for registration in &mut registrations {
        registration.handler_exists = globals
            .get::<Option<mlua::Function>>(registration.handler.as_str())
            .is_ok_and(|handler| handler.is_some());
    }
    Ok(registrations)
}

fn registration_arg_table(args: mlua::Variadic<LuaValue>) -> mlua::Result<Table> {
    let value = match args.len() {
        1 => args.into_iter().next(),
        2 => args.into_iter().nth(1),
        _ => None,
    };
    match value {
        Some(LuaValue::Table(table)) => Ok(table),
        _ => Err(mlua::Error::external(
            "lint:on expects a single registration table",
        )),
    }
}

fn registration_from_lua_table(table: Table) -> mlua::Result<RawCustomLintRegistration> {
    Ok(RawCustomLintRegistration {
        stage: required_registration_string(&table, "stage")?,
        entity: required_registration_string(&table, "entity")?,
        field: table.get::<Option<String>>("field")?,
        rule: required_registration_string(&table, "rule")?,
        handler: required_registration_string(&table, "handler")?,
        handler_exists: false,
    })
}

fn required_registration_string(table: &Table, key: &str) -> mlua::Result<String> {
    table
        .get::<Option<String>>(key)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| mlua::Error::external(format!("registration must contain {key}")))
}

pub async fn lint_registered_target(
    input: RegisteredLintInput,
) -> Result<Vec<RegisteredCustomLintOutput>> {
    tokio::task::spawn_blocking(move || lint_registered_target_script(input))
        .await
        .map_err(|err| RototoError::new(format!("custom lint task failed: {err}")))?
}

fn lint_registered_target_script(
    input: RegisteredLintInput,
) -> Result<Vec<RegisteredCustomLintOutput>> {
    let lua = Lua::new();
    lua.load(&input.script)
        .set_name(input.lint_path.display().to_string())
        .exec()
        .map_err(|err| RototoError::new(format!("failed to execute custom lint: {err}")))?;

    let globals = lua.globals();
    let handler = globals
        .get::<mlua::Function>(input.handler.as_str())
        .map_err(|err| RototoError::new(format!("custom lint handler is invalid: {err}")))?;
    let ctx = lua
        .to_value(&serde_json::json!({
            "stage": input.stage,
            "entity": input.target.entity,
            "target": input.target.data,
        }))
        .map_err(|err| RototoError::new(format!("failed to prepare Lua context: {err}")))?;
    let returned: LuaValue = handler
        .call(ctx)
        .map_err(|err| RototoError::new(format!("custom lint failed: {err}")))?;
    registered_outputs_from_lua(returned)
}

fn registered_outputs_from_lua(returned: LuaValue) -> Result<Vec<RegisteredCustomLintOutput>> {
    match returned {
        LuaValue::Nil => Ok(Vec::new()),
        LuaValue::Table(table) => {
            let mut diagnostics = Vec::new();
            for entry in table.sequence_values::<Table>() {
                let entry = entry.map_err(|err| {
                    RototoError::new(format!("custom lint returned an invalid diagnostic: {err}"))
                })?;
                let message = entry
                    .get::<Option<String>>("message")
                    .map_err(|err| {
                        RototoError::new(format!("custom lint message is invalid: {err}"))
                    })?
                    .ok_or_else(|| {
                        RototoError::new("custom lint diagnostic must contain message")
                    })?;
                let field = entry.get::<Option<String>>("field").map_err(|err| {
                    RototoError::new(format!("custom lint field is invalid: {err}"))
                })?;
                diagnostics.push(RegisteredCustomLintOutput { message, field });
            }
            Ok(diagnostics)
        }
        other => Err(RototoError::new(format!(
            "custom lint must return a list of diagnostics, got {}",
            lua_type_name(&other)
        ))),
    }
}

#[derive(Clone)]
pub struct PipelineLintInput {
    pub workspace_root: PathBuf,
    pub environments: Vec<String>,
    pub variable: PipelineVariable,
    pub lint_path: PathBuf,
    pub script: String,
    pub custom_rules: Vec<CustomRuleDefinition>,
}

#[derive(Clone)]
pub struct PipelineVariable {
    pub id: String,
    pub uri: String,
    pub path: String,
    pub toml: JsonValue,
    pub values: Vec<PipelineValue>,
}

#[derive(Clone)]
pub struct PipelineValue {
    pub name: String,
    pub value: JsonValue,
}

#[derive(Clone)]
pub enum CustomLintTarget {
    Variable,
    Value { name: String },
}

pub enum CustomLintOutput {
    Diagnostic {
        target: CustomLintTarget,
        rule: CustomRuleId,
        message: String,
    },
    InvalidRule {
        target: CustomLintTarget,
        message: String,
    },
    UnknownRule {
        target: CustomLintTarget,
        rule: String,
        message: String,
    },
}

pub async fn lint_pipeline_variable(input: PipelineLintInput) -> Result<Vec<CustomLintOutput>> {
    tokio::task::spawn_blocking(move || lint_pipeline_variable_script(input))
        .await
        .map_err(|err| RototoError::new(format!("custom lint task failed: {err}")))?
}

fn lint_pipeline_variable_script(input: PipelineLintInput) -> Result<Vec<CustomLintOutput>> {
    let lua = Lua::new();
    let globals = lua.globals();
    let variable_id = input.variable.id.clone();
    let variable_uri = input.variable.uri.clone();
    let variable_path = input.variable.path.clone();
    let variable_context = lua
        .to_value(&serde_json::json!({
            "id": variable_id,
            "uri": variable_uri,
            "path": variable_path,
            "toml": input.variable.toml.clone(),
            "workspace": {
                "root": input.workspace_root,
                "environments": input.environments,
            },
        }))
        .map_err(|err| RototoError::new(format!("failed to prepare Lua context: {err}")))?;
    globals
        .set("variable", variable_context.clone())
        .map_err(|err| RototoError::new(format!("failed to set Lua context: {err}")))?;

    lua.load(&input.script)
        .set_name(input.lint_path.display().to_string())
        .exec()
        .map_err(|err| RototoError::new(format!("failed to execute custom lint: {err}")))?;

    let lint = globals
        .get::<Option<mlua::Function>>("lint")
        .map_err(|err| RototoError::new(format!("custom lint has invalid lint function: {err}")))?;
    let lint_value = globals
        .get::<Option<mlua::Function>>("lint_value")
        .map_err(|err| {
            RototoError::new(format!(
                "custom lint has invalid lint_value function: {err}"
            ))
        })?;
    if lint.is_none() && lint_value.is_none() {
        return Err(RototoError::new(
            "custom lint must define lint(variable) or lint_value(value)",
        ));
    }

    let mut diagnostics = Vec::new();
    if let Some(lint) = lint {
        let returned: LuaValue = lint
            .call(variable_context)
            .map_err(|err| RototoError::new(format!("custom lint failed: {err}")))?;
        diagnostics.extend(pipeline_diagnostics_from_lua(
            CustomLintTarget::Variable,
            returned,
            &input.custom_rules,
        )?);
    }

    if let Some(lint_value) = lint_value {
        for value in input.variable.values {
            let value_context = lua
                .to_value(&serde_json::json!({
                    "name": value.name.clone(),
                    "value": value.value,
                    "variable": {
                        "id": input.variable.id.clone(),
                        "uri": input.variable.uri.clone(),
                        "path": input.variable.path.clone(),
                    },
                }))
                .map_err(|err| {
                    RototoError::new(format!("failed to prepare Lua value context: {err}"))
                })?;
            let returned: LuaValue = lint_value
                .call(value_context)
                .map_err(|err| RototoError::new(format!("custom value lint failed: {err}")))?;
            diagnostics.extend(pipeline_diagnostics_from_lua(
                CustomLintTarget::Value { name: value.name },
                returned,
                &input.custom_rules,
            )?);
        }
    }

    Ok(diagnostics)
}

fn pipeline_diagnostics_from_lua(
    target: CustomLintTarget,
    returned: LuaValue,
    custom_rules: &[CustomRuleDefinition],
) -> Result<Vec<CustomLintOutput>> {
    match returned {
        LuaValue::Nil => Ok(Vec::new()),
        LuaValue::Table(table) => pipeline_diagnostics_from_table(target, table, custom_rules),
        other => Err(RototoError::new(format!(
            "custom lint must return a list of diagnostics, got {}",
            lua_type_name(&other)
        ))),
    }
}

fn pipeline_diagnostics_from_table(
    target: CustomLintTarget,
    table: Table,
    custom_rules: &[CustomRuleDefinition],
) -> Result<Vec<CustomLintOutput>> {
    let mut diagnostics = Vec::new();
    for entry in table.sequence_values::<Table>() {
        let entry = entry.map_err(|err| {
            RototoError::new(format!("custom lint returned an invalid diagnostic: {err}"))
        })?;
        let rule = entry
            .get::<Option<String>>("rule")
            .map_err(|err| RototoError::new(format!("custom lint rule is invalid: {err}")))?;
        let message = entry
            .get::<Option<String>>("message")
            .map_err(|err| RototoError::new(format!("custom lint message is invalid: {err}")))?
            .ok_or_else(|| RototoError::new("custom lint diagnostic must contain message"))?;

        let Some(rule) = rule else {
            diagnostics.push(CustomLintOutput::InvalidRule {
                target: target.clone(),
                message: "custom lint diagnostic must contain rule".to_owned(),
            });
            continue;
        };
        let parsed_rule = match CustomRuleId::parse(&rule) {
            Ok(rule) => rule,
            Err(err) => {
                diagnostics.push(CustomLintOutput::InvalidRule {
                    target: target.clone(),
                    message: format!("custom lint emitted invalid rule {rule}: {err}"),
                });
                continue;
            }
        };
        if !custom_rules
            .iter()
            .any(|definition| definition.rule == parsed_rule)
        {
            diagnostics.push(CustomLintOutput::UnknownRule {
                target: target.clone(),
                rule,
                message: "custom lint emitted undeclared rule".to_owned(),
            });
            continue;
        }

        diagnostics.push(CustomLintOutput::Diagnostic {
            target: target.clone(),
            rule: parsed_rule,
            message,
        });
    }
    Ok(diagnostics)
}

pub async fn lint_variable(
    workspace_root: &Path,
    variable: &VariableInspection,
    variable_toml: &Value,
    environments: &[String],
    custom_rules: &[CustomRuleDefinition],
) -> Result<Vec<Diagnostic>> {
    let Some(path) = lint_path(variable_toml) else {
        return Ok(Vec::new());
    };

    let lint_path = workspace_root
        .join(&variable.path)
        .parent()
        .unwrap_or(workspace_root)
        .join(path);
    let script = tokio::fs::read_to_string(&lint_path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to read custom lint {}: {err}",
            lint_path.display()
        ))
    })?;

    let workspace_root = workspace_root.to_path_buf();
    let variable = variable.clone();
    let variable_toml = variable_toml.clone();
    let environments = environments.to_vec();
    let custom_rules = custom_rules.to_vec();
    tokio::task::spawn_blocking(move || {
        lint_variable_script(
            workspace_root,
            variable,
            variable_toml,
            environments,
            custom_rules,
            lint_path,
            script,
        )
    })
    .await
    .map_err(|err| RototoError::new(format!("custom lint task failed: {err}")))?
}

fn lint_variable_script(
    workspace_root: PathBuf,
    variable: VariableInspection,
    variable_toml: Value,
    environments: Vec<String>,
    custom_rules: Vec<CustomRuleDefinition>,
    lint_path: PathBuf,
    script: String,
) -> Result<Vec<Diagnostic>> {
    let lua = Lua::new();
    let globals = lua.globals();
    globals
        .set(
            "variable",
            lua.to_value(&serde_json::json!({
                "id": variable.id,
                "uri": variable.uri,
                "path": variable.path,
                "toml": serde_json::to_value(&variable_toml)
                    .map_err(|err| RototoError::new(err.to_string()))?,
                "workspace": {
                    "root": workspace_root,
                    "environments": environments,
                },
            }))
            .map_err(|err| RototoError::new(format!("failed to prepare Lua context: {err}")))?,
        )
        .map_err(|err| RototoError::new(format!("failed to set Lua context: {err}")))?;

    lua.load(&script)
        .set_name(lint_path.display().to_string())
        .exec()
        .map_err(|err| RototoError::new(format!("failed to execute custom lint: {err}")))?;

    let lint = globals
        .get::<Option<mlua::Function>>("lint")
        .map_err(|err| RototoError::new(format!("custom lint has invalid lint function: {err}")))?;
    let lint_value = globals
        .get::<Option<mlua::Function>>("lint_value")
        .map_err(|err| {
            RototoError::new(format!(
                "custom lint has invalid lint_value function: {err}"
            ))
        })?;
    if lint.is_none() && lint_value.is_none() {
        return Err(RototoError::new(
            "custom lint must define lint(variable) or lint_value(value)",
        ));
    }

    let mut diagnostics = Vec::new();
    if let Some(lint) = lint {
        let returned: LuaValue = lint
            .call(globals.get::<LuaValue>("variable").map_err(|err| {
                RototoError::new(format!("failed to read Lua variable context: {err}"))
            })?)
            .map_err(|err| RototoError::new(format!("custom lint failed: {err}")))?;
        diagnostics.extend(diagnostics_from_lua(&variable, returned, &custom_rules)?);
    }

    if let Some(lint_value) = lint_value {
        let Some(values) = variable_toml.get("values").and_then(Value::as_table) else {
            return Ok(diagnostics);
        };

        for (name, value) in values {
            let value_context = lua
                .to_value(&serde_json::json!({
                    "name": name,
                    "value": serde_json::to_value(value)
                        .map_err(|err| RototoError::new(err.to_string()))?,
                    "variable": {
                        "id": variable.id,
                        "uri": variable.uri,
                        "path": variable.path,
                    },
                }))
                .map_err(|err| {
                    RototoError::new(format!("failed to prepare Lua value context: {err}"))
                })?;
            let returned: LuaValue = lint_value
                .call(value_context)
                .map_err(|err| RototoError::new(format!("custom value lint failed: {err}")))?;
            diagnostics.extend(diagnostics_from_lua(&variable, returned, &custom_rules)?);
        }
    }

    Ok(diagnostics)
}

fn lint_path(variable_toml: &Value) -> Option<&str> {
    variable_toml
        .get("lint")
        .and_then(|lint| lint.get("path"))
        .and_then(Value::as_str)
}

fn diagnostics_from_lua(
    variable: &VariableInspection,
    returned: LuaValue,
    custom_rules: &[CustomRuleDefinition],
) -> Result<Vec<Diagnostic>> {
    match returned {
        LuaValue::Nil => Ok(Vec::new()),
        LuaValue::Table(table) => diagnostics_from_table(variable, table, custom_rules),
        other => Err(RototoError::new(format!(
            "custom lint must return a list of diagnostics, got {}",
            lua_type_name(&other)
        ))),
    }
}

fn diagnostics_from_table(
    variable: &VariableInspection,
    table: Table,
    custom_rules: &[CustomRuleDefinition],
) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    for entry in table.sequence_values::<Table>() {
        let entry = entry.map_err(|err| {
            RototoError::new(format!("custom lint returned an invalid diagnostic: {err}"))
        })?;
        let rule = entry
            .get::<Option<String>>("rule")
            .map_err(|err| RototoError::new(format!("custom lint rule is invalid: {err}")))?;
        let message = entry
            .get::<Option<String>>("message")
            .map_err(|err| RototoError::new(format!("custom lint message is invalid: {err}")))?
            .ok_or_else(|| RototoError::new("custom lint diagnostic must contain message"))?;

        let Some(rule) = rule else {
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::CustomLintInvalidRule,
                variable.path.display().to_string(),
                "custom lint diagnostic must contain rule",
            ));
            continue;
        };
        let rule = match CustomRuleId::parse(&rule) {
            Ok(rule) => rule,
            Err(err) => {
                diagnostics.push(Diagnostic::rototo(
                    RototoRuleId::CustomLintInvalidRule,
                    variable.path.display().to_string(),
                    format!("custom lint emitted invalid rule {rule}: {err}"),
                ));
                continue;
            }
        };
        let Some(definition) = custom_rules
            .iter()
            .find(|definition| definition.rule == rule)
        else {
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::CustomLintUnknownRule,
                variable.path.display().to_string(),
                format!("custom lint emitted undeclared rule: {rule}"),
            ));
            continue;
        };

        diagnostics.push(Diagnostic::custom(
            definition,
            variable.path.display().to_string(),
            message,
        ));
    }
    Ok(diagnostics)
}

fn lua_type_name(value: &LuaValue) -> &'static str {
    match value {
        LuaValue::Nil => "nil",
        LuaValue::Boolean(_) => "boolean",
        LuaValue::LightUserData(_) => "lightuserdata",
        LuaValue::Integer(_) => "integer",
        LuaValue::Number(_) => "number",
        LuaValue::String(_) => "string",
        LuaValue::Table(_) => "table",
        LuaValue::Function(_) => "function",
        LuaValue::Thread(_) => "thread",
        LuaValue::UserData(_) => "userdata",
        LuaValue::Error(_) => "error",
        LuaValue::Other(_) => "other",
    }
}
