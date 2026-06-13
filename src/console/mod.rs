//! The rototo console: an HTTP server that serves the embedded console UI and
//! a JSON API over the same workspace, lint, and resolution machinery the CLI
//! and SDK use. Git stays the source of truth — the console edits draft
//! branches through the GitHub API and publishes pull requests.

mod api;
mod api_draft;
mod api_workspace;
mod auth;
mod github;
mod inventory;
mod lsp;
mod observability;
mod resolve_preview;
mod stage;
mod static_assets;
mod store;
mod time;
mod token_crypto;
mod variable_toml;
mod workspace_edit;

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{Result, RototoError};

use self::api::ConsoleState;
use self::auth::{
    AuthMode, GITHUB_CLIENT_ID_ENV, GITHUB_CLIENT_SECRET_ENV, LocalAuth, resolve_ambient_token,
};
use self::github::GitHubClient;
use self::lsp::LspSessions;
use self::observability::DevObservability;
use self::stage::StageCache;
use self::store::{DiscoveredWorkspaceInput, Store};
use self::token_crypto::TokenCrypto;

pub const DEFAULT_BIND: &str = "127.0.0.1:7686";
const READ_ONLY_USER_ID: &str = "read-only";

/// Options resolved by the CLI layer; the console itself stays clap-free.
pub struct ConsoleOptions {
    pub bind: String,
    pub public_url: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub read_only: bool,
    pub workspace: Option<String>,
    pub workspace_token: Option<String>,
}

pub async fn run(options: ConsoleOptions) -> Result<()> {
    let data_dir = match options.data_dir.clone() {
        Some(dir) => dir,
        None => default_data_dir()?,
    };
    tokio::fs::create_dir_all(&data_dir).await.map_err(|err| {
        RototoError::new(format!(
            "failed to create console data directory {}: {err}",
            data_dir.display()
        ))
    })?;

    let mode = resolve_mode(&options)?;
    let observability = DevObservability::from_env().await?;
    let crypto = resolve_token_crypto(&mode, &data_dir).await?;
    let store = Store::open(&data_dir.join("console.db"), crypto)?;

    let local = match mode {
        AuthMode::Local => {
            let ambient =
                resolve_ambient_token(options.workspace_token.as_deref(), &data_dir).await;
            Some(LocalAuth::new(ambient, &data_dir))
        }
        _ => None,
    };

    let listener = tokio::net::TcpListener::bind(&options.bind)
        .await
        .map_err(|err| {
            RototoError::new(format!("failed to bind console to {}: {err}", options.bind))
        })?;
    let bound = listener
        .local_addr()
        .map_err(|err| RototoError::new(format!("failed to read console bind address: {err}")))?;
    let public_url = options
        .public_url
        .map(|url| url.trim_end_matches('/').to_owned())
        .unwrap_or_else(|| format!("http://{bound}"));
    let secure_cookies = public_url.starts_with("https://");
    let allowed_origins = allowed_origins(&public_url, bound.port());

    let state = Arc::new(ConsoleState {
        mode: mode.clone(),
        store,
        github: GitHubClient::new(),
        stage: StageCache::new(),
        lsp: LspSessions::new(),
        local,
        public_url: public_url.clone(),
        allowed_origins,
        secure_cookies,
        read_only_user_id: READ_ONLY_USER_ID.to_owned(),
        observability,
    });

    if mode == AuthMode::ReadOnly {
        let source = options
            .workspace
            .as_deref()
            .expect("read-only mode validated a workspace source");
        register_read_only_workspace(&state, source).await?;
    }

    println!(
        "rototo console ({}) listening on {public_url}",
        mode.label()
    );
    match &mode {
        AuthMode::Local => {
            let has_token = state
                .local
                .as_ref()
                .expect("local mode has local auth")
                .token()
                .await
                .is_some();
            if !has_token {
                println!(
                    "no GitHub token found; set ROTOTO_WORKSPACE_TOKEN, sign in with `gh auth login`, or use the device-flow sign-in in the UI"
                );
            }
        }
        AuthMode::Team { .. } => {
            println!("team mode: users sign in with GitHub OAuth at {public_url}/login");
        }
        AuthMode::ReadOnly => {
            println!("read-only mode: write routes are disabled");
        }
    }
    if let Some(observability) = &state.observability {
        println!("dev observability: {}", observability.dir().display());
    }

    let app = api::router(state).fallback(static_assets::serve_spa);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|err| RototoError::new(format!("console server failed: {err}")))?;
    Ok(())
}

