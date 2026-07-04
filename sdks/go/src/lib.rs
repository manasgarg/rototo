use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rototo::{
    EvaluationContext, LintMode, LoadOptions, RefreshOptions, ResolveOptions, SourceAuth,
    SourceFingerprint, SourceOptions, TraceSubscription,
};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{Mutex, broadcast};

#[repr(C)]
pub struct RototoGoStringResult {
    value: *mut c_char,
    error: *mut c_char,
}

#[repr(C)]
pub struct RototoGoHandleResult {
    handle: *mut c_void,
    error: *mut c_char,
}

#[repr(C)]
pub struct RototoGoVoidResult {
    error: *mut c_char,
}

struct GoRefreshingPackage {
    inner: Mutex<Option<rototo::RefreshingPackage>>,
}

struct GoRefreshEvents {
    rx: Mutex<broadcast::Receiver<rototo::RefreshEvent>>,
}

struct GoTraceEvents {
    subscription: Mutex<TraceSubscription>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_version() -> RototoGoStringResult {
    string_result(|| Ok(env!("CARGO_PKG_VERSION").to_owned()))
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_load(
    source: *const c_char,
    package_token: *const c_char,
    lint: *const c_char,
    fallback_source: *const c_char,
) -> RototoGoHandleResult {
    handle_result(|| {
        let source = required_string(source, "source")?;
        let package_token = optional_string(package_token)?;
        let lint = required_string(lint, "lint")?;
        let fallback_source = optional_string(fallback_source)?;
        let options = load_options(package_token, &lint, fallback_source)?;
        let package = runtime()
            .block_on(rototo::Package::load_with_options(source, options))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(package)) as *mut c_void)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_inspect(
    source: *const c_char,
    package_token: *const c_char,
) -> RototoGoHandleResult {
    handle_result(|| {
        let source = required_string(source, "source")?;
        let options = source_options(optional_string(package_token)?);
        let package = runtime()
            .block_on(rototo::Package::inspect_with_source_options(
                source, &options,
            ))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(package)) as *mut c_void)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_root(handle: *mut c_void) -> RototoGoStringResult {
    string_result(|| {
        let package = package_from_handle(handle)?;
        Ok(package.root().display().to_string())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_served_fallback(handle: *mut c_void) -> c_int {
    let Ok(package) = package_from_handle(handle) else {
        return 0;
    };
    c_int::from(package.served_fallback())
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_identity(handle: *mut c_void) -> RototoGoStringResult {
    string_result(|| {
        let package = package_from_handle(handle)?;
        json_string(package.identity().to_json())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_lint(handle: *mut c_void) -> RototoGoStringResult {
    string_result(|| {
        let package = package_from_handle(handle)?;
        let lint = runtime()
            .block_on(package.lint())
            .map_err(|err| err.to_string())?;
        json_string(serde_json::json!({
            "root": lint.root.display().to_string(),
            "diagnostics": lint.diagnostics,
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_resolve_variable(
    handle: *mut c_void,
    id: *const c_char,
    context_json: *const c_char,
    validate_context: c_int,
    trace: c_int,
) -> RototoGoStringResult {
    string_result(|| {
        let package = package_from_handle(handle)?;
        let id = required_string(id, "id")?;
        let context = evaluation_context(context_json)?;
        let options = resolve_options(validate_context, trace);
        let resolution = package
            .resolve_variable_with_options(&id, &context, options)
            .map_err(|err| err.to_string())?;
        json_string(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
            "source": resolution.source,
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_package_free(handle: *mut c_void) {
    if !handle.is_null() {
        unsafe {
            drop(Box::from_raw(handle as *mut rototo::Package));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_load(
    source: *const c_char,
    period_seconds: c_double,
    has_period_seconds: c_int,
    package_token: *const c_char,
    lint: *const c_char,
    fallback_source: *const c_char,
) -> RototoGoHandleResult {
    handle_result(|| {
        let source = required_string(source, "source")?;
        let package_token = optional_string(package_token)?;
        let lint = required_string(lint, "lint")?;
        let fallback_source = optional_string(fallback_source)?;
        let load_options = load_options(package_token, &lint, fallback_source)?;
        let refresh_options = refresh_options(period_seconds, has_period_seconds)?;
        let package = runtime()
            .block_on(rototo::RefreshingPackage::load_with_options(
                source,
                load_options,
                refresh_options,
            ))
            .map_err(|err| err.to_string())?;
        Ok(Box::into_raw(Box::new(GoRefreshingPackage {
            inner: Mutex::new(Some(package)),
        })) as *mut c_void)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_resolve_variable(
    handle: *mut c_void,
    id: *const c_char,
    context_json: *const c_char,
    validate_context: c_int,
    trace: c_int,
) -> RototoGoStringResult {
    string_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        let id = required_string(id, "id")?;
        let context = evaluation_context(context_json)?;
        let options = resolve_options(validate_context, trace);
        let guard = package.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        let resolution = package
            .resolve_variable_with_options(&id, &context, options)
            .map_err(|err| err.to_string())?;
        json_string(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
            "source": resolution.source,
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_refresh_now(
    handle: *mut c_void,
) -> RototoGoStringResult {
    string_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        let outcome = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            package.refresh_now().await.map_err(|err| err.to_string())
        })?;
        Ok(refresh_outcome_name(outcome).to_owned())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_status(handle: *mut c_void) -> RototoGoStringResult {
    string_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        let status = runtime().block_on(async {
            let guard = package.inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            Ok::<_, String>(package.status())
        })?;
        json_string(refresh_status_to_json(status))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_identity(
    handle: *mut c_void,
) -> RototoGoStringResult {
    string_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        let guard = package.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        json_string(package.identity().to_json())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_snapshot(
    handle: *mut c_void,
) -> RototoGoStringResult {
    string_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        let guard = package.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        json_string(package.snapshot().to_json())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_subscribe_events(
    handle: *mut c_void,
) -> RototoGoHandleResult {
    handle_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        let guard = package.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        let rx = package.subscribe_refresh_events();
        Ok(Box::into_raw(Box::new(GoRefreshEvents { rx: Mutex::new(rx) })) as *mut c_void)
    })
}

/// Block until the next refresh event. Returns the event JSON on `value`; a
/// closed stream (the package was shut down or freed) returns null `value` and
/// null `error` to signal end-of-stream. A lagging subscriber skips the gap.
#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refresh_events_next(handle: *mut c_void) -> RototoGoStringResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let events = refresh_events_from_handle(handle)?;
        runtime().block_on(async {
            let mut rx = events.rx.lock().await;
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        return Ok::<Option<String>, String>(Some(json_string(event.to_json())?));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return Ok(None),
                }
            }
        })
    })) {
        Ok(Ok(Some(json))) => RototoGoStringResult {
            value: c_string_ptr(json),
            error: ptr::null_mut(),
        },
        Ok(Ok(None)) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: ptr::null_mut(),
        },
        Ok(Err(error)) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: c_string_ptr(error),
        },
        Err(_) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: c_string_ptr("rototo Go native call panicked"),
        },
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refresh_events_free(handle: *mut c_void) {
    if !handle.is_null() {
        unsafe {
            drop(Box::from_raw(handle as *mut GoRefreshEvents));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_subscribe_trace_events(
    handle: *mut c_void,
) -> RototoGoHandleResult {
    handle_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        let guard = package.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        let subscription = package.subscribe_trace_events();
        Ok(Box::into_raw(Box::new(GoTraceEvents {
            subscription: Mutex::new(subscription),
        })) as *mut c_void)
    })
}

/// Block until the next trace stream item. Returns the item JSON on `value`; a
/// closed stream returns null `value` and null `error` to signal end-of-stream.
/// A lagging subscriber receives a `{"kind":"dropped","count":n}` item.
#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_trace_events_next(handle: *mut c_void) -> RototoGoStringResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let events = trace_events_from_handle(handle)?;
        runtime().block_on(async {
            let mut subscription = events.subscription.lock().await;
            match subscription.recv().await {
                Some(item) => Ok::<Option<String>, String>(Some(json_string(item.to_json())?)),
                None => Ok(None),
            }
        })
    })) {
        Ok(Ok(Some(json))) => RototoGoStringResult {
            value: c_string_ptr(json),
            error: ptr::null_mut(),
        },
        Ok(Ok(None)) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: ptr::null_mut(),
        },
        Ok(Err(error)) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: c_string_ptr(error),
        },
        Err(_) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: c_string_ptr("rototo Go native call panicked"),
        },
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_trace_events_free(handle: *mut c_void) {
    if !handle.is_null() {
        unsafe {
            drop(Box::from_raw(handle as *mut GoTraceEvents));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_shutdown(handle: *mut c_void) -> RototoGoVoidResult {
    void_result(|| {
        let package = refreshing_package_from_handle(handle)?;
        runtime().block_on(async {
            let package = {
                let mut guard = package.inner.lock().await;
                guard.take()
            };
            if let Some(package) = package {
                package.shutdown().await;
            }
        });
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rototo_go_refreshing_package_free(handle: *mut c_void) {
    if !handle.is_null() {
        unsafe {
            drop(Box::from_raw(handle as *mut GoRefreshingPackage));
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be null or a pointer to a `RototoGoStringResult` returned by
/// this library. Call this at most once for each result value.
pub unsafe extern "C" fn rototo_go_string_result_free(result: *mut RototoGoStringResult) {
    if result.is_null() {
        return;
    }
    unsafe {
        free_c_string((*result).value);
        free_c_string((*result).error);
        (*result).value = ptr::null_mut();
        (*result).error = ptr::null_mut();
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be null or a pointer to a `RototoGoHandleResult` returned by
/// this library. Call this at most once for each result value.
pub unsafe extern "C" fn rototo_go_handle_result_free(result: *mut RototoGoHandleResult) {
    if result.is_null() {
        return;
    }
    unsafe {
        free_c_string((*result).error);
        (*result).error = ptr::null_mut();
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be null or a pointer to a `RototoGoVoidResult` returned by
/// this library. Call this at most once for each result value.
pub unsafe extern "C" fn rototo_go_void_result_free(result: *mut RototoGoVoidResult) {
    if result.is_null() {
        return;
    }
    unsafe {
        free_c_string((*result).error);
        (*result).error = ptr::null_mut();
    }
}

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("rototo Go SDK runtime should initialize")
    })
}

fn string_result(func: impl FnOnce() -> Result<String, String>) -> RototoGoStringResult {
    match catch_unwind(AssertUnwindSafe(func)) {
        Ok(Ok(value)) => RototoGoStringResult {
            value: c_string_ptr(value),
            error: ptr::null_mut(),
        },
        Ok(Err(error)) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: c_string_ptr(error),
        },
        Err(_) => RototoGoStringResult {
            value: ptr::null_mut(),
            error: c_string_ptr("rototo Go native call panicked"),
        },
    }
}

fn handle_result(func: impl FnOnce() -> Result<*mut c_void, String>) -> RototoGoHandleResult {
    match catch_unwind(AssertUnwindSafe(func)) {
        Ok(Ok(handle)) => RototoGoHandleResult {
            handle,
            error: ptr::null_mut(),
        },
        Ok(Err(error)) => RototoGoHandleResult {
            handle: ptr::null_mut(),
            error: c_string_ptr(error),
        },
        Err(_) => RototoGoHandleResult {
            handle: ptr::null_mut(),
            error: c_string_ptr("rototo Go native call panicked"),
        },
    }
}

fn void_result(func: impl FnOnce() -> Result<(), String>) -> RototoGoVoidResult {
    match catch_unwind(AssertUnwindSafe(func)) {
        Ok(Ok(())) => RototoGoVoidResult {
            error: ptr::null_mut(),
        },
        Ok(Err(error)) => RototoGoVoidResult {
            error: c_string_ptr(error),
        },
        Err(_) => RototoGoVoidResult {
            error: c_string_ptr("rototo Go native call panicked"),
        },
    }
}

fn c_string_ptr(value: impl AsRef<str>) -> *mut c_char {
    match CString::new(value.as_ref()) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new("rototo Go native string contained a NUL byte")
            .expect("static string should not contain NUL")
            .into_raw(),
    }
}

unsafe fn free_c_string(value: *mut c_char) {
    if !value.is_null() {
        unsafe {
            drop(CString::from_raw(value));
        }
    }
}

fn required_string(value: *const c_char, name: &str) -> Result<String, String> {
    if value.is_null() {
        return Err(format!("{name} is required"));
    }
    unsafe { c_string(value) }
}

fn optional_string(value: *const c_char) -> Result<Option<String>, String> {
    if value.is_null() {
        Ok(None)
    } else {
        unsafe { c_string(value).map(Some) }
    }
}

unsafe fn c_string(value: *const c_char) -> Result<String, String> {
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "string must be valid UTF-8".to_owned())
}

fn source_options(package_token: Option<String>) -> SourceOptions {
    match package_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
        None => SourceOptions::new(),
    }
}

fn load_options(
    package_token: Option<String>,
    lint: &str,
    fallback_source: Option<String>,
) -> Result<LoadOptions, String> {
    let lint = match lint {
        "deny" => LintMode::Deny,
        "skip" => LintMode::Skip,
        other => {
            return Err(format!("lint must be 'deny' or 'skip', got {other:?}"));
        }
    };
    let mut options = LoadOptions::new()
        .with_lint(lint)
        .with_source_auth(match package_token {
            Some(token) => SourceAuth::Bearer(token),
            None => SourceAuth::None,
        });
    if let Some(fallback) = fallback_source {
        options = options.with_fallback_source(fallback);
    }
    Ok(options)
}

fn refresh_options(
    period_seconds: c_double,
    has_period_seconds: c_int,
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

fn evaluation_context(context_json: *const c_char) -> Result<EvaluationContext, String> {
    let context_json = required_string(context_json, "context_json")?;
    let context = serde_json::from_str(&context_json)
        .map_err(|err| format!("evaluation context must be valid JSON: {err}"))?;
    EvaluationContext::from_json(context).map_err(|err| err.to_string())
}

fn resolve_options(validate_context: c_int, trace: c_int) -> ResolveOptions {
    ResolveOptions {
        validate_context: validate_context != 0,
        trace: trace != 0,
    }
}

fn package_from_handle<'a>(handle: *mut c_void) -> Result<&'a rototo::Package, String> {
    if handle.is_null() {
        return Err("package has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const rototo::Package) })
}

fn refreshing_package_from_handle<'a>(
    handle: *mut c_void,
) -> Result<&'a GoRefreshingPackage, String> {
    if handle.is_null() {
        return Err("refreshing package has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const GoRefreshingPackage) })
}

fn active_refreshing_package(
    guard: &Option<rototo::RefreshingPackage>,
) -> Result<&rototo::RefreshingPackage, String> {
    guard
        .as_ref()
        .ok_or_else(|| "refreshing package has been shut down".to_owned())
}

fn refresh_events_from_handle<'a>(handle: *mut c_void) -> Result<&'a GoRefreshEvents, String> {
    if handle.is_null() {
        return Err("refresh event stream has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const GoRefreshEvents) })
}

fn trace_events_from_handle<'a>(handle: *mut c_void) -> Result<&'a GoTraceEvents, String> {
    if handle.is_null() {
        return Err("trace event stream has been closed".to_owned());
    }
    Ok(unsafe { &*(handle as *const GoTraceEvents) })
}

fn json_string(value: serde_json::Value) -> Result<String, String> {
    serde_json::to_string(&value).map_err(|err| err.to_string())
}

fn refresh_status_to_json(status: rototo::RefreshStatus) -> serde_json::Value {
    serde_json::json!({
        "currentFingerprint": status.current_fingerprint.as_ref().map(source_fingerprint_to_json),
        "lastSuccess": system_time_to_unix_seconds(status.last_success),
        "lastAttempt": system_time_to_unix_seconds(status.last_attempt),
        "consecutiveFailures": status.consecutive_failures,
        "lastError": status.last_error,
        "refreshing": status.refreshing,
        "immutable": status.immutable,
        "servingFallback": status.serving_fallback,
    })
}

fn source_fingerprint_to_json(fingerprint: &SourceFingerprint) -> serde_json::Value {
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
