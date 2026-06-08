use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jdouble, jlong, jstring};
use rototo::{
    LintMode, LoadOptions, RefreshOptions, ResolveContext, ResolveOptions, SourceAuth,
    SourceFingerprint, SourceOptions,
};
use serde_json::Value as JsonValue;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex;

struct JavaRefreshingWorkspace {
    inner: Mutex<Option<rototo::RefreshingWorkspace>>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_versionNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jstring {
    jni_call_string(&mut env, |env| env_string(env, env!("CARGO_PKG_VERSION")))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_workspaceLoadNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    source: JString<'_>,
    workspace_token: JString<'_>,
    lint: JString<'_>,
) -> jlong {
    jni_call_long(&mut env, |env| {
        let source = required_string(env, source, "source")?;
        let workspace_token = optional_string(env, workspace_token)?;
        let lint = required_string(env, lint, "lint")?;
        let options = load_options(workspace_token, &lint)?;
        let workspace = runtime()
            .block_on(rototo::Workspace::load_with_options(source, options))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(workspace)) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_workspaceInspectNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    source: JString<'_>,
    workspace_token: JString<'_>,
) -> jlong {
    jni_call_long(&mut env, |env| {
        let source = required_string(env, source, "source")?;
        let options = source_options(optional_string(env, workspace_token)?);
        let workspace = runtime()
            .block_on(rototo::Workspace::inspect_with_source_options(
                source, &options,
            ))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(workspace)) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_workspaceRootNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = workspace_from_handle(handle)?;
        env_string(env, &workspace.root().display().to_string())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_workspaceLintNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = workspace_from_handle(handle)?;
        let lint = runtime()
            .block_on(workspace.lint())
            .map_err(|err| err.to_string())?;
        let value = serde_json::json!({
            "root": lint.root.display().to_string(),
            "diagnostics": lint.diagnostics,
        });
        env_json(env, value)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_workspaceResolveVariableNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = workspace_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = resolve_context(env, context_json)?;
        let resolution = runtime()
            .block_on(workspace.resolve_variable_with_options(
                &id,
                &context,
                resolve_options(validate_context),
            ))
            .map_err(|err| err.to_string())?;
        env_json(
            env,
            serde_json::json!({
                "id": resolution.id,
                "valueKey": resolution.value_key,
                "value": resolution.value,
            }),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_workspaceResolveQualifierNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = workspace_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = resolve_context(env, context_json)?;
        let resolution = runtime()
            .block_on(workspace.resolve_qualifier_with_options(
                &id,
                &context,
                resolve_options(validate_context),
            ))
            .map_err(|err| err.to_string())?;
        env_json(
            env,
            serde_json::json!({
                "id": resolution.id,
                "value": resolution.value,
            }),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_workspaceFreeNative(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut rototo::Workspace));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_refreshingWorkspaceLoadNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    source: JString<'_>,
    period_seconds: jdouble,
    has_period_seconds: jboolean,
    workspace_token: JString<'_>,
    lint: JString<'_>,
) -> jlong {
    jni_call_long(&mut env, |env| {
        let source = required_string(env, source, "source")?;
        let workspace_token = optional_string(env, workspace_token)?;
        let lint = required_string(env, lint, "lint")?;
        let load_options = load_options(workspace_token, &lint)?;
        let refresh_options = refresh_options(period_seconds, has_period_seconds)?;
        let workspace = runtime()
            .block_on(rototo::RefreshingWorkspace::load_with_options(
                source,
                load_options,
                refresh_options,
            ))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(JavaRefreshingWorkspace {
            inner: Mutex::new(Some(workspace)),
        })) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_refreshingWorkspaceResolveVariableNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = refreshing_workspace_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = resolve_context(env, context_json)?;
        let resolution = runtime().block_on(async {
            let guard = workspace.inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            workspace
                .resolve_variable_with_options(&id, &context, resolve_options(validate_context))
                .await
                .map_err(|err| err.to_string())
        })?;
        env_json(
            env,
            serde_json::json!({
                "id": resolution.id,
                "valueKey": resolution.value_key,
                "value": resolution.value,
            }),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_refreshingWorkspaceResolveQualifierNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = refreshing_workspace_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = resolve_context(env, context_json)?;
        let resolution = runtime().block_on(async {
            let guard = workspace.inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            workspace
                .resolve_qualifier_with_options(&id, &context, resolve_options(validate_context))
                .await
                .map_err(|err| err.to_string())
        })?;
        env_json(
            env,
            serde_json::json!({
                "id": resolution.id,
                "value": resolution.value,
            }),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_refreshingWorkspaceRefreshNowNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = refreshing_workspace_from_handle(handle)?;
        let outcome = runtime().block_on(async {
            let guard = workspace.inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            workspace.refresh_now().await.map_err(|err| err.to_string())
        })?;
        env_string(env, refresh_outcome_name(outcome))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_refreshingWorkspaceStatusNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let workspace = refreshing_workspace_from_handle(handle)?;
        let status = runtime().block_on(async {
            let guard = workspace.inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            Ok::<_, String>(workspace.status().await)
        })?;
        env_json(env, refresh_status_to_json(status))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_refreshingWorkspaceShutdownNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    let _ = jni_call_unit(&mut env, |_| {
        let workspace = refreshing_workspace_from_handle(handle)?;
        runtime().block_on(async {
            let workspace = {
                let mut guard = workspace.inner.lock().await;
                guard.take()
            };
            if let Some(workspace) = workspace {
                workspace.shutdown().await;
            }
            Ok::<_, String>(())
        })
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rototo_Native_refreshingWorkspaceFreeNative(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut JavaRefreshingWorkspace));
        }
    }
}

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build rototo Java SDK runtime")
    })
}

fn source_options(workspace_token: Option<String>) -> SourceOptions {
    match workspace_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
        None => SourceOptions::new(),
    }
}

fn load_options(workspace_token: Option<String>, lint: &str) -> Result<LoadOptions, String> {
    let lint = match lint {
        "deny" => LintMode::Deny,
        "skip" => LintMode::Skip,
        other => return Err(format!("lint must be 'deny' or 'skip', got {other:?}")),
    };
    Ok(LoadOptions::new()
        .with_lint(lint)
        .with_source_auth(match workspace_token {
            Some(token) => SourceAuth::Bearer(token),
            None => SourceAuth::None,
        }))
}

fn refresh_options(
    period_seconds: jdouble,
    has_period_seconds: jboolean,
) -> Result<RefreshOptions, String> {
    let mut options = RefreshOptions::new();
    if has_period_seconds != 0 {
        if !period_seconds.is_finite() || period_seconds <= 0.0 {
            return Err("periodSeconds must be a positive finite number".to_owned());
        }
        options = options.with_period(Duration::from_secs_f64(period_seconds));
    }
    Ok(options)
}

fn resolve_options(validate_context: jboolean) -> ResolveOptions {
    ResolveOptions {
        validate_context: validate_context != 0,
    }
}

fn resolve_context(env: &mut JNIEnv<'_>, value: JString<'_>) -> Result<ResolveContext, String> {
    let context_json = required_string(env, value, "contextJson")?;
    let context: JsonValue = serde_json::from_str(&context_json)
        .map_err(|err| format!("failed to parse JSON context: {err}"))?;
    ResolveContext::from_json(context).map_err(|err| err.to_string())
}

fn workspace_from_handle(handle: jlong) -> Result<&'static rototo::Workspace, String> {
    if handle == 0 {
        return Err("workspace has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const rototo::Workspace) })
}

fn refreshing_workspace_from_handle(
    handle: jlong,
) -> Result<&'static JavaRefreshingWorkspace, String> {
    if handle == 0 {
        return Err("refreshing workspace has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const JavaRefreshingWorkspace) })
}

fn active_refreshing_workspace(
    guard: &Option<rototo::RefreshingWorkspace>,
) -> Result<&rototo::RefreshingWorkspace, String> {
    guard
        .as_ref()
        .ok_or_else(|| "refreshing workspace has been shut down".to_owned())
}

fn refresh_status_to_json(status: rototo::RefreshStatus) -> JsonValue {
    serde_json::json!({
        "currentFingerprint": status.current_fingerprint.as_ref().map(source_fingerprint_to_json),
        "lastSuccess": system_time_to_unix_seconds(status.last_success),
        "lastAttempt": system_time_to_unix_seconds(status.last_attempt),
        "consecutiveFailures": status.consecutive_failures,
        "lastError": status.last_error,
        "refreshing": status.refreshing,
        "immutable": status.immutable,
    })
}

fn source_fingerprint_to_json(fingerprint: &SourceFingerprint) -> JsonValue {
    match fingerprint {
        SourceFingerprint::GitCommit(value) => {
            serde_json::json!({ "kind": "git_commit", "value": value })
        }
        SourceFingerprint::HttpValidator(value) => {
            serde_json::json!({ "kind": "http_validator", "value": value })
        }
        SourceFingerprint::ContentHash(value) => {
            serde_json::json!({ "kind": "content_hash", "value": value })
        }
        SourceFingerprint::WorkspaceLayers(layers) => serde_json::json!({
            "kind": "workspace_layers",
            "layers": layers.iter().map(source_fingerprint_to_json).collect::<Vec<_>>(),
        }),
    }
}

fn system_time_to_unix_seconds(time: Option<SystemTime>) -> Option<f64> {
    time.and_then(|value| {
        value
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs_f64())
    })
}

fn refresh_outcome_name(outcome: rototo::RefreshOutcome) -> &'static str {
    match outcome {
        rototo::RefreshOutcome::Unchanged => "unchanged",
        rototo::RefreshOutcome::Refreshed => "refreshed",
        rototo::RefreshOutcome::Immutable => "immutable",
    }
}

fn required_string(env: &mut JNIEnv<'_>, value: JString<'_>, name: &str) -> Result<String, String> {
    if value.is_null() {
        return Err(format!("{name} must not be null"));
    }
    env.get_string(&value)
        .map(|value| value.into())
        .map_err(|err| err.to_string())
}

fn optional_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    env.get_string(&value)
        .map(|value| Some(value.into()))
        .map_err(|err| err.to_string())
}

fn env_json(env: &mut JNIEnv<'_>, value: JsonValue) -> Result<jstring, String> {
    let text = serde_json::to_string(&value).map_err(|err| err.to_string())?;
    env_string(env, &text)
}

fn env_string(env: &mut JNIEnv<'_>, value: &str) -> Result<jstring, String> {
    env.new_string(value)
        .map(|value| value.into_raw())
        .map_err(|err| err.to_string())
}

fn jni_call_long(
    env: &mut JNIEnv<'_>,
    f: impl FnOnce(&mut JNIEnv<'_>) -> Result<jlong, String>,
) -> jlong {
    match f(env) {
        Ok(value) => value,
        Err(err) => {
            throw_rototo(env, err);
            0
        }
    }
}

fn jni_call_string(
    env: &mut JNIEnv<'_>,
    f: impl FnOnce(&mut JNIEnv<'_>) -> Result<jstring, String>,
) -> jstring {
    match f(env) {
        Ok(value) => value,
        Err(err) => {
            throw_rototo(env, err);
            std::ptr::null_mut()
        }
    }
}

fn jni_call_unit(
    env: &mut JNIEnv<'_>,
    f: impl FnOnce(&mut JNIEnv<'_>) -> Result<(), String>,
) -> Result<(), ()> {
    match f(env) {
        Ok(()) => Ok(()),
        Err(err) => {
            throw_rototo(env, err);
            Err(())
        }
    }
}

fn throw_rototo(env: &mut JNIEnv<'_>, message: String) {
    if env
        .throw_new("com/rototo/RototoException", message.as_str())
        .is_err()
    {
        let _ = env.throw_new("java/lang/RuntimeException", message);
    }
}
