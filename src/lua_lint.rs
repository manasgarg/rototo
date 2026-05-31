use std::path::PathBuf;

use mlua::{Lua, LuaSerdeExt, Table, Value as LuaValue};
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};

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
