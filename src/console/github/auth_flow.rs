use serde::Deserialize;
use serde_json::json;

use crate::error::{Result, RototoError};

use super::GITHUB_USER_AGENT;

/// GitHub OAuth web-flow code exchange.
pub async fn exchange_github_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<String> {
    let started = std::time::Instant::now();
    tracing::debug!(
        operation = "github.oauth.exchange",
        method = "POST",
        url = "https://github.com/login/oauth/access_token",
        "console outbound GitHub OAuth exchange started"
    );
    /// GitHub OAuth code-exchange response body.
    ///
    /// The function extracts the access token or error message immediately and
    /// does not expose this raw OAuth shape outside the helper.
    #[derive(Deserialize)]
    struct Exchange {
        access_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let response = reqwest::Client::new()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .header("User-Agent", GITHUB_USER_AGENT)
        .json(&json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
        }))
        .send()
        .await
        .map_err(|err| {
            tracing::warn!(
                operation = "github.oauth.exchange",
                method = "POST",
                error = %err,
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub OAuth exchange failed before response"
            );
            RototoError::new(format!("GitHub OAuth exchange failed: {err}"))
        })?;
    let ok = response.status().is_success();
    let status = response.status();
    let body: Exchange = response.json().await.map_err(|err| {
        tracing::warn!(
            operation = "github.oauth.exchange",
            method = "POST",
            status = status.as_u16(),
            error = %err,
            latency_ms = started.elapsed().as_millis(),
            "console outbound GitHub OAuth exchange response decode failed"
        );
        RototoError::new(format!("GitHub OAuth exchange failed: {err}"))
    })?;
    match body.access_token {
        Some(token) if ok => {
            tracing::info!(
                operation = "github.oauth.exchange",
                method = "POST",
                status = status.as_u16(),
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub OAuth exchange completed"
            );
            Ok(token)
        }
        _ => {
            tracing::warn!(
                operation = "github.oauth.exchange",
                method = "POST",
                status = status.as_u16(),
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub OAuth exchange returned error"
            );
            Err(RototoError::new(
                body.error_description
                    .or(body.error)
                    .unwrap_or_else(|| "GitHub OAuth failed".to_owned()),
            ))
        }
    }
}

/// GitHub device-flow start response.
///
/// The console sends the user-facing code and polling interval to the browser,
/// but stores only `device_code` in `LocalAuth` while polling is in progress.
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
}

