use std::cell::Cell;
use std::path::{Component, Path, PathBuf};
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
    pub target: String,
    pub id: String,
    pub title: String,
    pub help: String,
    pub severity: Option<String>,
    pub handler: String,
    pub handler_exists: bool,
}

#[derive(Clone, Debug)]
pub struct RegisteredLintInput {
    pub package: JsonValue,
    pub target: JsonValue,
    pub lint_path: PathBuf,
    pub script: String,
    pub handler: String,
}

#[derive(Debug)]
pub struct RegisteredCustomLintOutput {
    pub message: String,
    pub path: Option<String>,
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
        .set_name(lua_chunk_name(&input.lint_path))
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
    let rule_registrations = registrations.clone();
    let rule = lua
        .create_function_mut(move |_, args: mlua::Variadic<LuaValue>| {
            let table = registration_arg_table(args)?;
            rule_registrations
                .borrow_mut()
                .push(registration_from_lua_table(table)?);
            Ok(())
        })
        .map_err(|err| RototoError::new(format!("failed to prepare custom lint API: {err}")))?;
    let lint_api = lua
        .create_table()
        .map_err(|err| RototoError::new(format!("failed to prepare custom lint API: {err}")))?;
    lint_api
        .set("rule", rule)
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
            "lint:rule expects a single registration table",
        )),
    }
}

fn registration_from_lua_table(table: Table) -> mlua::Result<RawCustomLintRegistration> {
    Ok(RawCustomLintRegistration {
        target: table
            .get::<Option<String>>("target")?
            .unwrap_or_else(|| "/".to_owned()),
        id: required_registration_string(&table, "id")?,
        title: required_registration_string(&table, "title")?,
        help: required_registration_string(&table, "help")?,
        severity: table.get::<Option<String>>("severity")?,
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
    bounded_lua_task("custom lint", move || lint_registered_target_script(input)).await
}

fn lint_registered_target_script(
    input: RegisteredLintInput,
) -> Result<Vec<RegisteredCustomLintOutput>> {
    let lua = custom_lint_lua()?;
    lua.load(&input.script)
        .set_name(lua_chunk_name(&input.lint_path))
        .exec()
        .map_err(|err| RototoError::new(format!("failed to execute custom lint: {err}")))?;

    let globals = lua.globals();
    let handler = globals
        .get::<mlua::Function>(input.handler.as_str())
        .map_err(|err| RototoError::new(format!("custom lint handler is invalid: {err}")))?;
    let package = lua
        .to_value(&input.package)
        .map_err(|err| RototoError::new(format!("failed to prepare Lua package: {err}")))?;
    let target = lua
        .to_value(&input.target)
        .map_err(|err| RototoError::new(format!("failed to prepare Lua context: {err}")))?;
    let returned: LuaValue = handler
        .call((package, target))
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

fn lua_chunk_name(lint_path: &Path) -> String {
    format!("={}", safe_lua_chunk_label(lint_path))
}

fn safe_lua_chunk_label(lint_path: &Path) -> String {
    let segments = lint_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    if let Some(index) = segments.iter().rposition(|segment| segment == "lint") {
        return segments[index..].join("/");
    }

    lint_path
        .file_name()
        .map(|file_name| format!("lint/{}", file_name.to_string_lossy()))
        .unwrap_or_else(|| "lint/custom.lua".to_owned())
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
                let path = entry
                    .get::<Option<String>>("path")
                    .map_err(|err| RototoError::new(format!("custom lint path is invalid: {err}")))?
                    .or(entry.get::<Option<String>>("field").map_err(|err| {
                        RototoError::new(format!("custom lint field is invalid: {err}"))
                    })?);
                diagnostics.push(RegisteredCustomLintOutput { message, path });
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
                  lint:rule({
                    id = "policy/sandbox",
                    title = "Sandbox",
                    help = "Sandbox policy.",
                    handler = "check"
                  })
                end

                function check(package, target)
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
    async fn custom_lint_registration_errors_use_safe_chunk_names() {
        let err = register_pipeline_lint(RegisterLintInput {
            lint_path: PathBuf::from(
                "/tmp/.tmpWrGs2H/clone/examples/basic/lint/checkout-redesign.lua",
            ),
            script: "function register(lint)\n  lint:on({})\nend".to_owned(),
        })
        .await
        .unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("lint/checkout-redesign.lua:2"),
            "{message}"
        );
        assert!(!message.contains("/tmp"), "{message}");
        assert!(!message.contains("clone/examples/basic"), "{message}");
    }

    #[tokio::test]
    async fn custom_lint_handler_loop_is_bounded() {
        let err = lint_registered_target(RegisteredLintInput {
            package: serde_json::json!({}),
            target: serde_json::json!({}),
            lint_path: PathBuf::from("lint/loop.lua"),
            script: r#"
                function check(package, target)
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

    #[tokio::test]
    async fn custom_lint_handler_errors_use_safe_chunk_names() {
        let err = lint_registered_target(RegisteredLintInput {
            package: serde_json::json!({}),
            target: serde_json::json!({}),
            lint_path: PathBuf::from(
                "/tmp/.tmpWrGs2H/clone/examples/basic/lint/checkout-redesign.lua",
            ),
            script: r#"
                function check(package, target)
                  error("handler exploded")
                end
            "#
            .to_owned(),
            handler: "check".to_owned(),
        })
        .await
        .unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("lint/checkout-redesign.lua:3"),
            "{message}"
        );
        assert!(!message.contains("/tmp"), "{message}");
        assert!(!message.contains("clone/examples/basic"), "{message}");
    }
}
