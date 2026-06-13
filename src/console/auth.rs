use std::path::PathBuf;

use axum::http::HeaderMap;
use tokio::sync::{Mutex, RwLock};

use crate::error::{Result, RototoError};

use super::github::{self, GitHubClient};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// Single user on a trusted machine: no login, ambient GitHub token.
    Local,
    /// Shared deployment: GitHub OAuth web flow, per-user encrypted tokens.
    Team {
        client_id: String,
        client_secret: String,
    },
    /// Demo deployment: no auth, fixed workspace source, mutations rejected.
    ReadOnly,
}

impl AuthMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Team { .. } => "team",
            Self::ReadOnly => "read-only",
        }
    }
}

/// Where the local-mode GitHub token came from, for the /api/me explanation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenSource {
    Flag,
    Environment,
    DeviceFlow,
    GhCli,
}

#[derive(Clone)]
pub struct AmbientToken {
    pub token: String,
    pub source: TokenSource,
}

pub struct DeviceFlowState {
    pub device_code: String,
}

/// Local-mode authentication state: the ambient token plus the GitHub
/// identity it maps to, fetched once per token.
pub struct LocalAuth {
    token: RwLock<Option<AmbientToken>>,
    identity: RwLock<Option<(String, SessionUser)>>,
    pub device_flow: Mutex<Option<DeviceFlowState>>,
    credentials_path: PathBuf,
    device_client_id: Option<String>,
}

impl LocalAuth {
    pub fn new(initial: Option<AmbientToken>, data_dir: &std::path::Path) -> Self {
        Self {
            token: RwLock::new(initial),
            identity: RwLock::new(None),
            device_flow: Mutex::new(None),
            credentials_path: data_dir.join("credentials.json"),
            device_client_id: device_client_id(),
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
        let credentials = serde_json::json!({ "github_token": token });
        write_private_file(&self.credentials_path, &credentials.to_string()).await?;
        *self.token.write().await = Some(AmbientToken {
            token,
            source: TokenSource::DeviceFlow,
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
            github_user_id: viewer.id.to_string(),
            github_login: viewer.login,
            github_name: viewer.name,
            github_avatar_url: viewer.avatar_url,
            github_token: ambient.token.clone(),
        };
        *self.identity.write().await = Some((ambient.token, user.clone()));
        Ok(Some(user))
    }
}

fn device_client_id() -> Option<String> {
    let from_env = std::env::var(GITHUB_CLIENT_ID_ENV).ok();
    let id = from_env.unwrap_or_else(|| BAKED_DEVICE_CLIENT_ID.to_owned());
    let id = id.trim().to_owned();
    (!id.is_empty()).then_some(id)
}

/// Resolves the local-mode ambient token: explicit flag/env first, then a
/// token stored by a previous device-flow sign-in, then the GitHub CLI.
pub async fn resolve_ambient_token(
    flag_token: Option<&str>,
    data_dir: &std::path::Path,
) -> Option<AmbientToken> {
    if let Some(token) = flag_token {
        let token = token.trim();
        if !token.is_empty() {
            // clap fills the flag from ROTOTO_WORKSPACE_TOKEN too; report the
            // narrower source only when the flag came from the environment.
            let source = if std::env::args().any(|arg| arg == "--workspace-token") {
                TokenSource::Flag
            } else {
                TokenSource::Environment
            };
            return Some(AmbientToken {
                token: token.to_owned(),
                source,
            });
        }
    }

    let credentials_path = data_dir.join("credentials.json");
    if let Ok(contents) = tokio::fs::read_to_string(&credentials_path).await
        && let Ok(credentials) = serde_json::from_str::<serde_json::Value>(&contents)
        && let Some(token) = credentials
            .get("github_token")
            .and_then(serde_json::Value::as_str)
        && !token.trim().is_empty()
    {
        return Some(AmbientToken {
            token: token.trim().to_owned(),
            source: TokenSource::DeviceFlow,
        });
    }

    if let Ok(output) = tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
        && output.status.success()
    {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !token.is_empty() {
            return Some(AmbientToken {
                token,
                source: TokenSource::GhCli,
            });
        }
    }

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
