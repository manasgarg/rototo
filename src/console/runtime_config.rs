use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tracing_subscriber::EnvFilter;

use crate::error::{Result, RototoError};
use crate::{Package, ResolveContext};

use super::capabilities::{DeploymentType, WritePolicy};

const STARTUP_OBSERVABILITY_VARIABLE: &str = "console-observability";
const REQUEST_OBSERVABILITY_VARIABLE: &str = "console-request-observability";
const DEV_HOST: &str = "dev.rototo.dev";

#[derive(Clone)]
pub(crate) struct ConsoleRuntimeConfig {
    package: Option<Arc<Package>>,
    base: ConsoleRuntimeBase,
    startup_observability: ConsoleObservabilityConfig,
}

impl ConsoleRuntimeConfig {
    pub(crate) async fn load(base: ConsoleRuntimeBase) -> Result<Self> {
        Self::load_from_path(default_runtime_package_path(), base).await
    }

    pub(crate) async fn load_from_path(
        path: Option<PathBuf>,
        base: ConsoleRuntimeBase,
    ) -> Result<Self> {
        let Some(path) = path else {
            tracing::warn!(
                operation = "console.runtime_config.load",
                "console runtime package path could not be resolved; using built-in defaults"
            );
            return Ok(Self::built_in(base));
        };
        if !tokio::fs::try_exists(path.join("rototo-package.toml"))
            .await
            .map_err(|err| {
                RototoError::new(format!(
                    "failed to inspect console runtime package {}: {err}",
                    path.display()
                ))
            })?
        {
            tracing::info!(
                operation = "console.runtime_config.load",
                path = %path.display(),
                "console runtime package not found; using built-in defaults"
            );
            return Ok(Self::built_in(base));
        }

        let package = Arc::new(Package::load(path.to_string_lossy()).await.map_err(|err| {
            RototoError::new(format!(
                "failed to load console runtime package {}: {err}",
                path.display()
            ))
        })?);
        let startup_observability =
            resolve_startup_observability(&package, &base.startup_context()).await?;
        tracing::info!(
            operation = "console.runtime_config.load",
            path = %path.display(),
            tracing_filter = %startup_observability.tracing.filter,
            observability_enabled = startup_observability.enabled,
            "console runtime package loaded"
        );
        Ok(Self {
            package: Some(package),
            base,
            startup_observability,
        })
    }

    pub(crate) fn built_in(base: ConsoleRuntimeBase) -> Self {
        let startup_observability = if base.is_dev_host() {
            ConsoleObservabilityConfig::dev()
        } else {
            ConsoleObservabilityConfig::standard()
        };
        Self {
            package: None,
            base,
            startup_observability,
        }
    }

    pub(crate) fn startup_observability(&self) -> &ConsoleObservabilityConfig {
        &self.startup_observability
    }

    pub(crate) fn default_request_observability(&self) -> ConsoleRequestObservabilityConfig {
        if self.base.is_dev_host() {
            ConsoleRequestObservabilityConfig::dev_all()
        } else {
            ConsoleRequestObservabilityConfig::standard()
        }
    }

    pub(crate) async fn resolve_request_observability(
        &self,
        request: RequestObservabilityContext,
    ) -> ConsoleRequestObservabilityConfig {
        let context = self.base.request_context(&request);
        let resolved = match &self.package {
            Some(package) => resolve_request_observability(package, &context).await,
            None => Ok(self.resolve_built_in_request_observability(&request)),
        };
        match resolved {
            Ok(config) => config,
            Err(err) => {
                tracing::warn!(
                    operation = "console.runtime_config.resolve_request",
                    error = %err,
                    "console request observability policy failed to resolve; using default policy"
                );
                self.default_request_observability()
            }
        }
    }

    fn resolve_built_in_request_observability(
        &self,
        request: &RequestObservabilityContext,
    ) -> ConsoleRequestObservabilityConfig {
        if self.base.is_dev_host() {
            return ConsoleRequestObservabilityConfig::dev_all();
        }
        if request.status_class == "server_error" {
            return ConsoleRequestObservabilityConfig::retain_errors();
        }
        if request.latency_ms > self.startup_observability.thresholds.api_p95_ms as u128 {
            return ConsoleRequestObservabilityConfig::retain_slow();
        }
        ConsoleRequestObservabilityConfig::standard()
    }
}

