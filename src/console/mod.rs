//! The rototo console: an HTTP server that serves the embedded console UI and
//! a JSON API over the same workspace, lint, and resolution machinery the CLI
//! and SDK use. Git stays the source of truth: the console writes through the
//! configured GitHub API or local-git policy for the workspace source.

mod api;
mod api_branch;
mod api_workspace;
mod auth;
mod capabilities;
mod fixed_workspace;
mod github;
mod identity;
mod inventory;
mod local_git;
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
mod workspace_source;

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{Result, RototoError};

use self::api::ConsoleState;
use self::auth::{
    GITHUB_CLIENT_ID_ENV, GITHUB_CLIENT_SECRET_ENV, HostedOAuth, LocalAuth, resolve_ambient_token,
};
use self::capabilities::{DeploymentType, WritePolicy};
use self::github::GitHubClient;
use self::lsp::LspSessions;
use self::observability::DevObservability;
use self::stage::StageCache;
use self::store::Store;
use self::token_crypto::TokenCrypto;

pub const DEFAULT_BIND: &str = "127.0.0.1:7686";
pub use self::capabilities::WritePolicy as ConsoleWritePolicy;

/// Console startup options resolved by the CLI layer.
///
/// These values configure one server process: bind address, public origin,
/// data directory, optional fixed workspace, write policy, and startup token.
/// They are consumed by `run` to build `ConsoleState`; runtime source tree, workspace,
/// branch, and session lifecycles are then managed by the store and route code.
pub struct ConsoleOptions {
    pub bind: String,
    pub public_url: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub workspace: Option<String>,
    pub write_policy: WritePolicy,
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

    let (deployment, oauth) = resolve_deployment(&options)?;
    let observability = DevObservability::from_env().await?;
    let crypto = resolve_token_crypto(&deployment, &data_dir).await?;
    let store = Store::open(&data_dir.join("console.db"), crypto)?;

