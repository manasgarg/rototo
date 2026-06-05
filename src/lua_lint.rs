use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, Value as LuaValue, VmState};
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};

const LUA_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const LUA_INSTRUCTION_LIMIT: u64 = 1_000_000;
const LUA_HOOK_INTERVAL: u32 = 1_000;
const LUA_TASK_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct RegisterLintInput {
    pub lint_path: PathBuf,
    pub script: String,
}

#[derive(Clone, Debug)]
pub struct RawCustomLintRegistration {
    pub stage: String,
    pub entity: String,
    pub field: Option<String>,
    pub rule: RawCustomLintRule,
    pub handler: String,
    pub handler_exists: bool,
}

#[derive(Clone, Debug)]
pub struct RawCustomLintRule {
    pub id: String,
    pub title: String,
    pub help: String,
    pub severity: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RegisteredLintInput {
    pub stage: String,
    pub target: RegisteredLintTarget,
    pub lint_path: PathBuf,
    pub script: String,
    pub handler: String,
}

#[derive(Clone, Debug)]
pub struct RegisteredLintTarget {
    pub entity: String,
    pub data: JsonValue,
}

#[derive(Debug)]
pub struct RegisteredCustomLintOutput {
    pub message: String,
    pub field: Option<String>,
}

pub async fn register_pipeline_lint(
    input: RegisterLintInput,
) -> Result<Vec<RawCustomLintRegistration>> {
    bounded_lua_task("custom lint registration", move || {
        register_pipeline_lint_script(input)
    })
    .await
}

fn register_pipeline_lint_script(
    input: RegisterLintInput,
) -> Result<Vec<RawCustomLintRegistration>> {
    let lua = custom_lint_lua()?;
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
        return Err(RototoError::new("custom lint must define register(lint)"));
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
        rule: registration_rule_from_lua_table(&table)?,
        handler: required_registration_string(&table, "handler")?,
        handler_exists: false,
    })
}

fn registration_rule_from_lua_table(table: &Table) -> mlua::Result<RawCustomLintRule> {
    let rule = table
        .get::<Option<Table>>("rule")?
        .ok_or_else(|| mlua::Error::external("registration must contain rule metadata"))?;
    Ok(RawCustomLintRule {
        id: required_registration_string(&rule, "id")?,
        title: required_registration_string(&rule, "title")?,
        help: required_registration_string(&rule, "help")?,
        severity: rule.get::<Option<String>>("severity")?,
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
    bounded_lua_task("custom lint", move || lint_registered_target_script(input)).await
}

fn lint_registered_target_script(
    input: RegisteredLintInput,
) -> Result<Vec<RegisteredCustomLintOutput>> {
    let lua = custom_lint_lua()?;
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

async fn bounded_lua_task<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::time::timeout(LUA_TASK_TIMEOUT, tokio::task::spawn_blocking(task))
        .await
        .map_err(|_| RototoError::new(format!("{label} exceeded execution timeout")))?
        .map_err(|err| RototoError::new(format!("{label} task failed: {err}")))?
}

fn custom_lint_lua() -> Result<Lua> {
    let libs = StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
    let lua = Lua::new_with(libs, LuaOptions::default())
        .map_err(|err| RototoError::new(format!("failed to prepare custom lint VM: {err}")))?;
    lua.set_memory_limit(LUA_MEMORY_LIMIT_BYTES)
        .map_err(|err| {
            RototoError::new(format!("failed to set custom lint memory limit: {err}"))
        })?;

    let globals = lua.globals();
    for global in [
        "os",
        "io",
        "package",
        "require",
        "dofile",
        "loadfile",
        "load",
        "collectgarbage",
        "debug",
    ] {
        globals.set(global, LuaValue::Nil).map_err(|err| {
            RototoError::new(format!(
                "failed to restrict custom lint global {global}: {err}"
            ))
        })?;
    }

    let started = Instant::now();
    let instructions = Rc::new(Cell::new(0_u64));
    let hook_instructions = instructions.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(LUA_HOOK_INTERVAL),
        move |_, _| {
            let executed = hook_instructions
                .get()
                .saturating_add(u64::from(LUA_HOOK_INTERVAL));
            hook_instructions.set(executed);
            if executed > LUA_INSTRUCTION_LIMIT || started.elapsed() > LUA_TASK_TIMEOUT {
                return Err(mlua::Error::external(
                    "custom lint exceeded Lua execution limits",
                ));
            }
            Ok(VmState::Continue)
        },
    );

    Ok(lua)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn custom_lint_sandbox_denies_file_process_state_and_require_globals() {
        let registrations = register_pipeline_lint(RegisterLintInput {
            lint_path: PathBuf::from("lint/sandbox.lua"),
            script: r#"
                assert(os == nil, "os global is available")
                assert(io == nil, "io global is available")
                assert(package == nil, "package global is available")
                assert(require == nil, "require global is available")
                assert(dofile == nil, "dofile global is available")
                assert(loadfile == nil, "loadfile global is available")

                function register(lint)
                  lint:on({
                    stage = "policy",
                    entity = "workspace",
                    rule = {
                      id = "policy/sandbox",
                      title = "Sandbox",
                      help = "Sandbox policy.",
                    },
                    handler = "check"
                  })
                end

                function check(ctx)
                  return {}
                end
            "#
            .to_owned(),
        })
        .await
        .unwrap();

        assert_eq!(registrations.len(), 1);
        assert!(registrations[0].handler_exists);
    }

    #[tokio::test]
    async fn custom_lint_registration_loop_is_bounded() {
        let err = register_pipeline_lint(RegisterLintInput {
            lint_path: PathBuf::from("lint/loop.lua"),
            script: "while true do end".to_owned(),
        })
        .await
        .unwrap_err();

        assert!(
            err.to_string().contains("execution limits")
                || err.to_string().contains("execution timeout"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn custom_lint_without_register_is_an_error() {
        let err = register_pipeline_lint(RegisterLintInput {
            lint_path: PathBuf::from("lint/no-register.lua"),
            script: "function check(ctx) return {} end".to_owned(),
        })
        .await
        .unwrap_err();

        assert!(err.to_string().contains("must define register"), "{err}");
    }

    #[tokio::test]
    async fn custom_lint_handler_loop_is_bounded() {
        let err = lint_registered_target(RegisteredLintInput {
            stage: "policy".to_owned(),
            target: RegisteredLintTarget {
                entity: "workspace".to_owned(),
                data: serde_json::json!({}),
            },
            lint_path: PathBuf::from("lint/loop.lua"),
            script: r#"
                function check(ctx)
                  while true do end
                end
            "#
            .to_owned(),
            handler: "check".to_owned(),
        })
        .await
        .unwrap_err();

        assert!(
            err.to_string().contains("execution limits")
                || err.to_string().contains("execution timeout"),
            "{err}"
        );
    }
}
