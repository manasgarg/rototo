use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jdouble, jlong, jstring};
use rototo::{
    EvaluationContext, LintMode, LoadOptions, RefreshOptions, ResolveOptions, SourceAuth,
    SourceFingerprint, SourceOptions, TraceSubscription,
};
use serde_json::Value as JsonValue;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{Mutex, broadcast};

struct JavaRefreshingPackage {
    inner: Mutex<Option<rototo::RefreshingPackage>>,
}

struct JavaRefreshEvents {
    rx: Mutex<broadcast::Receiver<rototo::RefreshEvent>>,
}

struct JavaTraceEvents {
    subscription: Mutex<TraceSubscription>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_versionNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jstring {
    jni_call_string(&mut env, |env| env_string(env, env!("CARGO_PKG_VERSION")))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageLoadNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    source: JString<'_>,
    package_token: JString<'_>,
    lint: JString<'_>,
) -> jlong {
    jni_call_long(&mut env, |env| {
        let source = required_string(env, source, "source")?;
        let package_token = optional_string(env, package_token)?;
        let lint = required_string(env, lint, "lint")?;
        let options = load_options(package_token, &lint)?;
        let package = runtime()
            .block_on(rototo::Package::load_with_options(source, options))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(package)) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageInspectNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    source: JString<'_>,
    package_token: JString<'_>,
) -> jlong {
    jni_call_long(&mut env, |env| {
        let source = required_string(env, source, "source")?;
        let options = source_options(optional_string(env, package_token)?);
        let package = runtime()
            .block_on(rototo::Package::inspect_with_source_options(
                source, &options,
            ))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(package)) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageRootNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = package_from_handle(handle)?;
        env_string(env, &package.root().display().to_string())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageIdentityNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = package_from_handle(handle)?;
        env_json(env, package.identity().to_json())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageLintNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = package_from_handle(handle)?;
        let lint = runtime()
            .block_on(package.lint())
            .map_err(|err| err.to_string())?;
        let value = serde_json::json!({
            "root": lint.root.display().to_string(),
            "diagnostics": lint.diagnostics,
        });
        env_json(env, value)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageResolveVariableNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
    trace: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = package_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = evaluation_context(env, context_json)?;
        let resolution = package
            .resolve_variable_with_options(&id, &context, resolve_options(validate_context, trace))
            .map_err(|err| err.to_string())?;
        env_json(
            env,
            serde_json::json!({
                "id": resolution.id,
                "value": resolution.value,
                "source": resolution.source,
            }),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageResolveQualifierNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
    trace: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = package_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = evaluation_context(env, context_json)?;
        let value = package
            .resolve_qualifier_with_options(&id, &context, resolve_options(validate_context, trace))
            .map_err(|err| err.to_string())?;
        env_json(env, serde_json::json!(value))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_packageFreeNative(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut rototo::Package));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageLoadNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    source: JString<'_>,
    period_seconds: jdouble,
    has_period_seconds: jboolean,
    package_token: JString<'_>,
    lint: JString<'_>,
) -> jlong {
    jni_call_long(&mut env, |env| {
        let source = required_string(env, source, "source")?;
        let package_token = optional_string(env, package_token)?;
        let lint = required_string(env, lint, "lint")?;
        let load_options = load_options(package_token, &lint)?;
        let refresh_options = refresh_options(period_seconds, has_period_seconds)?;
        let package = runtime()
            .block_on(rototo::RefreshingPackage::load_with_options(
                source,
                load_options,
                refresh_options,
            ))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(JavaRefreshingPackage {
            inner: Mutex::new(Some(package)),
        })) as jlong)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageResolveVariableNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
    trace: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = refreshing_package_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = evaluation_context(env, context_json)?;
        let guard = package.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        let resolution = package
            .resolve_variable_with_options(&id, &context, resolve_options(validate_context, trace))
            .map_err(|err| err.to_string())?;
        env_json(
            env,
            serde_json::json!({
                "id": resolution.id,
                "value": resolution.value,
                "source": resolution.source,
            }),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageResolveQualifierNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
    context_json: JString<'_>,
    validate_context: jboolean,
    trace: jboolean,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = refreshing_package_from_handle(handle)?;
        let id = required_string(env, id, "id")?;
        let context = evaluation_context(env, context_json)?;
        let guard = package.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        let value = package
            .resolve_qualifier_with_options(&id, &context, resolve_options(validate_context, trace))
            .map_err(|err| err.to_string())?;
        env_json(env, serde_json::json!(value))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageRefreshNowNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = refreshing_package_from_handle(handle)?;
        let outcome = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            package.refresh_now().await.map_err(|err| err.to_string())
        })?;
        env_string(env, refresh_outcome_name(outcome))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageStatusNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = refreshing_package_from_handle(handle)?;
        let status = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            Ok::<_, String>(package.status())
        })?;
        env_json(env, refresh_status_to_json(status))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageIdentityNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = refreshing_package_from_handle(handle)?;
        let identity = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            Ok::<_, String>(package.identity())
        })?;
        env_json(env, identity.to_json())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageSnapshotNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let package = refreshing_package_from_handle(handle)?;
        let snapshot = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            Ok::<_, String>(package.snapshot())
        })?;
        env_json(env, snapshot.to_json())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageSubscribeEventsNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    jni_call_long(&mut env, |_env| {
        let package = refreshing_package_from_handle(handle)?;
        let rx = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            Ok::<_, String>(package.subscribe_refresh_events())
        })?;
        Ok(Box::into_raw(Box::new(JavaRefreshEvents { rx: Mutex::new(rx) })) as jlong)
    })
}

