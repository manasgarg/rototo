use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rototo::{
    EvaluationContext, LintMode, LoadOptions, RefreshOptions, ResolveOptions, SourceAuth,
    SourceFingerprint, SourceOptions, TraceSubscription,
};
use serde_json::Value as JsonValue;
use tokio::sync::{Mutex, broadcast};

#[napi]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[napi(js_name = "_Package")]
pub struct JsPackage {
    inner: Arc<rototo::Package>,
}

#[napi]
impl JsPackage {
    #[napi(factory)]
    pub async fn load(
        source: String,
        package_token: Option<String>,
        lint: Option<String>,
    ) -> Result<Self> {
        let options = load_options(package_token, lint.as_deref())?;
        let package = rototo::Package::load_with_options(source, options)
            .await
            .map_err(js_err)?;
        Ok(Self {
            inner: Arc::new(package),
        })
    }

    #[napi(factory)]
    pub async fn inspect(source: String, package_token: Option<String>) -> Result<Self> {
        let options = source_options(package_token);
        let package = rototo::Package::inspect_with_source_options(source, &options)
            .await
            .map_err(js_err)?;
        Ok(Self {
            inner: Arc::new(package),
        })
    }

    #[napi]
    pub fn root(&self) -> String {
        self.inner.root().display().to_string()
    }

    #[napi]
    pub fn identity(&self) -> JsonValue {
        self.inner.identity().to_json()
    }

    #[napi]
    pub async fn lint(&self) -> Result<JsonValue> {
        let lint = self.inner.lint().await.map_err(js_err)?;
        Ok(serde_json::json!({
            "root": lint.root.display().to_string(),
            "diagnostics": lint.diagnostics,
        }))
    }

    #[napi(js_name = "semanticModel")]
    pub async fn semantic_model(&self) -> Result<JsonValue> {
        let model = self.inner.semantic_model().await.map_err(js_err)?;
        serde_json::to_value(model).map_err(|err| js_err(rototo::RototoError::new(err.to_string())))
    }

    #[napi(js_name = "resolveVariable")]
    pub fn resolve_variable(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<JsonValue> {
        let context = EvaluationContext::from_json(context).map_err(js_err)?;
        let resolution = self
            .inner
            .resolve_variable_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .map_err(js_err)?;
        Ok(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
            "source": resolution.source,
        }))
    }

    #[napi(js_name = "resolveQualifier")]
    pub fn resolve_qualifier(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<bool> {
        let context = EvaluationContext::from_json(context).map_err(js_err)?;
        self.inner
            .resolve_qualifier_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .map_err(js_err)
    }

    #[napi(js_name = "subscribeTraceEvents")]
    pub fn subscribe_trace_events(&self) -> JsTraceEvents {
        JsTraceEvents {
            subscription: Arc::new(Mutex::new(self.inner.subscribe_trace_events())),
        }
    }
}

#[napi(js_name = "_RefreshingPackage")]
pub struct JsRefreshingPackage {
    inner: Arc<Mutex<Option<rototo::RefreshingPackage>>>,
}

