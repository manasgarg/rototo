use std::path::PathBuf;

use axum::http::HeaderMap;
use tokio::sync::{Mutex, RwLock};

use crate::error::{Result, RototoError};

use super::github::{self, GitHubClient};
use super::identity::ActorIdentity;
use super::store::{SessionUser, Store};

pub const GITHUB_CLIENT_ID_ENV: &str = "ROTOTO_GITHUB_CLIENT_ID";
pub const GITHUB_CLIENT_SECRET_ENV: &str = "ROTOTO_GITHUB_CLIENT_SECRET";
pub const SESSION_COOKIE: &str = "rototo_console_session";
pub const OAUTH_STATE_COOKIE: &str = "rototo_console_oauth_state";
pub const GITHUB_OAUTH_SCOPES: &str = "read:user repo";

/// Device-flow client ID baked into release builds once the rototo GitHub App
/// exists. Empty means device flow is only available when
/// ROTOTO_GITHUB_CLIENT_ID is set.
const BAKED_DEVICE_CLIENT_ID: &str = "";

/// Hosted-mode GitHub OAuth app credentials.
///
/// Startup resolves this from environment variables and stores it in
/// `ConsoleState`. It lives for the server process and is used only to build
/// authorization URLs and exchange callback codes for user tokens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedOAuth {
    pub client_id: String,
    pub client_secret: String,
}

/// Where the current GitHub token came from.
///
/// This is serialized for `/api/me` so the UI can explain why a package can
/// or cannot write. The source follows the token: it changes when local device
/// flow stores a new token or hosted OAuth creates a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitHubCredentialSource {
    Flag,
    Environment,
    DeviceFlow,
    GhCli,
    OAuthSession,
}

/// Local-mode token plus provenance.
///
/// The token may come from a flag, environment, device-flow credentials file,
/// or `gh auth token`. It is kept in memory inside `LocalAuth`; device-flow
/// tokens are also written to the console data directory for later launches.
#[derive(Clone)]
pub struct AmbientToken {
    pub token: String,
    pub source: GitHubCredentialSource,
}

/// In-progress local GitHub device-flow session.
///
/// The console stores only the device code between `/device/start` and polling
/// completion. It is replaced when a new device flow starts and cleared when
/// polling succeeds or fails.
pub struct DeviceFlowState {
    pub device_code: String,
}

/// Local-mode authentication state.
///
/// This owns the mutable ambient token, the GitHub identity fetched for that
/// exact token, and the optional in-flight device flow. It lives in
/// `ConsoleState` for the process lifetime; the stored credentials file lets a
/// successful device-flow sign-in survive restarts.
pub struct LocalAuth {
    token: RwLock<Option<AmbientToken>>,
    identity: RwLock<Option<(String, SessionUser)>>,
    pub device_flow: Mutex<Option<DeviceFlowState>>,
    credentials_path: Option<PathBuf>,
    device_client_id: Option<String>,
}

impl LocalAuth {
    pub fn new(
        initial: Option<AmbientToken>,
        data_dir: Option<&std::path::Path>,
        device_client_id: Option<String>,
    ) -> Self {
        Self {
            token: RwLock::new(initial),
            identity: RwLock::new(None),
            device_flow: Mutex::new(None),
            credentials_path: data_dir.map(|dir| dir.join("credentials.json")),
            device_client_id,
        }
    }

    pub fn device_flow_available(&self) -> bool {
        self.device_client_id.is_some()
    }

    pub fn device_client_id(&self) -> Option<&str> {
        self.device_client_id.as_deref()
    }

    pub async fn token(&self) -> Option<AmbientToken> {
        self.token.read().await.clone()
    }

    pub async fn set_device_token(&self, token: String) -> Result<()> {
        if let Some(credentials_path) = self.credentials_path.as_ref() {
            let credentials = serde_json::json!({ "github_token": token });
            write_private_file(credentials_path, &credentials.to_string()).await?;
        }
        *self.token.write().await = Some(AmbientToken {
            token,
            source: GitHubCredentialSource::DeviceFlow,
        });
        *self.identity.write().await = None;
        Ok(())
    }

    /// The GitHub identity behind the ambient token, fetched once per token.
    pub async fn identity(&self, github: &GitHubClient) -> Result<Option<SessionUser>> {
        let Some(ambient) = self.token().await else {
            return Ok(None);
        };
        {
            let identity = self.identity.read().await;
            if let Some((token, user)) = identity.as_ref()
                && *token == ambient.token
            {
                return Ok(Some(user.clone()));
            }
        }
        let viewer = github.viewer(&ambient.token).await.map_err(|err| {
            RototoError::new(format!(
                "the configured GitHub token was rejected: {}",
                github::github_error_message(&err, "Authenticating")
            ))
        })?;
        let user = SessionUser {
            session_hash: "local".to_owned(),
            principal_id: format!("github:{}", viewer.id),
            identity: ActorIdentity::GitHub {
                id: viewer.id.to_string(),
                login: viewer.login,
                name: viewer.name,
                avatar_url: viewer.avatar_url,
            },
            github_token: Some(ambient.token.clone()),
        };
        *self.identity.write().await = Some((ambient.token, user.clone()));
        Ok(Some(user))
    }
}

pub(super) fn baked_device_client_id() -> Option<String> {
    let id = BAKED_DEVICE_CLIENT_ID.to_owned();
    let id = id.trim().to_owned();
    (!id.is_empty()).then_some(id)
}

