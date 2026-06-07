use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyModule};
use pyo3_async_runtimes::tokio::future_into_py;
use pythonize::{depythonize, pythonize};
use rototo::{
    LintMode, LoadOptions, RefreshOptions, ResolveContext, ResolveOptions, SourceAuth,
    SourceFingerprint, SourceOptions,
};
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;

pyo3::create_exception!(_rototo, RototoError, pyo3::exceptions::PyException);

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pyclass(name = "_Workspace")]
struct PyWorkspace {
    inner: Arc<rototo::Workspace>,
}

#[pymethods]
impl PyWorkspace {
    #[staticmethod]
    #[pyo3(signature = (source, *, workspace_token = None, lint = "deny"))]
    fn load<'py>(
        py: Python<'py>,
        source: String,
        workspace_token: Option<String>,
        lint: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let options = load_options(workspace_token, lint)?;
        future_into_py(py, async move {
            let workspace = rototo::Workspace::load_with_options(source, options)
                .await
                .map_err(py_err)?;
            Python::attach(|py| {
                Py::new(
                    py,
                    PyWorkspace {
                        inner: Arc::new(workspace),
                    },
                )
            })
        })
    }

    #[staticmethod]
    #[pyo3(signature = (source, *, workspace_token = None))]
    fn inspect<'py>(
        py: Python<'py>,
        source: String,
        workspace_token: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let options = source_options(workspace_token);
        future_into_py(py, async move {
            let workspace = rototo::Workspace::inspect_with_source_options(source, &options)
                .await
                .map_err(py_err)?;
            Python::attach(|py| {
                Py::new(
                    py,
                    PyWorkspace {
                        inner: Arc::new(workspace),
                    },
                )
            })
        })
    }

    fn root(&self) -> String {
        self.inner.root().display().to_string()
    }

    fn lint<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let workspace = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let lint = workspace.lint().await.map_err(py_err)?;
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
    ) -> PyResult<Bound<'py, PyAny>> {
        let context = json_from_py(&context)?;
        let workspace = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let context = ResolveContext::from_json(context).map_err(py_err)?;
            let resolution = workspace
                .resolve_variable_with_options(&id, &context, resolve_options(validate_context))
                .await
                .map_err(py_err)?;
            Python::attach(|py| variable_resolution_to_py(py, resolution))
        })
    }

    #[pyo3(signature = (id, context, *, validate_context = true))]
    fn resolve_qualifier<'py>(
        &self,
        py: Python<'py>,
        id: String,
        context: Bound<'py, PyAny>,
        validate_context: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let context = json_from_py(&context)?;
        let workspace = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let context = ResolveContext::from_json(context).map_err(py_err)?;
            let resolution = workspace
                .resolve_qualifier_with_options(&id, &context, resolve_options(validate_context))
                .await
                .map_err(py_err)?;
            Python::attach(|py| qualifier_resolution_to_py(py, resolution))
        })
    }
}

#[pyclass(name = "_RefreshingWorkspace")]
struct PyRefreshingWorkspace {
    inner: Arc<Mutex<Option<rototo::RefreshingWorkspace>>>,
}

#[pymethods]
impl PyRefreshingWorkspace {
    #[staticmethod]
    #[pyo3(signature = (source, *, period_seconds = None, workspace_token = None, lint = "deny"))]
    fn load<'py>(
        py: Python<'py>,
        source: String,
        period_seconds: Option<f64>,
        workspace_token: Option<String>,
        lint: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let load_options = load_options(workspace_token, lint)?;
        let refresh_options = refresh_options(period_seconds)?;
        future_into_py(py, async move {
            let workspace = rototo::RefreshingWorkspace::load_with_options(
                source,
                load_options,
                refresh_options,
            )
            .await
            .map_err(py_err)?;
            Python::attach(|py| {
                Py::new(
                    py,
                    PyRefreshingWorkspace {
                        inner: Arc::new(Mutex::new(Some(workspace))),
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
    ) -> PyResult<Bound<'py, PyAny>> {
        let context = json_from_py(&context)?;
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let context = ResolveContext::from_json(context).map_err(py_err)?;
            let guard = inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            let resolution = workspace
                .resolve_variable_with_options(&id, &context, resolve_options(validate_context))
                .await
                .map_err(py_err)?;
            Python::attach(|py| variable_resolution_to_py(py, resolution))
        })
    }

    #[pyo3(signature = (id, context, *, validate_context = true))]
    fn resolve_qualifier<'py>(
        &self,
        py: Python<'py>,
        id: String,
        context: Bound<'py, PyAny>,
        validate_context: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let context = json_from_py(&context)?;
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let context = ResolveContext::from_json(context).map_err(py_err)?;
            let guard = inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            let resolution = workspace
                .resolve_qualifier_with_options(&id, &context, resolve_options(validate_context))
                .await
                .map_err(py_err)?;
            Python::attach(|py| qualifier_resolution_to_py(py, resolution))
        })
    }

    fn refresh_now<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let guard = inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            let outcome = workspace.refresh_now().await.map_err(py_err)?;
            Ok(refresh_outcome_name(outcome).to_owned())
        })
    }

    fn status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let guard = inner.lock().await;
            let workspace = active_refreshing_workspace(&guard)?;
            let status = workspace.status().await;
            Python::attach(|py| refresh_status_to_py(py, status))
        })
    }

    fn shutdown<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let workspace = {
                let mut guard = inner.lock().await;
                guard.take()
            };
            if let Some(workspace) = workspace {
                workspace.shutdown().await;
            }
            Ok(())
        })
    }
}

fn active_refreshing_workspace(
    guard: &Option<rototo::RefreshingWorkspace>,
) -> PyResult<&rototo::RefreshingWorkspace> {
    guard
        .as_ref()
        .ok_or_else(|| RototoError::new_err("refreshing workspace has been shut down"))
}

fn source_options(workspace_token: Option<String>) -> SourceOptions {
    match workspace_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
        None => SourceOptions::new(),
    }
}

fn load_options(workspace_token: Option<String>, lint: &str) -> PyResult<LoadOptions> {
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
        .with_source_auth(match workspace_token {
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
    ResolveOptions { validate_context }
}

fn json_from_py(value: &Bound<'_, PyAny>) -> PyResult<JsonValue> {
    depythonize(value).map_err(|err| {
        RototoError::new_err(format!("failed to convert Python value to JSON: {err}"))
    })
}

fn py_err(err: rototo::RototoError) -> PyErr {
    RototoError::new_err(err.to_string())
}

fn lint_to_py(py: Python<'_>, lint: rototo::model::WorkspaceLint) -> PyResult<Py<PyAny>> {
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
    dict.set_item("value_key", resolution.value_key)?;
    dict.set_item("value", pythonize(py, &resolution.value)?)?;
    Ok(dict.into_any().unbind())
}

fn qualifier_resolution_to_py(
    py: Python<'_>,
    resolution: rototo::model::QualifierResolution,
) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("id", resolution.id)?;
    dict.set_item("value", resolution.value)?;
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

#[pymodule]
fn _rototo(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("RototoError", py.get_type::<RototoError>())?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<PyWorkspace>()?;
    m.add_class::<PyRefreshingWorkspace>()?;
    Ok(())
}