#[napi]
impl JsRefreshingPackage {
    #[napi(factory)]
    pub async fn load(
        source: String,
        period_seconds: Option<f64>,
        package_token: Option<String>,
        lint: Option<String>,
    ) -> Result<Self> {
        let load_options = load_options(package_token, lint.as_deref())?;
        let refresh_options = refresh_options(period_seconds)?;
        let package =
            rototo::RefreshingPackage::load_with_options(source, load_options, refresh_options)
                .await
                .map_err(js_err)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Some(package))),
        })
    }

    #[napi(js_name = "resolveVariable")]
    pub fn resolve_variable(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<JsonValue> {
        let context = EvaluationContext::from_json(context).map_err(js_err)?;
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        let resolution = package
            .resolve_variable_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .map_err(js_err)?;
        Ok(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
            "source": resolution.source,
        }))
    }

    #[napi(js_name = "resolveQualifier")]
    pub fn resolve_qualifier(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<bool> {
        let context = EvaluationContext::from_json(context).map_err(js_err)?;
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        package
            .resolve_qualifier_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .map_err(js_err)
    }

    #[napi(js_name = "refreshNow")]
    pub async fn refresh_now(&self) -> Result<String> {
        let guard = self.inner.lock().await;
        let package = active_refreshing_package(&guard)?;
        let outcome = package.refresh_now().await.map_err(js_err)?;
        Ok(refresh_outcome_name(outcome).to_owned())
    }

    #[napi]
    pub async fn status(&self) -> Result<JsonValue> {
        let guard = self.inner.lock().await;
        let package = active_refreshing_package(&guard)?;
        let status = package.status();
        Ok(refresh_status_to_json(status))
    }

    #[napi]
    pub async fn identity(&self) -> Result<JsonValue> {
        let guard = self.inner.lock().await;
        let package = active_refreshing_package(&guard)?;
        Ok(package.identity().to_json())
    }

    #[napi]
    pub async fn snapshot(&self) -> Result<JsonValue> {
        let guard = self.inner.lock().await;
        let package = active_refreshing_package(&guard)?;
        Ok(package.snapshot().to_json())
    }

    #[napi(js_name = "subscribeEvents")]
    pub fn subscribe_events(&self) -> Result<JsRefreshEvents> {
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        Ok(JsRefreshEvents {
            rx: Arc::new(Mutex::new(package.subscribe_refresh_events())),
        })
    }

    #[napi(js_name = "subscribeTraceEvents")]
    pub fn subscribe_trace_events(&self) -> Result<JsTraceEvents> {
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        Ok(JsTraceEvents {
            subscription: Arc::new(Mutex::new(package.subscribe_trace_events())),
        })
    }

    #[napi]
    pub async fn shutdown(&self) -> Result<()> {
        let package = {
            let mut guard = self.inner.lock().await;
            guard.take()
        };
        if let Some(package) = package {
            package.shutdown().await;
        }
        Ok(())
    }
}

#[napi(js_name = "_RefreshEvents")]
pub struct JsRefreshEvents {
    rx: Arc<Mutex<broadcast::Receiver<rototo::RefreshEvent>>>,
}

#[napi]
impl JsRefreshEvents {
    /// Resolve to the next refresh event, or `null` when the stream has closed
    /// (the package was shut down or dropped). A lagging subscriber skips the
    /// gap rather than erroring; recover ground truth from `snapshot()`.
    #[napi]
    pub async fn recv(&self) -> Result<Option<JsonValue>> {
        let mut rx = self.rx.lock().await;
        loop {
            match rx.recv().await {
                Ok(event) => return Ok(Some(event.to_json())),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return Ok(None),
            }
        }
    }
}

#[napi(js_name = "_TraceEvents")]
pub struct JsTraceEvents {
    subscription: Arc<Mutex<TraceSubscription>>,
}

#[napi]
impl JsTraceEvents {
    /// Resolve to the next trace stream item, or `null` when the stream has
    /// closed. A lagging subscriber receives a `{ kind: "dropped", count }` item
    /// rather than erroring.
    #[napi]
    pub async fn recv(&self) -> Result<Option<JsonValue>> {
        let mut subscription = self.subscription.lock().await;
        Ok(subscription.recv().await.map(|item| item.to_json()))
    }
}

fn active_refreshing_package(
    guard: &Option<rototo::RefreshingPackage>,
) -> Result<&rototo::RefreshingPackage> {
    guard
        .as_ref()
        .ok_or_else(|| Error::from_reason("refreshing package has been shut down"))
}

fn source_options(package_token: Option<String>) -> SourceOptions {
    match package_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
        None => SourceOptions::new(),
    }
}

fn load_options(package_token: Option<String>, lint: Option<&str>) -> Result<LoadOptions> {
    let lint = match lint.unwrap_or("deny") {
        "deny" => LintMode::Deny,
        "skip" => LintMode::Skip,
        other => {
            return Err(Error::from_reason(format!(
                "lint must be 'deny' or 'skip', got {other:?}"
            )));
        }
    };
    Ok(LoadOptions::new()
        .with_lint(lint)
        .with_source_auth(match package_token {
            Some(token) => SourceAuth::Bearer(token),
            None => SourceAuth::None,
        }))
}

fn refresh_options(period_seconds: Option<f64>) -> Result<RefreshOptions> {
    let mut options = RefreshOptions::new();
    if let Some(seconds) = period_seconds {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(Error::from_reason(
                "periodSeconds must be a positive finite number",
            ));
        }
        options = options.with_period(Duration::from_secs_f64(seconds));
    }
    Ok(options)
}

fn resolve_options(validate_context: bool) -> ResolveOptions {
    ResolveOptions {
        validate_context,
        ..ResolveOptions::default()
    }
}

fn js_err(err: rototo::RototoError) -> Error {
    Error::from_reason(err.to_string())
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
