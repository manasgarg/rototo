use std::path::{Path, PathBuf};

use mlua::{Lua, LuaSerdeExt, Table, Value as LuaValue};
use toml::Value;

use crate::diagnostics::{Diagnostic, DiagnosticSource, LintRule, VARIABLE_CUSTOM_LINT_FAILED};
use crate::error::{Result, RototoError};
use crate::model::VariableInspection;

const RULE_VARIABLE_CUSTOM_LINT: LintRule = LintRule {
    id: "rototo/variable/custom-lint/failed",
    title: "Variable custom lint failed",
    help: "Update the variable or its Lua lint rule so custom lint passes.",
};

pub async fn lint_variable(
    workspace_root: &Path,
    variable: &VariableInspection,
    variable_toml: &Value,
    environments: &[String],
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
    tokio::task::spawn_blocking(move || {
        lint_variable_script(
            workspace_root,
            variable,
            variable_toml,
            environments,
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
        diagnostics.extend(diagnostics_from_lua(&variable, returned)?);
    }

    if let Some(lint_value) = lint_value {
        let Some(values) = variable_toml
            .get("variable")
            .and_then(|variable| variable.get("values"))
            .and_then(Value::as_table)
        else {
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
            diagnostics.extend(diagnostics_from_lua(&variable, returned)?);
        }
    }

    Ok(diagnostics)
}

fn lint_path(variable_toml: &Value) -> Option<&str> {
    variable_toml
        .get("variable")
        .and_then(|variable| variable.get("lint"))
        .and_then(|lint| lint.get("path"))
        .and_then(Value::as_str)
}

fn diagnostics_from_lua(
    variable: &VariableInspection,
    returned: LuaValue,
) -> Result<Vec<Diagnostic>> {
    match returned {
        LuaValue::Nil => Ok(Vec::new()),
        LuaValue::Table(table) => diagnostics_from_table(variable, table),
        other => Err(RototoError::new(format!(
            "custom lint must return a list of diagnostics, got {}",
            lua_type_name(&other)
        ))),
    }
}

fn diagnostics_from_table(variable: &VariableInspection, table: Table) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    for entry in table.sequence_values::<Table>() {
        let entry = entry.map_err(|err| {
            RototoError::new(format!("custom lint returned an invalid diagnostic: {err}"))
        })?;
        let message = entry
            .get::<Option<String>>("message")
            .map_err(|err| RototoError::new(format!("custom lint message is invalid: {err}")))?
            .ok_or_else(|| RototoError::new("custom lint diagnostic must contain message"))?;
        let help = entry
            .get::<Option<String>>("help")
            .map_err(|err| RototoError::new(format!("custom lint help is invalid: {err}")))?
            .unwrap_or_else(|| VARIABLE_CUSTOM_LINT_FAILED.help.to_owned());

        diagnostics.push(
            Diagnostic::new_rule(
                VARIABLE_CUSTOM_LINT_FAILED,
                DiagnosticSource::Custom,
                variable.path.display().to_string(),
                message,
                RULE_VARIABLE_CUSTOM_LINT,
            )
            .with_kind("variable")
            .with_details(serde_json::json!({
                "title": VARIABLE_CUSTOM_LINT_FAILED.title,
                "help": help,
            })),
        );
        if let Some(diagnostic) = diagnostics.last_mut() {
            diagnostic.help = help;
        }
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