fn resolve_mode(options: &ConsoleOptions) -> Result<AuthMode> {
    let client_id = std::env::var(GITHUB_CLIENT_ID_ENV).unwrap_or_default();
    let client_secret = std::env::var(GITHUB_CLIENT_SECRET_ENV).unwrap_or_default();
    resolve_mode_from_env(options, &client_id, &client_secret)
}

fn resolve_mode_from_env(
    options: &ConsoleOptions,
    client_id: &str,
    client_secret: &str,
) -> Result<AuthMode> {
    if options.read_only {
        if options.workspace.as_deref().unwrap_or("").trim().is_empty() {
            return Err(RototoError::new(
                "read-only mode requires --workspace <source>",
            ));
        }
        return Ok(AuthMode::ReadOnly);
    }
    match (client_id.trim(), client_secret.trim()) {
        ("", "") => Ok(AuthMode::Local),
        (_, "") => Ok(AuthMode::Local),
        ("", _) => Err(RototoError::new(format!(
            "team mode needs both {GITHUB_CLIENT_ID_ENV} and {GITHUB_CLIENT_SECRET_ENV}; set {GITHUB_CLIENT_ID_ENV} alone only for local device-flow sign-in"
        ))),
        (client_id, client_secret) => Ok(AuthMode::Team {
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
        }),
    }
}

async fn resolve_token_crypto(mode: &AuthMode, data_dir: &std::path::Path) -> Result<TokenCrypto> {
    if let Ok(raw) = std::env::var(token_crypto::KEY_ENV) {
        return TokenCrypto::from_env_value(&raw);
    }
    if matches!(mode, AuthMode::Team { .. }) {
        return Err(RototoError::new(format!(
            "{} is required for team mode so stored GitHub tokens survive restarts",
            token_crypto::KEY_ENV
        )));
    }
    // Local and read-only consoles get a generated key persisted next to the
    // database; the database only holds tokens in team mode, but the store
    // always needs a key to run.
    let key_path = data_dir.join("token.key");
    if let Ok(existing) = tokio::fs::read_to_string(&key_path).await
        && let Ok(crypto) = TokenCrypto::from_env_value(existing.trim())
    {
        return Ok(crypto);
    }
    let crypto = TokenCrypto::generate()?;
    tokio::fs::write(&key_path, crypto.key_base64())
        .await
        .map_err(|err| {
            RototoError::new(format!("failed to write {}: {err}", key_path.display()))
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).await;
    }
    Ok(crypto)
}

fn default_data_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("ROTOTO_CONSOLE_DATA_DIR")
        && !dir.trim().is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    #[cfg(unix)]
    {
        let base = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join(".local/share"))
            });
        if let Some(base) = base {
            return Ok(base.join("rototo/console"));
        }
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Ok(PathBuf::from(appdata).join("rototo/console"));
        }
    }
    Ok(PathBuf::from(".rototo-console"))
}

fn allowed_origins(public_url: &str, port: u16) -> Vec<String> {
    let mut origins = vec![
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
    ];
    let public_origin = public_url.trim_end_matches('/').to_owned();
    if !origins.contains(&public_origin) {
        origins.push(public_origin);
    }
    origins
}

/// Read-only deployments serve one configured workspace source. Register it
/// under the synthetic read-only user so every store-scoped query works.
async fn register_read_only_workspace(state: &ConsoleState, source: &str) -> Result<()> {
    let (owner, name, git_ref, path) = synthetic_registration(source);
    state
        .store
        .upsert_repo_with_workspaces(
            READ_ONLY_USER_ID.to_owned(),
            owner,
            name,
            git_ref.clone(),
            vec![DiscoveredWorkspaceInput {
                path,
                git_ref,
                source: source.to_owned(),
            }],
        )
        .await?;
    Ok(())
}