async fn resolve_startup_observability(
    package: &Package,
    context: &JsonValue,
) -> Result<ConsoleObservabilityConfig> {
    let context = ResolveContext::from_json(context.clone())?;
    let resolution = package.resolve_variable(STARTUP_OBSERVABILITY_VARIABLE, &context)?;
    let config: ConsoleObservabilityConfig = serde_json::from_value(resolution.value).map_err(|err| {
        RototoError::new(format!(
            "console runtime variable {STARTUP_OBSERVABILITY_VARIABLE} resolved invalid value: {err}"
        ))
    })?;
    config.validate(STARTUP_OBSERVABILITY_VARIABLE)?;
    Ok(config)
}

async fn resolve_request_observability(
    package: &Package,
    context: &JsonValue,
) -> Result<ConsoleRequestObservabilityConfig> {
    let context = ResolveContext::from_json(context.clone())?;
    let resolution = package.resolve_variable(REQUEST_OBSERVABILITY_VARIABLE, &context)?;
    let config: ConsoleRequestObservabilityConfig =
        serde_json::from_value(resolution.value).map_err(|err| {
        RototoError::new(format!(
            "console runtime variable {REQUEST_OBSERVABILITY_VARIABLE} resolved invalid value: {err}"
        ))
        })?;
    config.validate(REQUEST_OBSERVABILITY_VARIABLE)?;
    Ok(config)
}

#[derive(Clone, Debug)]
pub(crate) struct ConsoleRuntimeBase {
    pub(crate) deployment: DeploymentType,
    pub(crate) write_policy: WritePolicy,
    pub(crate) console_host: Option<String>,
    pub(crate) fixed_package: bool,
    pub(crate) secure_cookies: bool,
}

impl ConsoleRuntimeBase {
    fn is_dev_host(&self) -> bool {
        self.console_host
            .as_deref()
            .is_some_and(|host| host.eq_ignore_ascii_case(DEV_HOST))
    }

    fn startup_context(&self) -> JsonValue {
        json!({
            "phase": "startup",
            "deployment": self.deployment.label(),
            "write_policy": self.write_policy.label(),
            "console": {
                "host": self.console_host,
                "fixed_package": self.fixed_package,
                "secure_cookies": self.secure_cookies,
            },
            "request": {
                "present": false,
                "host": null,
                "method": "none",
                "route": "none",
                "repo": null,
                "package": null,
                "branch": null,
                "mutating": false,
            },
            "response": {
                "present": false,
                "status": 0,
                "status_class": "none",
                "latency_ms": 0,
            },
        })
    }

