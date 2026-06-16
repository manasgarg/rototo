use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value as JsonValue, json};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::error::{Result, RototoError};

use super::runtime_config::{
    ConsoleObservabilityConfig, ConsoleRequestObservabilityConfig, resolve_observability_dir,
};

/// Development-only NDJSON event sink for console API and UI activity.
///
/// The sink is enabled by the console runtime workspace, lives in `ConsoleState`,
/// and serializes append writes through one lock so concurrent requests do not
/// interleave JSON lines. Dropping it closes no external service; the files
/// remain in the configured directory.
#[derive(Clone)]
pub struct DevObservability {
    dir: Arc<PathBuf>,
    config: Arc<ConsoleObservabilityConfig>,
    write_lock: Arc<Mutex<()>>,
}

tokio::task_local! {
    static REQUEST_OBSERVABILITY: ConsoleRequestObservabilityConfig;
}

impl DevObservability {
    pub async fn from_config(
        data_dir: &Path,
        config: &ConsoleObservabilityConfig,
    ) -> Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }
        if config.event_sink.directory.trim().is_empty() {
            return Ok(None);
        }
        let dir = resolve_observability_dir(data_dir, &config.event_sink.directory);
        tokio::fs::create_dir_all(&dir).await.map_err(|err| {
            RototoError::new(format!(
                "failed to create console observability directory {}: {err}",
                dir.display()
            ))
        })?;
        for file in ["console-api.ndjson", "console-ui.ndjson"] {
            touch(&dir.join(file)).await?;
        }
        write_json_file(&dir.join("console-observability.json"), config).await?;
        Ok(Some(Self {
            dir: Arc::new(dir),
            config: Arc::new(config.clone()),
            write_lock: Arc::new(Mutex::new(())),
        }))
    }

    pub fn dir(&self) -> &Path {
        self.dir.as_ref().as_path()
    }

    pub async fn record_api_request(
        &self,
        mut event: JsonValue,
        policy: &ConsoleRequestObservabilityConfig,
    ) {
        if !self.config.event_sink.api_events
            || (!policy.record_api_event
                && !policy.records_error_status(
                    event
                        .get("status")
                        .and_then(JsonValue::as_u64)
                        .unwrap_or_default() as u16,
                ))
        {
            return;
        }
        if let Some(object) = event.as_object_mut() {
            object.insert(
                "kind".to_owned(),
                JsonValue::String("api-request".to_owned()),
            );
            object.insert(
                "request_tracing_filter".to_owned(),
                JsonValue::String(policy.tracing.filter.clone()),
            );
        }
        self.write_event("console-api.ndjson", event).await;
    }

    pub async fn record_ui_event(
        &self,
        mut event: JsonValue,
        policy: &ConsoleRequestObservabilityConfig,
    ) {
        if !self.config.event_sink.ui_events || !policy.record_ui_events {
            return;
        }
        if let Some(object) = event.as_object_mut() {
            object.insert(
                "request_tracing_filter".to_owned(),
                JsonValue::String(policy.tracing.filter.clone()),
            );
        }
        self.write_event("console-ui.ndjson", event).await;
    }

    pub async fn record_operation(
        &self,
        operation: &str,
        elapsed_ms: u128,
        ok: bool,
        extra: JsonValue,
    ) {
        if !self.config.event_sink.operation_events {
            return;
        }
        if let Some(policy) = current_request_observability()
            && !policy.record_operation_events
        {
            return;
        }
        self.write_event(
            "console-api.ndjson",
            json!({
                "kind": "operation",
                "operation": operation,
                "latency_ms": elapsed_ms,
                "ok": ok,
                "extra": extra,
            }),
        )
        .await;
    }

    async fn write_event(&self, file: &str, mut event: JsonValue) {
        add_timestamp(&mut event);
        redact_value(&mut event);
        let Ok(line) = serde_json::to_string(&event) else {
            return;
        };
        let _guard = self.write_lock.lock().await;
        let path = self.dir.join(file);
        let Ok(mut file) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
        else {
            return;
        };
        let _ = file.write_all(line.as_bytes()).await;
        let _ = file.write_all(b"\n").await;
    }
}

pub async fn scope_request_observability<F, T>(
    policy: ConsoleRequestObservabilityConfig,
    future: F,
) -> T
where
    F: Future<Output = T>,
{
    REQUEST_OBSERVABILITY.scope(policy, future).await
}

pub fn current_request_observability() -> Option<ConsoleRequestObservabilityConfig> {
    REQUEST_OBSERVABILITY.try_with(Clone::clone).ok()
}

async fn touch(path: &Path) -> Result<()> {
    tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map(|_| ())
        .map_err(|err| {
            RototoError::new(format!(
                "failed to create console observability file {}: {err}",
                path.display()
            ))
        })
}

async fn write_json_file(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let contents = serde_json::to_string_pretty(value)
        .map(|contents| format!("{contents}\n"))
        .map_err(|err| RototoError::new(format!("failed to encode {}: {err}", path.display())))?;
    tokio::fs::write(path, contents).await.map_err(|err| {
        RototoError::new(format!(
            "failed to write console observability config {}: {err}",
            path.display()
        ))
    })
}

fn add_timestamp(event: &mut JsonValue) {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    match event {
        JsonValue::Object(object) => {
            object.insert("ts_ms".to_owned(), json!(millis));
        }
        other => {
            let mut object = Map::new();
            object.insert("ts_ms".to_owned(), json!(millis));
            object.insert("value".to_owned(), other.take());
            *other = JsonValue::Object(object);
        }
    }
}

fn redact_value(value: &mut JsonValue) {
    match value {
        JsonValue::Object(object) => {
            for (key, value) in object {
                if sensitive_key(key) {
                    *value = JsonValue::String("[REDACTED]".to_owned());
                } else {
                    redact_value(value);
                }
            }
        }
        JsonValue::Array(values) => {
            for value in values {
                redact_value(value);
            }
        }
        JsonValue::String(text) if sensitive_string(text) => {
            *text = "[REDACTED]".to_owned();
        }
        _ => {}
    }
}

fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "code"
        || key.contains("authorization")
        || key.contains("cookie")
        || key.contains("oauth")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("device_code")
        || key.contains("devicecode")
        || key.contains("workspace_token")
}

fn sensitive_string(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("bearer ")
        || value.contains("authorization:")
        || value.contains("rototo_console_session=")
        || value.contains("ghp_")
        || value.contains("github_pat_")
        || value.contains("rototo_workspace_token")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_keys_recursively() {
        let mut value = json!({
            "token": "ghp_secret",
            "headers": {
                "authorization": "Bearer secret",
                "cookie": "rototo_console_session=secret"
            },
            "nested": [{ "workspaceToken": "secret" }],
            "status": 200
        });

        redact_value(&mut value);

        assert_eq!(value["token"], "[REDACTED]");
        assert_eq!(value["headers"]["authorization"], "[REDACTED]");
        assert_eq!(value["headers"]["cookie"], "[REDACTED]");
        assert_eq!(value["nested"][0]["workspaceToken"], "[REDACTED]");
        assert_eq!(value["status"], 200);
    }

    #[test]
    fn redacts_sensitive_strings_even_when_key_is_safe() {
        let mut value = json!({
            "message": "request failed with Authorization: Bearer secret",
            "safe": "workspace loaded"
        });

        redact_value(&mut value);

        assert_eq!(value["message"], "[REDACTED]");
        assert_eq!(value["safe"], "workspace loaded");
    }
}
