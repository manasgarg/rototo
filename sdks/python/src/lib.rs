use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyModule};
use pyo3_async_runtimes::tokio::future_into_py;
use pythonize::{depythonize, pythonize};
use rototo::{
    EvaluationContext, LintMode, LoadOptions, PackageIdentity, PackageLayerIdentity, RefreshEvent,
    RefreshEventSummary, RefreshOptions, RefreshSnapshot, ResolveOptions, SourceAuth,
    SourceFingerprint, SourceOptions, TraceStreamItem, TraceSubscription,
};
use serde_json::Value as JsonValue;
use tokio::sync::{Mutex, broadcast};

pyo3::create_exception!(_rototo, RototoError, pyo3::exceptions::PyException);

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pyclass(name = "_Package")]
struct PyPackage {
    inner: Arc<rototo::Package>,
}

#[pymethods]
impl PyPackage {
    #[staticmethod]
    #[pyo3(signature = (source, *, package_token = None, lint = "deny"))]
    fn load<'py>(
        py: Python<'py>,
        source: String,
        package_token: Option<String>,
        lint: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let options = load_options(package_token, lint)?;
        future_into_py(py, async move {
            let package = rototo::Package::load_with_options(source, options)
                .await
                .map_err(py_err)?;
            Python::attach(|py| {
                Py::new(
                    py,
                    PyPackage {
                        inner: Arc::new(package),
                    },
                )
            })
        })
    }

    #[staticmethod]
    #[pyo3(signature = (source, *, package_token = None))]
    fn inspect<'py>(
        py: Python<'py>,
        source: String,
        package_token: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let options = source_options(package_token);
        future_into_py(py, async move {
            let package = rototo::Package::inspect_with_source_options(source, &options)
                .await
                .map_err(py_err)?;
            Python::attach(|py| {
                Py::new(
                    py,
                    PyPackage {
                        inner: Arc::new(package),
                    },
                )
            })
        })
    }

    fn root(&self) -> String {
        self.inner.root().display().to_string()
    }

    fn identity(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(py, &package_identity_to_json(&self.inner.identity()))
    }

    fn lint<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let package = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let lint = package.lint().await.map_err(py_err)?;
            Python::attach(|py| lint_to_py(py, lint))
        })
    }

    #[pyo3(signature = (id, context, *, validate_context = true))]
    fn resolve_variable<'py>(
        &self,
        py: Python<'py>,
        id: String,
        context: Bound<'py, PyAny>,
        validate_context: bool,
    ) -> PyResult<Py<PyAny>> {
        let context = json_from_py(&context)?;
        let context = EvaluationContext::from_json(context).map_err(py_err)?;
        let resolution = self
            .inner
            .resolve_variable_with_options(&id, &context, resolve_options(validate_context))
            .map_err(py_err)?;
        variable_resolution_to_py(py, resolution)
    }

    #[pyo3(signature = (id, context, *, validate_context = true))]
    fn resolve_qualifier<'py>(
        &self,
        _py: Python<'py>,
        id: String,
        context: Bound<'py, PyAny>,
        validate_context: bool,
    ) -> PyResult<bool> {
        let context = json_from_py(&context)?;
        let context = EvaluationContext::from_json(context).map_err(py_err)?;
        self.inner
            .resolve_qualifier_with_options(&id, &context, resolve_options(validate_context))
            .map_err(py_err)
    }

    fn subscribe_trace_events(&self) -> PyTraceEvents {
        PyTraceEvents {
            subscription: Arc::new(Mutex::new(self.inner.subscribe_trace_events())),
        }
    }
}

#[pyclass(name = "_RefreshingPackage")]
struct PyRefreshingPackage {
    inner: Arc<Mutex<Option<rototo::RefreshingPackage>>>,
}