/// Block until the next refresh event. Returns the event JSON, or a null string
/// when the stream has closed (the package was shut down or freed). A lagging
/// subscriber skips the gap rather than erroring.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshEventsNextNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let events = refresh_events_from_handle(handle)?;
        let json = runtime().block_on(async {
            let mut rx = events.rx.lock().await;
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let text = serde_json::to_string(&event.to_json())
                            .map_err(|err| err.to_string())?;
                        return Ok::<Option<String>, String>(Some(text));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return Ok(None),
                }
            }
        })?;
        match json {
            Some(text) => env_string(env, &text),
            None => Ok(std::ptr::null_mut()),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshEventsFreeNative(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut JavaRefreshEvents));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageSubscribeTraceEventsNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jlong {
    jni_call_long(&mut env, |_env| {
        let package = refreshing_package_from_handle(handle)?;
        let subscription = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            Ok::<_, String>(package.subscribe_trace_events())
        })?;
        Ok(Box::into_raw(Box::new(JavaTraceEvents {
            subscription: Mutex::new(subscription),
        })) as jlong)
    })
}

/// Block until the next trace stream item. Returns the item JSON, or a null
/// string when the stream has closed. A lagging subscriber receives a
/// `{"kind":"dropped","count":n}` item.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_traceEventsNextNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    jni_call_string(&mut env, |env| {
        let events = trace_events_from_handle(handle)?;
        let json = runtime().block_on(async {
            let mut subscription = events.subscription.lock().await;
            match subscription.recv().await {
                Some(item) => Ok::<Option<String>, String>(Some(
                    serde_json::to_string(&item.to_json()).map_err(|err| err.to_string())?,
                )),
                None => Ok(None),
            }
        })?;
        match json {
            Some(text) => env_string(env, &text),
            None => Ok(std::ptr::null_mut()),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_traceEventsFreeNative(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut JavaTraceEvents));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageShutdownNative(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    let _ = jni_call_unit(&mut env, |_| {
        let package = refreshing_package_from_handle(handle)?;
        runtime().block_on(async {
            let package = {
                let mut guard = package.inner.lock().await;
                guard.take()
            };
            if let Some(package) = package {
                package.shutdown().await;
            }
            Ok::<_, String>(())
        })
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_rototo_Native_refreshingPackageFreeNative(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut JavaRefreshingPackage));
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

fn source_options(package_token: Option<String>) -> SourceOptions {
    match package_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
        None => SourceOptions::new(),
    }
}

fn load_options(package_token: Option<String>, lint: &str) -> Result<LoadOptions, String> {
    let lint = match lint {
        "deny" => LintMode::Deny,
        "skip" => LintMode::Skip,
        other => return Err(format!("lint must be 'deny' or 'skip', got {other:?}")),
    };
    Ok(LoadOptions::new()
        .with_lint(lint)
        .with_source_auth(match package_token {
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

fn resolve_options(validate_context: jboolean, trace: jboolean) -> ResolveOptions {
    ResolveOptions {
        validate_context: validate_context != 0,
        trace: trace != 0,
    }
}

fn evaluation_context(
    env: &mut JNIEnv<'_>,
    value: JString<'_>,
) -> Result<EvaluationContext, String> {
    let context_json = required_string(env, value, "contextJson")?;
    let context: JsonValue = serde_json::from_str(&context_json)
        .map_err(|err| format!("failed to parse JSON context: {err}"))?;
    EvaluationContext::from_json(context).map_err(|err| err.to_string())
}

fn package_from_handle(handle: jlong) -> Result<&'static rototo::Package, String> {
    if handle == 0 {
        return Err("package has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const rototo::Package) })
}

fn refreshing_package_from_handle(handle: jlong) -> Result<&'static JavaRefreshingPackage, String> {
    if handle == 0 {
        return Err("refreshing package has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const JavaRefreshingPackage) })
}

fn active_refreshing_package(
    guard: &Option<rototo::RefreshingPackage>,
) -> Result<&rototo::RefreshingPackage, String> {
    guard
        .as_ref()
        .ok_or_else(|| "refreshing package has been shut down".to_owned())
}

fn trace_events_from_handle(handle: jlong) -> Result<&'static JavaTraceEvents, String> {
    if handle == 0 {
        return Err("trace event stream has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const JavaTraceEvents) })
}

fn refresh_events_from_handle(handle: jlong) -> Result<&'static JavaRefreshEvents, String> {
    if handle == 0 {
        return Err("refresh event stream has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const JavaRefreshEvents) })
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
        SourceFingerprint::PackageLayers(layers) => serde_json::json!({
            "kind": "package_layers",
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
        .throw_new("dev/rototo/RototoException", message.as_str())
        .is_err()
    {
        let _ = env.throw_new("java/lang/RuntimeException", message);
    }
}
