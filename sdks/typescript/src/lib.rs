use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rototo::{
    LintMode, LoadOptions, RefreshOptions, ResolveContext, ResolveOptions, SourceAuth,
    SourceFingerprint, SourceOptions,
};
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;

#[napi]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[napi(js_name = "_Workspace")]
pub struct JsWorkspace {
    inner: Arc<rototo::Workspace>,
}

#[napi]
impl JsWorkspace {
    #[napi(factory)]
    pub async fn load(
        source: String,
        workspace_token: Option<String>,
        lint: Option<String>,
    ) -> Result<Self> {
        let options = load_options(workspace_token, lint.as_deref())?;
        let workspace = rototo::Workspace::load_with_options(source, options)
            .await
            .map_err(js_err)?;
        Ok(Self {
            inner: Arc::new(workspace),
        })
    }

    #[napi(factory)]
    pub async fn inspect(source: String, workspace_token: Option<String>) -> Result<Self> {
        let options = source_options(workspace_token);
        let workspace = rototo::Workspace::inspect_with_source_options(source, &options)
            .await
            .map_err(js_err)?;
        Ok(Self {
            inner: Arc::new(workspace),
        })
    }

    #[napi]
    pub fn root(&self) -> String {
        self.inner.root().display().to_string()
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
    pub async fn resolve_variable(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<JsonValue> {
        let context = ResolveContext::from_json(context).map_err(js_err)?;
        let resolution = self
            .inner
            .resolve_variable_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .await
            .map_err(js_err)?;
        Ok(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
            "source": resolution.source,
        }))
    }

    #[napi(js_name = "resolveQualifier")]
    pub async fn resolve_qualifier(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<JsonValue> {
        let context = ResolveContext::from_json(context).map_err(js_err)?;
        let resolution = self
            .inner
            .resolve_qualifier_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .await
            .map_err(js_err)?;
        Ok(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
        }))
    }
}

#[napi(js_name = "_RefreshingWorkspace")]
pub struct JsRefreshingWorkspace {
    inner: Arc<Mutex<Option<rototo::RefreshingWorkspace>>>,
}

#[napi]
impl JsRefreshingWorkspace {
    #[napi(factory)]
    pub async fn load(
        source: String,
        period_seconds: Option<f64>,
        workspace_token: Option<String>,
        lint: Option<String>,
    ) -> Result<Self> {
        let load_options = load_options(workspace_token, lint.as_deref())?;
        let refresh_options = refresh_options(period_seconds)?;
        let workspace =
            rototo::RefreshingWorkspace::load_with_options(source, load_options, refresh_options)
                .await
                .map_err(js_err)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Some(workspace))),
        })
    }

    #[napi(js_name = "resolveVariable")]
    pub async fn resolve_variable(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<JsonValue> {
        let context = ResolveContext::from_json(context).map_err(js_err)?;
        let guard = self.inner.lock().await;
        let workspace = active_refreshing_workspace(&guard)?;
        let resolution = workspace
            .resolve_variable_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .await
            .map_err(js_err)?;
        Ok(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
            "source": resolution.source,
        }))
    }

    #[napi(js_name = "resolveQualifier")]
    pub async fn resolve_qualifier(
        &self,
        id: String,
        context: JsonValue,
        validate_context: Option<bool>,
    ) -> Result<JsonValue> {
        let context = ResolveContext::from_json(context).map_err(js_err)?;
        let guard = self.inner.lock().await;
        let workspace = active_refreshing_workspace(&guard)?;
        let resolution = workspace
            .resolve_qualifier_with_options(
                &id,
                &context,
                resolve_options(validate_context.unwrap_or(true)),
            )
            .await
            .map_err(js_err)?;
        Ok(serde_json::json!({
            "id": resolution.id,
            "value": resolution.value,
        }))
    }

    #[napi(js_name = "refreshNow")]
    pub async fn refresh_now(&self) -> Result<String> {
        let guard = self.inner.lock().await;
        let workspace = active_refreshing_workspace(&guard)?;
        let outcome = workspace.refresh_now().await.map_err(js_err)?;
        Ok(refresh_outcome_name(outcome).to_owned())
    }

    #[napi]
    pub async fn status(&self) -> Result<JsonValue> {
        let guard = self.inner.lock().await;
        let workspace = active_refreshing_workspace(&guard)?;
        let status = workspace.status().await;
        Ok(refresh_status_to_json(status))
    }

    #[napi]
    pub async fn shutdown(&self) -> Result<()> {
        let workspace = {
            let mut guard = self.inner.lock().await;
            guard.take()
        };
        if let Some(workspace) = workspace {
            workspace.shutdown().await;
        }
        Ok(())
    }
}

fn active_refreshing_workspace(
    guard: &Option<rototo::RefreshingWorkspace>,
) -> Result<&rototo::RefreshingWorkspace> {
    guard
        .as_ref()
        .ok_or_else(|| Error::from_reason("refreshing workspace has been shut down"))
}

fn source_options(workspace_token: Option<String>) -> SourceOptions {
    match workspace_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
        None => SourceOptions::new(),
    }
}

fn load_options(workspace_token: Option<String>, lint: Option<&str>) -> Result<LoadOptions> {
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
        .with_source_auth(match workspace_token {
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
    ResolveOptions { validate_context }
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