#[pymethods]
impl PyRefreshingPackage {
    #[staticmethod]
    #[pyo3(signature = (source, *, period_seconds = None, package_token = None, lint = "deny"))]
    fn load<'py>(
        py: Python<'py>,
        source: String,
        period_seconds: Option<f64>,
        package_token: Option<String>,
        lint: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let load_options = load_options(package_token, lint)?;
        let refresh_options = refresh_options(period_seconds)?;
        future_into_py(py, async move {
            let package =
                rototo::RefreshingPackage::load_with_options(source, load_options, refresh_options)
                    .await
                    .map_err(py_err)?;
            Python::attach(|py| {
                Py::new(
                    py,
                    PyRefreshingPackage {
                        inner: Arc::new(Mutex::new(Some(package))),
                    },
                )
            })
        })
    }

    #[pyo3(signature = (id, context, *, validate_context = true))]
    fn resolve_variable<'py>(
        &self,
        py: Python<'py>,
        id: String,
        context: Bound<'py, PyAny>,
        validate_context: bool,
    ) -> PyResult<Py<PyAny>> {
        let context = json_from_py(&context)?;
        let context = EvaluationContext::from_json(context).map_err(py_err)?;
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        let resolution = package
            .resolve_variable_with_options(&id, &context, resolve_options(validate_context))
            .map_err(py_err)?;
        variable_resolution_to_py(py, resolution)
    }

    #[pyo3(signature = (id, context, *, validate_context = true))]
    fn resolve_qualifier<'py>(
        &self,
        _py: Python<'py>,
        id: String,
        context: Bound<'py, PyAny>,
        validate_context: bool,
    ) -> PyResult<bool> {
        let context = json_from_py(&context)?;
        let context = EvaluationContext::from_json(context).map_err(py_err)?;
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        package
            .resolve_qualifier_with_options(&id, &context, resolve_options(validate_context))
            .map_err(py_err)
    }

    fn refresh_now<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let guard = inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            let outcome = package.refresh_now().await.map_err(py_err)?;
            Ok(refresh_outcome_name(outcome).to_owned())
        })
    }

    fn status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let guard = inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            let status = package.status();
            Python::attach(|py| refresh_status_to_py(py, status))
        })
    }

    fn identity<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let guard = inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            let identity = package.identity();
            Python::attach(|py| json_to_py(py, &package_identity_to_json(&identity)))
        })
    }

    fn snapshot<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let guard = inner.lock().await;
            let package = active_refreshing_package(&guard)?;
            let snapshot = package.snapshot();
            Python::attach(|py| json_to_py(py, &refresh_snapshot_to_json(&snapshot)))
        })
    }

    fn subscribe_events(&self) -> PyResult<PyRefreshEvents> {
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        Ok(PyRefreshEvents {
            rx: Arc::new(Mutex::new(package.subscribe_refresh_events())),
        })
    }

    fn subscribe_trace_events(&self) -> PyResult<PyTraceEvents> {
        let guard = self.inner.blocking_lock();
        let package = active_refreshing_package(&guard)?;
        Ok(PyTraceEvents {
            subscription: Arc::new(Mutex::new(package.subscribe_trace_events())),
        })
    }

    fn shutdown<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let package = {
                let mut guard = inner.lock().await;
                guard.take()
            };
            if let Some(package) = package {
                package.shutdown().await;
            }
            Ok(())
        })
    }
}

#[pyclass(name = "_RefreshEvents")]
struct PyRefreshEvents {
    rx: Arc<Mutex<broadcast::Receiver<RefreshEvent>>>,
}

#[pymethods]
impl PyRefreshEvents {
    /// Resolve to the next refresh event, or `None` when the stream has closed
    /// (the package was shut down or dropped). A lagging subscriber that missed
    /// events skips the gap rather than erroring; recover ground truth from
    /// `snapshot()`.
    fn recv<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let rx = Arc::clone(&self.rx);
        future_into_py(py, async move {
            let mut rx = rx.lock().await;
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let json = refresh_event_to_json(&event);
                        return Python::attach(|py| json_to_py(py, &json).map(Some));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return Ok(None),
                }
            }
        })
    }
}

#[pyclass(name = "_TraceEvents")]
struct PyTraceEvents {
    subscription: Arc<Mutex<TraceSubscription>>,
}

#[pymethods]
impl PyTraceEvents {
    /// Resolve to the next trace stream item as a dict, or `None` when the
    /// stream has closed. A lagging subscriber receives a `{"kind": "dropped",
    /// "count": n}` item rather than erroring.
    fn recv<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let subscription = Arc::clone(&self.subscription);
        future_into_py(py, async move {
            let mut subscription = subscription.lock().await;
            match subscription.recv().await {
                Some(item) => {
                    let json = trace_stream_item_to_json(&item);
                    Python::attach(|py| json_to_py(py, &json).map(Some))
                }
                None => Ok(None),
            }
        })
    }
}

fn trace_stream_item_to_json(item: &TraceStreamItem) -> JsonValue {
    item.to_json()
}

fn active_refreshing_package(
    guard: &Option<rototo::RefreshingPackage>,
) -> PyResult<&rototo::RefreshingPackage> {
    guard
        .as_ref()
        .ok_or_else(|| RototoError::new_err("refreshing package has been shut down"))
}

fn source_options(package_token: Option<String>) -> SourceOptions {
    match package_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
        None => SourceOptions::new(),
    }
}