/// Best-effort owner/name/ref/path display fields for an arbitrary workspace
/// source. Staging always uses the source string itself, so these only feed
/// labels and repo-path prefixes.
fn synthetic_registration(source: &str) -> (String, String, String, String) {
    let (base, fragment) = match source.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (source, None),
    };
    let path = fragment
        .and_then(|fragment| fragment.split_once(':').map(|(_, path)| path))
        .filter(|path| !path.is_empty())
        .unwrap_or(".")
        .to_owned();
    let ref_from_fragment = fragment
        .map(|fragment| {
            fragment
                .split_once(':')
                .map(|(git_ref, _)| git_ref)
                .unwrap_or(fragment)
        })
        .filter(|git_ref| !git_ref.is_empty());

    // GitHub archive: https://api.github.com/repos/{owner}/{name}/tarball/{ref}
    if let Some(rest) = base.strip_prefix("https://api.github.com/repos/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 4 && (parts[2] == "tarball" || parts[2] == "zipball") {
            return (
                parts[0].to_owned(),
                parts[1].to_owned(),
                parts[3].to_owned(),
                path,
            );
        }
    }
    // Git URL: git+https://github.com/{owner}/{name}.git
    if let Some(at) = base.find("://")
        && base.starts_with("git+")
    {
        let rest = &base[at + 3..];
        let mut segments = rest.split('/').skip(1);
        if let (Some(owner), Some(name)) = (segments.next(), segments.next()) {
            let name = name.strip_suffix(".git").unwrap_or(name);
            return (
                owner.to_owned(),
                name.to_owned(),
                ref_from_fragment.unwrap_or("main").to_owned(),
                path,
            );
        }
    }
    // Local paths and anything else.
    let name = base
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace");
    (
        "demo".to_owned(),
        name.to_owned(),
        ref_from_fragment.unwrap_or("main").to_owned(),
        path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> ConsoleOptions {
        ConsoleOptions {
            bind: DEFAULT_BIND.to_owned(),
            public_url: None,
            data_dir: None,
            read_only: false,
            workspace: None,
            workspace_token: None,
        }
    }

    #[test]
    fn resolve_mode_stays_local_with_only_github_client_id() {
        let mode =
            resolve_mode_from_env(&options(), "device-client-id", "").expect("mode should resolve");

        assert_eq!(mode, AuthMode::Local);
    }

    #[test]
    fn resolve_mode_uses_team_when_namespaced_github_oauth_pair_is_set() {
        let mode = resolve_mode_from_env(&options(), "oauth-client-id", "oauth-secret")
            .expect("mode should resolve");

        assert_eq!(
            mode,
            AuthMode::Team {
                client_id: "oauth-client-id".to_owned(),
                client_secret: "oauth-secret".to_owned(),
            }
        );
    }

    #[test]
    fn resolve_mode_rejects_github_client_secret_without_client_id() {
        let err = resolve_mode_from_env(&options(), "", "oauth-secret")
            .expect_err("mode should reject a secret without a client id");

        assert!(err.to_string().contains(GITHUB_CLIENT_ID_ENV));
        assert!(err.to_string().contains(GITHUB_CLIENT_SECRET_ENV));
    }

    #[test]
    fn synthetic_registration_parses_source_forms() {
        assert_eq!(
            synthetic_registration("https://api.github.com/repos/octo/configs/tarball/main"),
            (
                "octo".to_owned(),
                "configs".to_owned(),
                "main".to_owned(),
                ".".to_owned()
            )
        );
        assert_eq!(
            synthetic_registration(
                "https://api.github.com/repos/octo/configs/tarball/v2#:payments/flags"
            ),
            (
                "octo".to_owned(),
                "configs".to_owned(),
                "v2".to_owned(),
                "payments/flags".to_owned()
            )
        );
        assert_eq!(
            synthetic_registration("git+https://github.com/octo/configs.git#release:apps"),
            (
                "octo".to_owned(),
                "configs".to_owned(),
                "release".to_owned(),
                "apps".to_owned()
            )
        );
        assert_eq!(
            synthetic_registration("examples/basic"),
            (
                "demo".to_owned(),
                "basic".to_owned(),
                "main".to_owned(),
                ".".to_owned()
            )
        );
    }
}