/// Resolves the local-mode ambient token: explicit flag/env first, then a
/// token stored by a previous device-flow sign-in, then the GitHub CLI.
pub async fn resolve_ambient_token(
    flag_token: Option<&str>,
    data_dir: Option<&std::path::Path>,
) -> Option<AmbientToken> {
    if let Some(token) = flag_token {
        let token = token.trim();
        if !token.is_empty() {
            // clap fills the flag from ROTOTO_PACKAGE_TOKEN too; report the
            // narrower source only when the flag came from the environment.
            let source = if std::env::args().any(|arg| arg == "--package-token") {
                GitHubCredentialSource::Flag
            } else {
                GitHubCredentialSource::Environment
            };
            tracing::info!(
                operation = "console.auth.ambient_token",
                source = ?source,
                "console local auth token resolved from explicit configuration"
            );
            return Some(AmbientToken {
                token: token.to_owned(),
                source,
            });
        }
    }

    if let Some(data_dir) = data_dir {
        let credentials_path = data_dir.join("credentials.json");
        if let Ok(contents) = tokio::fs::read_to_string(&credentials_path).await
            && let Ok(credentials) = serde_json::from_str::<serde_json::Value>(&contents)
            && let Some(token) = credentials
                .get("github_token")
                .and_then(serde_json::Value::as_str)
            && !token.trim().is_empty()
        {
            tracing::info!(
                operation = "console.auth.ambient_token",
                source = ?GitHubCredentialSource::DeviceFlow,
                credentials_path = %credentials_path.display(),
                "console local auth token resolved from stored device-flow credentials"
            );
            return Some(AmbientToken {
                token: token.trim().to_owned(),
                source: GitHubCredentialSource::DeviceFlow,
            });
        }
    } else {
        tracing::debug!(
            operation = "console.auth.ambient_token",
            "console local auth skipped stored device-flow credentials in ephemeral state mode"
        );
    }

    let started = std::time::Instant::now();
    tracing::debug!(
        operation = "process.command",
        command = "gh auth token",
        "console outbound process call started"
    );
    match tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            tracing::info!(
                operation = "process.command",
                command = "gh auth token",
                status = output.status.code(),
                token_found = !token.is_empty(),
                latency_ms = started.elapsed().as_millis(),
                "console outbound process call completed"
            );
            if !token.is_empty() {
                tracing::info!(
                    operation = "console.auth.ambient_token",
                    source = ?GitHubCredentialSource::GhCli,
                    "console local auth token resolved from GitHub CLI"
                );
                return Some(AmbientToken {
                    token,
                    source: GitHubCredentialSource::GhCli,
                });
            }
        }
        Ok(output) => {
            tracing::debug!(
                operation = "process.command",
                command = "gh auth token",
                status = output.status.code(),
                latency_ms = started.elapsed().as_millis(),
                "console outbound process call returned non-zero status"
            );
        }
        Err(err) => {
            tracing::debug!(
                operation = "process.command",
                command = "gh auth token",
                error = %err,
                latency_ms = started.elapsed().as_millis(),
                "console outbound process call failed to start"
            );
        }
    }

    tracing::info!(
        operation = "console.auth.ambient_token",
        "console local auth token was not found"
    );
    None
}

async fn write_private_file(path: &std::path::Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| {
            RototoError::new(format!("failed to create {}: {err}", parent.display()))
        })?;
    }
    tokio::fs::write(path, contents)
        .await
        .map_err(|err| RototoError::new(format!("failed to write {}: {err}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|err| {
                RototoError::new(format!(
                    "failed to set permissions on {}: {err}",
                    path.display()
                ))
            })?;
    }
    Ok(())
}

/// Reads one cookie from request headers.
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for header in headers.get_all(axum::http::header::COOKIE) {
        let Ok(value) = header.to_str() else {
            continue;
        };
        for pair in value.split(';') {
            let pair = pair.trim();
            if let Some((cookie_name, cookie_value)) = pair.split_once('=')
                && cookie_name == name
            {
                return Some(cookie_value.to_owned());
            }
        }
    }
    None
}

/// Builds a Set-Cookie value matching the admin app's cookie options:
/// httpOnly, SameSite=Lax, path=/, secure when the public URL is https.
pub fn set_cookie(name: &str, value: &str, secure: bool, max_age: Option<u64>) -> String {
    let mut cookie = format!("{name}={value}; HttpOnly; SameSite=Lax; Path=/");
    if secure {
        cookie.push_str("; Secure");
    }
    if let Some(max_age) = max_age {
        cookie.push_str(&format!("; Max-Age={max_age}"));
    }
    cookie
}

/// The team-mode session for a request, if any.
pub async fn session_from_headers(store: &Store, headers: &HeaderMap) -> Option<SessionUser> {
    let token = cookie_value(headers, SESSION_COOKIE)?;
    store.get_session(&token).await.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_parsing_picks_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "a=1; rototo_console_session=tok-en; b=2".parse().unwrap(),
        );
        assert_eq!(
            cookie_value(&headers, SESSION_COOKIE).as_deref(),
            Some("tok-en")
        );
        assert_eq!(cookie_value(&headers, "missing"), None);
    }

    #[test]
    fn set_cookie_includes_expected_attributes() {
        let cookie = set_cookie("name", "value", true, Some(600));
        assert_eq!(
            cookie,
            "name=value; HttpOnly; SameSite=Lax; Path=/; Secure; Max-Age=600"
        );
        let session = set_cookie("name", "value", false, None);
        assert!(!session.contains("Secure"));
        assert!(!session.contains("Max-Age"));
    }
}