fn load_options(package_token: Option<String>, lint: &str) -> PyResult<LoadOptions> {
    let lint = match lint {
        "deny" => LintMode::Deny,
        "skip" => LintMode::Skip,
        other => {
            return Err(PyValueError::new_err(format!(
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

fn refresh_options(period_seconds: Option<f64>) -> PyResult<RefreshOptions> {
    let mut options = RefreshOptions::new();
    if let Some(seconds) = period_seconds {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(PyValueError::new_err(
                "period_seconds must be a positive finite number",
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

fn json_from_py(value: &Bound<'_, PyAny>) -> PyResult<JsonValue> {
    depythonize(value).map_err(|err| {
        RototoError::new_err(format!("failed to convert Python value to JSON: {err}"))
    })
}

fn py_err(err: rototo::RototoError) -> PyErr {
    RototoError::new_err(err.to_string())
}

fn lint_to_py(py: Python<'_>, lint: rototo::model::PackageLint) -> PyResult<Py<PyAny>> {
    let value = serde_json::json!({
        "root": lint.root.display().to_string(),
        "diagnostics": lint.diagnostics,
    });
    json_to_py(py, &value)
}

fn variable_resolution_to_py(
    py: Python<'_>,
    resolution: rototo::model::VariableResolution,
) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("id", resolution.id)?;
    dict.set_item("value", pythonize(py, &resolution.value)?)?;
    dict.set_item(
        "source",
        pythonize(
            py,
            &serde_json::to_value(resolution.source).unwrap_or(JsonValue::Null),
        )?,
    )?;
    Ok(dict.into_any().unbind())
}

fn refresh_status_to_py(py: Python<'_>, status: rototo::RefreshStatus) -> PyResult<Py<PyAny>> {
    let value = serde_json::json!({
        "current_fingerprint": status.current_fingerprint.as_ref().map(source_fingerprint_to_json),
        "last_success": system_time_to_unix_seconds(status.last_success),
        "last_attempt": system_time_to_unix_seconds(status.last_attempt),
        "consecutive_failures": status.consecutive_failures,
        "last_error": status.last_error,
        "refreshing": status.refreshing,
        "immutable": status.immutable,
    });
    json_to_py(py, &value)
}

fn json_to_py(py: Python<'_>, value: &JsonValue) -> PyResult<Py<PyAny>> {
    Ok(pythonize(py, value)?.unbind())
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

fn package_identity_to_json(identity: &PackageIdentity) -> JsonValue {
    serde_json::json!({
        "source": identity.source.as_str(),
        "fingerprint": identity.fingerprint.as_ref().map(source_fingerprint_to_json),
        "release_id": identity.release_id,
        "loaded_at": system_time_to_unix_seconds(Some(identity.loaded_at)),
        "immutable": identity.immutable,
        "layers": identity
            .layers
            .iter()
            .map(package_layer_identity_to_json)
            .collect::<Vec<_>>(),
    })
}

fn package_layer_identity_to_json(layer: &PackageLayerIdentity) -> JsonValue {
    serde_json::json!({
        "source": layer.source.as_str(),
        "fingerprint": layer.fingerprint.as_ref().map(source_fingerprint_to_json),
        "release_id": layer.release_id,
        "immutable": layer.immutable,
    })
}

fn refresh_snapshot_to_json(snapshot: &RefreshSnapshot) -> JsonValue {
    serde_json::json!({
        "identity": package_identity_to_json(&snapshot.identity),
        "last_attempt": system_time_to_unix_seconds(snapshot.last_attempt),
        "last_success": system_time_to_unix_seconds(snapshot.last_success),
        "last_event": snapshot.last_event.as_ref().map(refresh_event_summary_to_json),
        "consecutive_failures": snapshot.consecutive_failures,
        "last_error": snapshot.last_error,
        "refreshing": snapshot.refreshing,
        "immutable": snapshot.immutable,
    })
}

fn refresh_event_summary_to_json(summary: &RefreshEventSummary) -> JsonValue {
    serde_json::json!({
        "event_id": summary.event_id.to_string(),
        "event_type": summary.event_type.as_str(),
        "release_id": summary.release_id,
        "completed_at": system_time_to_unix_seconds(Some(summary.completed_at)),
    })
}

fn refresh_event_to_json(event: &RefreshEvent) -> JsonValue {
    serde_json::json!({
        "schema_version": 1,
        "event_id": event.event_id.to_string(),
        "event_type": event.event_type.as_str(),
        "source": event.source.as_str(),
        "previous": event.previous.as_ref().map(package_identity_to_json),
        "current": event.current.as_ref().map(package_identity_to_json),
        "attempted_at": system_time_to_unix_seconds(Some(event.attempted_at)),
        "completed_at": system_time_to_unix_seconds(Some(event.completed_at)),
        "duration_ms": event.duration.as_millis() as u64,
        "outcome": event.outcome.map(refresh_outcome_name),
        "consecutive_failures": event.consecutive_failures,
        "error": event.error,
        "sdk": {
            "name": event.sdk.name,
            "version": event.sdk.version,
            "language": event.sdk.language,
        },
    })
}

#[pymodule]
fn _rototo(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("RototoError", py.get_type::<RototoError>())?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<PyPackage>()?;
    m.add_class::<PyRefreshingPackage>()?;
    m.add_class::<PyRefreshEvents>()?;
    Ok(())
}