    fn request_context(&self, request: &RequestObservabilityContext) -> JsonValue {
        json!({
            "phase": "request",
            "deployment": self.deployment.label(),
            "write_policy": self.write_policy.label(),
            "console": {
                "host": self.console_host,
                "fixed_package": self.fixed_package,
                "secure_cookies": self.secure_cookies,
            },
            "request": {
                "present": true,
                "host": request.host,
                "method": request.method,
                "route": request.route,
                "repo": request.repo,
                "package": request.package,
                "branch": request.branch,
                "mutating": request.mutating,
            },
            "response": {
                "present": request.response_present,
                "status": request.status,
                "status_class": request.status_class,
                "latency_ms": request.latency_ms,
            },
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RequestObservabilityContext {
    pub(crate) host: Option<String>,
    pub(crate) method: String,
    pub(crate) route: String,
    pub(crate) repo: Option<String>,
    pub(crate) package: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) mutating: bool,
    pub(crate) response_present: bool,
    pub(crate) status: u16,
    pub(crate) status_class: &'static str,
    pub(crate) latency_ms: u128,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConsoleObservabilityConfig {
    pub(crate) enabled: bool,
    pub(crate) event_sink: EventSinkConfig,
    pub(crate) tracing: TracingConfig,
    pub(crate) thresholds: ConsoleObservabilityThresholds,
}

impl ConsoleObservabilityConfig {
    fn standard() -> Self {
        Self {
            enabled: true,
            event_sink: EventSinkConfig::all("observability"),
            tracing: TracingConfig {
                filter: "warn".to_owned(),
                format: "compact".to_owned(),
            },
            thresholds: ConsoleObservabilityThresholds::standard(),
        }
    }

    fn dev() -> Self {
        Self {
            enabled: true,
            event_sink: EventSinkConfig::all("observability"),
            tracing: TracingConfig {
                filter: "rototo=trace,warn".to_owned(),
                format: "compact".to_owned(),
            },
            thresholds: ConsoleObservabilityThresholds {
                api_p95_ms: 1000,
                api_errors: 0,
                frontend_errors: 0,
                lsp_failures: 0,
            },
        }
    }

    fn validate(&self, variable: &str) -> Result<()> {
        validate_tracing_filter(variable, &self.tracing.filter)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventSinkConfig {
    pub(crate) directory: String,
    pub(crate) api_events: bool,
    pub(crate) ui_events: bool,
    pub(crate) operation_events: bool,
}

impl EventSinkConfig {
    fn all(directory: &str) -> Self {
        Self {
            directory: directory.to_owned(),
            api_events: true,
            ui_events: true,
            operation_events: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TracingConfig {
    pub(crate) filter: String,
    pub(crate) format: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConsoleObservabilityThresholds {
    pub(crate) api_p95_ms: u64,
    pub(crate) api_errors: u64,
    pub(crate) frontend_errors: u64,
    pub(crate) lsp_failures: u64,
}

impl ConsoleObservabilityThresholds {
    fn standard() -> Self {
        Self {
            api_p95_ms: 750,
            api_errors: 0,
            frontend_errors: 0,
            lsp_failures: 0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConsoleRequestObservabilityConfig {
    pub(crate) record_api_event: bool,
    pub(crate) record_operation_events: bool,
    pub(crate) record_ui_events: bool,
    pub(crate) record_errors: bool,
    pub(crate) sample_rate: f64,
    pub(crate) slow_request_ms: u64,
    pub(crate) tracing: RequestTracingConfig,
}

impl ConsoleRequestObservabilityConfig {
    fn standard() -> Self {
        Self {
            record_api_event: false,
            record_operation_events: false,
            record_ui_events: false,
            record_errors: true,
            sample_rate: 0.0,
            slow_request_ms: 750,
            tracing: RequestTracingConfig {
                filter: "warn".to_owned(),
            },
        }
    }

    fn retain_errors() -> Self {
        Self {
            record_api_event: true,
            record_operation_events: true,
            record_ui_events: true,
            record_errors: true,
            sample_rate: 1.0,
            slow_request_ms: 750,
            tracing: RequestTracingConfig {
                filter: "rototo=debug,warn".to_owned(),
            },
        }
    }

    fn retain_slow() -> Self {
        Self {
            record_api_event: true,
            record_operation_events: true,
            record_ui_events: true,
            record_errors: true,
            sample_rate: 1.0,
            slow_request_ms: 500,
            tracing: RequestTracingConfig {
                filter: "rototo=debug,warn".to_owned(),
            },
        }
    }

    fn dev_all() -> Self {
        Self {
            record_api_event: true,
            record_operation_events: true,
            record_ui_events: true,
            record_errors: true,
            sample_rate: 1.0,
            slow_request_ms: 1,
            tracing: RequestTracingConfig {
                filter: "rototo=trace,warn".to_owned(),
            },
        }
    }

    pub(crate) fn records_error_status(&self, status: u16) -> bool {
        self.record_errors && status >= 400
    }

    fn validate(&self, variable: &str) -> Result<()> {
        validate_tracing_filter(variable, &self.tracing.filter)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RequestTracingConfig {
    pub(crate) filter: String,
}

pub(crate) fn default_runtime_package_path() -> Option<PathBuf> {
    default_runtime_package_path_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

fn default_runtime_package_path_from(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    if let Some(dir) = xdg_config_home.filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("rototo/package"));
    }
    home.filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join(".config/rototo/package"))
}

pub(crate) fn public_url_host(public_url: &str) -> Option<String> {
    let trimmed = public_url.trim();
    let after_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let authority = after_scheme.split('/').next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map(|(host, _)| host).unwrap_or(rest)
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

pub(crate) fn resolve_observability_dir(data_dir: &Path, configured: &str) -> PathBuf {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        path
    } else {
        data_dir.join(path)
    }
}

fn validate_tracing_filter(variable: &str, filter: &str) -> Result<()> {
    EnvFilter::try_new(filter).map(|_| ()).map_err(|err| {
        RototoError::new(format!(
            "console runtime variable {variable} has invalid tracing filter {filter:?}: {err}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(host: Option<&str>, deployment: DeploymentType) -> ConsoleRuntimeBase {
        ConsoleRuntimeBase {
            deployment,
            write_policy: WritePolicy::PullRequest,
            console_host: host.map(str::to_owned),
            fixed_package: false,
            secure_cookies: host.is_some_and(|host| host != "127.0.0.1"),
        }
    }

    #[test]
    fn runtime_package_path_uses_xdg_config_home() {
        let path =
            default_runtime_package_path_from(Some("/tmp/xdg".into()), Some("/tmp/home".into()))
                .unwrap();
        assert_eq!(path, PathBuf::from("/tmp/xdg/rototo/package"));
    }

    #[test]
    fn runtime_package_path_falls_back_to_home_config() {
        let path = default_runtime_package_path_from(None, Some("/tmp/home".into())).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/home/.config/rototo/package"));
    }

    #[test]
    fn public_url_host_extracts_host_without_port_or_path() {
        assert_eq!(
            public_url_host("https://dev.rototo.dev:443/app"),
            Some("dev.rototo.dev".to_owned())
        );
        assert_eq!(
            public_url_host("http://127.0.0.1:7686"),
            Some("127.0.0.1".to_owned())
        );
    }

    #[tokio::test]
    async fn missing_runtime_package_uses_built_in_dev_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = ConsoleRuntimeConfig::load_from_path(
            Some(dir.path().join("missing")),
            base(Some(DEV_HOST), DeploymentType::Hosted),
        )
        .await
        .unwrap();
        assert_eq!(
            config.startup_observability().tracing.filter,
            "rototo=trace,warn"
        );
        assert_eq!(
            config.default_request_observability().tracing.filter,
            "rototo=trace,warn"
        );
    }

    #[tokio::test]
    async fn generated_package_resolves_dev_for_host_not_deployment() {
        let config = ConsoleRuntimeConfig::load_from_path(
            Some(PathBuf::from("examples/console-runtime")),
            base(Some(DEV_HOST), DeploymentType::Hosted),
        )
        .await
        .unwrap();
        assert_eq!(
            config.startup_observability().tracing.filter,
            "rototo=trace,warn"
        );
        let policy = config
            .resolve_request_observability(RequestObservabilityContext {
                host: Some(DEV_HOST.to_owned()),
                method: "GET".to_owned(),
                route: "/api/source-trees".to_owned(),
                repo: None,
                package: None,
                branch: None,
                mutating: false,
                response_present: true,
                status: 200,
                status_class: "success",
                latency_ms: 12,
            })
            .await;
        assert_eq!(policy.tracing.filter, "rototo=trace,warn");
    }

    #[tokio::test]
    async fn generated_package_keeps_standard_host_quiet_until_problematic() {
        let config = ConsoleRuntimeConfig::load_from_path(
            Some(PathBuf::from("examples/console-runtime")),
            base(Some("console.rototo.dev"), DeploymentType::Hosted),
        )
        .await
        .unwrap();
        assert_eq!(config.startup_observability().tracing.filter, "warn");
        let ok = config
            .resolve_request_observability(RequestObservabilityContext {
                host: Some("console.rototo.dev".to_owned()),
                method: "GET".to_owned(),
                route: "/api/source-trees".to_owned(),
                repo: None,
                package: None,
                branch: None,
                mutating: false,
                response_present: true,
                status: 200,
                status_class: "success",
                latency_ms: 12,
            })
            .await;
        assert_eq!(ok.tracing.filter, "warn");
        assert!(!ok.record_api_event);

        let problem = config
            .resolve_request_observability(RequestObservabilityContext {
                status: 500,
                status_class: "server_error",
                latency_ms: 12,
                ..RequestObservabilityContext {
                    host: Some("console.rototo.dev".to_owned()),
                    method: "GET".to_owned(),
                    route: "/api/source-trees".to_owned(),
                    repo: None,
                    package: None,
                    branch: None,
                    mutating: false,
                    response_present: true,
                    status: 200,
                    status_class: "success",
                    latency_ms: 12,
                }
            })
            .await;
        assert_eq!(problem.tracing.filter, "rototo=debug,warn");
        assert!(problem.record_api_event);
    }
}