pub async fn start_device_flow(client_id: &str) -> Result<DeviceCode> {
    let started = std::time::Instant::now();
    tracing::debug!(
        operation = "github.device.start",
        method = "POST",
        url = "https://github.com/login/device/code",
        "console outbound GitHub device flow start call started"
    );
    /// GitHub device-code response body.
    ///
    /// The helper normalizes polling interval defaults into `DeviceCode` and
    /// drops this raw API shape before returning to auth routes.
    #[derive(Deserialize)]
    struct DeviceResponse {
        device_code: String,
        user_code: String,
        verification_uri: String,
        #[serde(default)]
        interval: u64,
        expires_in: u64,
    }
    let response = reqwest::Client::new()
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .header("User-Agent", GITHUB_USER_AGENT)
        .json(&json!({ "client_id": client_id, "scope": "read:user repo" }))
        .send()
        .await
        .map_err(|err| {
            tracing::warn!(
                operation = "github.device.start",
                method = "POST",
                error = %err,
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub device flow start call failed before response"
            );
            RototoError::new(format!("GitHub device flow start failed: {err}"))
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        tracing::warn!(
            operation = "github.device.start",
            method = "POST",
            status = status.as_u16(),
            latency_ms = started.elapsed().as_millis(),
            "console outbound GitHub device flow start call returned error status"
        );
        return Err(RototoError::new(format!(
            "GitHub device flow start failed: {status}: {text}"
        )));
    }
    let status = response.status();
    let body: DeviceResponse = response.json().await.map_err(|err| {
        tracing::warn!(
            operation = "github.device.start",
            method = "POST",
            status = status.as_u16(),
            error = %err,
            latency_ms = started.elapsed().as_millis(),
            "console outbound GitHub device flow start response decode failed"
        );
        RototoError::new(format!("GitHub device flow start failed: {err}"))
    })?;
    tracing::info!(
        operation = "github.device.start",
        method = "POST",
        status = status.as_u16(),
        latency_ms = started.elapsed().as_millis(),
        "console outbound GitHub device flow start call completed"
    );
    Ok(DeviceCode {
        device_code: body.device_code,
        user_code: body.user_code,
        verification_uri: body.verification_uri,
        interval_seconds: body.interval.max(5),
        expires_in_seconds: body.expires_in,
    })
}

/// Result of one GitHub device-flow polling request.
///
/// The local auth route maps pending states to browser polling responses,
/// persists the token on success, and clears the in-flight device flow on
/// success or terminal failure.
pub enum DevicePoll {
    Pending,
    SlowDown,
    Token(String),
    Failed(String),
}

pub async fn poll_device_flow(client_id: &str, device_code: &str) -> Result<DevicePoll> {
    let started = std::time::Instant::now();
    tracing::debug!(
        operation = "github.device.poll",
        method = "POST",
        url = "https://github.com/login/oauth/access_token",
        "console outbound GitHub device flow poll call started"
    );
    /// GitHub device-flow polling response body.
    ///
    /// The helper maps this raw API shape into `DevicePoll`, which drives the
    /// local auth lifecycle in the console route.
    #[derive(Deserialize)]
    struct PollResponse {
        access_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let response = reqwest::Client::new()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .header("User-Agent", GITHUB_USER_AGENT)
        .json(&json!({
            "client_id": client_id,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await
        .map_err(|err| {
            tracing::warn!(
                operation = "github.device.poll",
                method = "POST",
                error = %err,
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub device flow poll call failed before response"
            );
            RototoError::new(format!("GitHub device flow poll failed: {err}"))
        })?;
    let status = response.status();
    let body: PollResponse = response.json().await.map_err(|err| {
        tracing::warn!(
            operation = "github.device.poll",
            method = "POST",
            status = status.as_u16(),
            error = %err,
            latency_ms = started.elapsed().as_millis(),
            "console outbound GitHub device flow poll response decode failed"
        );
        RototoError::new(format!("GitHub device flow poll failed: {err}"))
    })?;
    if let Some(token) = body.access_token {
        tracing::info!(
            operation = "github.device.poll",
            method = "POST",
            status = status.as_u16(),
            outcome = "authorized",
            latency_ms = started.elapsed().as_millis(),
            "console outbound GitHub device flow poll completed"
        );
        return Ok(DevicePoll::Token(token));
    }
    let outcome = match body.error.as_deref() {
        Some("authorization_pending") => "pending",
        Some("slow_down") => "slow_down",
        Some(_) => "failed",
        None => "failed",
    };
    if outcome == "failed" {
        tracing::warn!(
            operation = "github.device.poll",
            method = "POST",
            status = status.as_u16(),
            outcome,
            latency_ms = started.elapsed().as_millis(),
            "console outbound GitHub device flow poll returned terminal failure"
        );
    } else {
        tracing::info!(
            operation = "github.device.poll",
            method = "POST",
            status = status.as_u16(),
            outcome,
            latency_ms = started.elapsed().as_millis(),
            "console outbound GitHub device flow poll completed"
        );
    }
    Ok(match body.error.as_deref() {
        Some("authorization_pending") => DevicePoll::Pending,
        Some("slow_down") => DevicePoll::SlowDown,
        Some(error) => DevicePoll::Failed(
            body.error_description
                .unwrap_or_else(|| format!("GitHub device flow failed: {error}")),
        ),
        None => DevicePoll::Failed("GitHub device flow failed".to_owned()),
    })
}