    let local = match deployment {
        DeploymentType::Local => {
            let ambient =
                resolve_ambient_token(options.workspace_token.as_deref(), &data_dir).await;
            Some(LocalAuth::new(ambient, &data_dir))
        }
        DeploymentType::Hosted => None,
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
        deployment: deployment.clone(),
        oauth,
        write_policy: options.write_policy,
        fixed_workspace_source: options.workspace.clone(),
        store,
        github: GitHubClient::new(),
        stage: StageCache::new(),
        lsp: LspSessions::new(),
        local,
        public_url: public_url.clone(),
        allowed_origins,
        secure_cookies,
        observability,
    });

    if deployment == DeploymentType::Local
        && let Some(source) = options.workspace.as_deref()
    {
        let actor = local_actor(&state, source).await?;
        register_fixed_workspace(&state, &actor.principal_id, source).await?;
    }

    println!(
        "rototo console ({}, write: {}) listening on {public_url}",
        deployment.label(),
        options.write_policy.label()
    );
    match &deployment {
        DeploymentType::Local => {
            let has_token = state
                .local
                .as_ref()
                .expect("local deployment has local auth")
                .token()
                .await
                .is_some();
            if !has_token {
                println!(
                    "no GitHub token found; set ROTOTO_WORKSPACE_TOKEN, sign in with `gh auth login`, or use the device-flow sign-in in the UI"
                );
            }
        }
        DeploymentType::Hosted => {
            println!("hosted deployment: users sign in with GitHub OAuth at {public_url}/login");
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

fn resolve_deployment(options: &ConsoleOptions) -> Result<(DeploymentType, Option<HostedOAuth>)> {
    let client_id = std::env::var(GITHUB_CLIENT_ID_ENV).unwrap_or_default();
    let client_secret = std::env::var(GITHUB_CLIENT_SECRET_ENV).unwrap_or_default();
    resolve_deployment_from_env(options, &client_id, &client_secret)
}

fn resolve_deployment_from_env(
    _options: &ConsoleOptions,
    client_id: &str,
    client_secret: &str,
) -> Result<(DeploymentType, Option<HostedOAuth>)> {
    match (client_id.trim(), client_secret.trim()) {
        ("", "") => Ok((DeploymentType::Local, None)),
        (_, "") => Ok((DeploymentType::Local, None)),
        ("", _) => Err(RototoError::new(format!(
            "hosted deployment needs both {GITHUB_CLIENT_ID_ENV} and {GITHUB_CLIENT_SECRET_ENV}; set {GITHUB_CLIENT_ID_ENV} alone only for local device-flow sign-in"
        ))),
        (client_id, client_secret) => Ok((
            DeploymentType::Hosted,
            Some(HostedOAuth {
                client_id: client_id.to_owned(),
                client_secret: client_secret.to_owned(),
            }),
        )),
    }
}

async fn resolve_token_crypto(
    deployment: &DeploymentType,
    data_dir: &std::path::Path,
) -> Result<TokenCrypto> {
    if let Ok(raw) = std::env::var(token_crypto::KEY_ENV) {
        return TokenCrypto::from_env_value(&raw);
    }
    if matches!(deployment, DeploymentType::Hosted) {
        return Err(RototoError::new(format!(
            "{} is required for hosted deployment so stored GitHub tokens survive restarts",
            token_crypto::KEY_ENV
        )));
    }
    // Local consoles get a generated key persisted next to the database. The
    // database only holds OAuth tokens in hosted deployment, but the store
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

async fn local_actor(state: &ConsoleState, source: &str) -> Result<store::SessionUser> {
    if let Some(local) = state.local.as_ref()
        && let Ok(Some(user)) = local.identity(&state.github).await
    {
        return Ok(user);
    }
    let local_root = local_git::workspace_root(source).ok();
    let identity = identity::resolve_git_config_identity(local_root.as_deref()).await?;
    Ok(store::SessionUser {
        session_hash: "local-git".to_owned(),
        principal_id: identity.principal_id(),
        identity,
        github_token: None,
    })
}

/// Fixed workspace deployments register the configured source tree under the
/// request actor so the existing store-scoped workspace queries still work.
pub(crate) async fn register_fixed_workspace(
    state: &ConsoleState,
    principal_id: &str,
    source: &str,
) -> Result<()> {
    let registration = fixed_workspace::registration(source).await?;
    state
        .store
        .upsert_source_tree_with_workspaces(store::RegisterSourceTreeInput {
            principal_id: principal_id.to_owned(),
            kind: registration.kind,
            source: registration.source,
            display_name: registration.display_name,
            default_revision: registration.default_revision,
            workspaces: registration.workspaces,
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> ConsoleOptions {
        ConsoleOptions {
            bind: DEFAULT_BIND.to_owned(),
            public_url: None,
            data_dir: None,
            workspace: None,
            write_policy: WritePolicy::PullRequest,
            workspace_token: None,
        }
    }

    #[test]
    fn resolve_deployment_stays_local_with_only_github_client_id() {
        let (deployment, oauth) = resolve_deployment_from_env(&options(), "device-client-id", "")
            .expect("deployment should resolve");

        assert_eq!(deployment, DeploymentType::Local);
        assert!(oauth.is_none());
    }

    #[test]
    fn resolve_deployment_uses_hosted_when_namespaced_github_oauth_pair_is_set() {
        let (deployment, oauth) =
            resolve_deployment_from_env(&options(), "oauth-client-id", "oauth-secret")
                .expect("deployment should resolve");

        assert_eq!(deployment, DeploymentType::Hosted);
        assert_eq!(
            oauth,
            Some(HostedOAuth {
                client_id: "oauth-client-id".to_owned(),
                client_secret: "oauth-secret".to_owned(),
            })
        );
    }

    #[test]
    fn resolve_deployment_rejects_github_client_secret_without_client_id() {
        let err = resolve_deployment_from_env(&options(), "", "oauth-secret")
            .expect_err("deployment should reject a secret without a client id");

        assert!(err.to_string().contains(GITHUB_CLIENT_ID_ENV));
        assert!(err.to_string().contains(GITHUB_CLIENT_SECRET_ENV));
    }
}
