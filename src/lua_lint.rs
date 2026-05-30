use std::path::{Path, PathBuf};

use mlua::{Lua, LuaSerdeExt, Table, Value as LuaValue};
use toml::Value;

use crate::diagnostics::{CustomRuleDefinition, CustomRuleId, Diagnostic, RototoRuleId};
use crate::error::{Result, RototoError};
use crate::model::VariableInspection;

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
