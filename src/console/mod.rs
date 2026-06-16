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
mod runtime_config;
mod stage;
mod static_assets;
mod store;
mod time;
mod token_crypto;
mod variable_toml;
mod workspace_edit;
mod workspace_source;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::error::{Result, RototoError};

use self::api::ConsoleState;
use self::auth::{
    GITHUB_CLIENT_ID_ENV, GITHUB_CLIENT_SECRET_ENV, HostedOAuth, LocalAuth, baked_device_client_id,
    resolve_ambient_token,
};
use self::capabilities::{DeploymentType, WritePolicy};
use self::github::GitHubClient;
use self::lsp::LspSessions;
use self::observability::DevObservability;
use self::runtime_config::{ConsoleRuntimeBase, ConsoleRuntimeConfig, public_url_host};
use self::stage::StageCache;
use self::store::Store;
use self::token_crypto::TokenCrypto;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Registry;
use tracing_subscriber::reload::Handle as TracingReloadHandle;

pub const DEFAULT_BIND: &str = "127.0.0.1:7686";
pub use self::capabilities::WritePolicy as ConsoleWritePolicy;

const CONSOLE_PUBLIC_URL_ENV: &str = "ROTOTO_CONSOLE_PUBLIC_URL";
const CONSOLE_DATA_DIR_ENV: &str = "ROTOTO_CONSOLE_DATA_DIR";
const WORKSPACE_TOKEN_ENV: &str = "ROTOTO_WORKSPACE_TOKEN";

static TRACING_FILTER_RELOAD: OnceLock<TracingReloadHandle<EnvFilter, Registry>> = OnceLock::new();

pub fn set_tracing_filter_reload_handle(handle: TracingReloadHandle<EnvFilter, Registry>) {
    let _ = TRACING_FILTER_RELOAD.set(handle);
}

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

/// Optional per-user console startup environment.
///
/// Values come from `${XDG_CONFIG_HOME:-$HOME/.config}/rototo/admin.env` and
/// are used only by `rototo console`. Process environment values still win so
/// a one-off shell override does not require editing the file.
#[derive(Default)]
struct ConsoleAdminEnv {
    values: HashMap<String, String>,
}

impl ConsoleAdminEnv {
    async fn load() -> Result<Self> {
        Self::load_from_path(admin_env_path()).await
    }

    async fn load_from_path(path: Option<PathBuf>) -> Result<Self> {
        let Some(path) = path else {
            tracing::debug!(
                operation = "console.admin_env.load",
                "console admin env path could not be resolved"
            );
            return Ok(Self::default());
        };
        let contents = match tokio::fs::read_to_string(&path).await {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(
                    operation = "console.admin_env.load",
                    path = %path.display(),
                    "console admin env file not found"
                );
                return Ok(Self::default());
            }
            Err(err) => {
                tracing::warn!(
                    operation = "console.admin_env.load",
                    path = %path.display(),
                    error = %err,
                    "console admin env file could not be read"
                );
                return Err(RototoError::new(format!(
                    "failed to read console admin env {}: {err}",
                    path.display()
                )));
            }
        };
        let values = parse_admin_env(&path, &contents)?;
        tracing::info!(
            operation = "console.admin_env.load",
            path = %path.display(),
            keys = values.len(),
            "console admin env file loaded"
        );
        Ok(Self { values })
    }

    fn get(&self, key: &str) -> Option<String> {
        self.get_with_process_value(key, std::env::var(key).ok())
    }

    fn get_with_process_value(&self, key: &str, process_value: Option<String>) -> Option<String> {
        process_value
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.values
                    .get(key)
                    .filter(|value| !value.trim().is_empty())
                    .cloned()
            })
    }
}

pub async fn run(options: ConsoleOptions) -> Result<()> {
    let admin_env = ConsoleAdminEnv::load().await?;
    let data_dir = match options.data_dir.clone() {
        Some(dir) => dir,
        None => match admin_env.get(CONSOLE_DATA_DIR_ENV) {
            Some(dir) => PathBuf::from(dir),
            None => default_data_dir()?,
        },
    };
    tokio::fs::create_dir_all(&data_dir).await.map_err(|err| {
        RototoError::new(format!(
            "failed to create console data directory {}: {err}",
            data_dir.display()
        ))
    })?;

    let (deployment, oauth) = resolve_deployment(&admin_env)?;
    tracing::info!(
        operation = "console.startup",
        deployment = deployment.label(),
        write_policy = options.write_policy.label(),
        data_dir = %data_dir.display(),
        fixed_workspace = options.workspace.is_some(),
        "console startup configuration resolved"
    );

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
        .or_else(|| admin_env.get(CONSOLE_PUBLIC_URL_ENV))
        .map(|url| url.trim_end_matches('/').to_owned())
        .unwrap_or_else(|| format!("http://{bound}"));
    let secure_cookies = public_url.starts_with("https://");
    let allowed_origins = allowed_origins(&public_url, bound.port());
    let console_host = public_url_host(&public_url);
    let runtime_config = ConsoleRuntimeConfig::load(ConsoleRuntimeBase {
        deployment: deployment.clone(),
        write_policy: options.write_policy,
        console_host: console_host.clone(),
        fixed_workspace: options.workspace.is_some(),
        secure_cookies,
    })
    .await?;
    reload_console_tracing_filter(&runtime_config.startup_observability().tracing.filter);
    let observability =
        DevObservability::from_config(&data_dir, runtime_config.startup_observability()).await?;
    tracing::info!(
        operation = "console.listen",
        bind = %bound,
        public_url = %public_url,
        console_host = console_host.as_deref(),
        secure_cookies,
        allowed_origins = allowed_origins.len(),
        tracing_filter = %runtime_config.startup_observability().tracing.filter,
        "console listener bound"
    );

    let token_key = admin_env.get(token_crypto::KEY_ENV);
    let crypto = resolve_token_crypto(&deployment, &data_dir, token_key.as_deref()).await?;
    let store = Store::open(&data_dir.join("console.db"), crypto)?;

    let local = match deployment {
        DeploymentType::Local => {
            let workspace_token = options
                .workspace_token
                .clone()
                .or_else(|| admin_env.get(WORKSPACE_TOKEN_ENV));
            let ambient = resolve_ambient_token(workspace_token.as_deref(), &data_dir).await;
            let device_client_id = admin_env
                .get(GITHUB_CLIENT_ID_ENV)
                .or_else(baked_device_client_id);
            Some(LocalAuth::new(ambient, &data_dir, device_client_id))
        }
        DeploymentType::Hosted => None,
    };

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
        runtime_config,
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

fn reload_console_tracing_filter(filter: &str) {
    let Some(handle) = TRACING_FILTER_RELOAD.get() else {
        tracing::warn!(
            operation = "console.tracing.reload",
            tracing_filter = filter,
            "console tracing reload handle is not installed"
        );
        return;
    };
    match EnvFilter::try_new(filter) {
        Ok(filter) => {
            if let Err(err) = handle.reload(filter) {
                tracing::warn!(
                    operation = "console.tracing.reload",
                    error = %err,
                    "console tracing filter could not be reloaded"
                );
            }
        }
        Err(err) => {
            tracing::warn!(
                operation = "console.tracing.reload",
                tracing_filter = filter,
                error = %err,
                "console tracing filter is invalid"
            );
        }
    }
}

fn resolve_deployment(
    admin_env: &ConsoleAdminEnv,
) -> Result<(DeploymentType, Option<HostedOAuth>)> {
    let client_id = admin_env.get(GITHUB_CLIENT_ID_ENV).unwrap_or_default();
    let client_secret = admin_env.get(GITHUB_CLIENT_SECRET_ENV).unwrap_or_default();
    resolve_deployment_from_env(&client_id, &client_secret)
}

fn resolve_deployment_from_env(
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
    env_value: Option<&str>,
) -> Result<TokenCrypto> {
    if let Some(raw) = env_value {
        return TokenCrypto::from_env_value(raw);
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
    if let Ok(dir) = std::env::var(CONSOLE_DATA_DIR_ENV)
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

fn admin_env_path() -> Option<PathBuf> {
    admin_env_path_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

fn admin_env_path_from(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    if let Some(dir) = xdg_config_home.filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("rototo/admin.env"));
    }
    home.filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join(".config/rototo/admin.env"))
}

fn parse_admin_env(path: &std::path::Path, contents: &str) -> Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    for (index, line) in contents.lines().enumerate() {
        let line_no = index + 1;
        let mut line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(admin_env_parse_error(path, line_no, "expected KEY=value"));
        };
        let key = key.trim();
        if !valid_env_key(key) {
            return Err(admin_env_parse_error(
                path,
                line_no,
                format!("invalid environment key `{key}`"),
            ));
        }
        values.insert(key.to_owned(), parse_admin_env_value(path, line_no, value)?);
    }
    Ok(values)
}

fn valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn parse_admin_env_value(path: &std::path::Path, line_no: usize, raw: &str) -> Result<String> {
    let raw = raw.trim();
    match raw.as_bytes().first().copied() {
        Some(b'"') => parse_double_quoted_admin_env_value(path, line_no, raw),
        Some(b'\'') => parse_single_quoted_admin_env_value(path, line_no, raw),
        _ => Ok(strip_unquoted_admin_env_comment(raw).trim_end().to_owned()),
    }
}

fn parse_single_quoted_admin_env_value(
    path: &std::path::Path,
    line_no: usize,
    raw: &str,
) -> Result<String> {
    let Some(end) = raw[1..].find('\'').map(|index| index + 1) else {
        return Err(admin_env_parse_error(
            path,
            line_no,
            "unterminated single-quoted value",
        ));
    };
    ensure_admin_env_value_tail(path, line_no, &raw[end + 1..])?;
    Ok(raw[1..end].to_owned())
}

fn parse_double_quoted_admin_env_value(
    path: &std::path::Path,
    line_no: usize,
    raw: &str,
) -> Result<String> {
    let mut value = String::new();
    let mut escaped = false;
    for (index, ch) in raw[1..].char_indices() {
        let absolute = index + 1;
        if escaped {
            value.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                ensure_admin_env_value_tail(path, line_no, &raw[absolute + 1..])?;
                return Ok(value);
            }
            other => value.push(other),
        }
    }
    Err(admin_env_parse_error(
        path,
        line_no,
        "unterminated double-quoted value",
    ))
}

fn ensure_admin_env_value_tail(path: &std::path::Path, line_no: usize, tail: &str) -> Result<()> {
    let tail = tail.trim_start();
    if tail.is_empty() || tail.starts_with('#') {
        return Ok(());
    }
    Err(admin_env_parse_error(
        path,
        line_no,
        "unexpected characters after quoted value",
    ))
}

fn strip_unquoted_admin_env_comment(raw: &str) -> &str {
    for (index, ch) in raw.char_indices() {
        if ch == '#'
            && (index == 0
                || raw[..index]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
        {
            return &raw[..index];
        }
    }
    raw
}

fn admin_env_parse_error(
    path: &std::path::Path,
    line_no: usize,
    detail: impl std::fmt::Display,
) -> RototoError {
    RototoError::new(format!(
        "failed to parse console admin env {} line {line_no}: {detail}",
        path.display()
    ))
}

fn allowed_origins(public_url: &str, port: u16) -> Vec<String> {
    let mut origins = vec![
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        "http://127.0.0.1:5173".to_owned(),
        "http://localhost:5173".to_owned(),
        "http://dev.rototo.dev:5173".to_owned(),
    ];
    let public_origin = public_url.trim_end_matches('/').to_owned();
    if !origins.contains(&public_origin) {
        origins.push(public_origin);
    }
    origins.dedup();
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

    #[test]
    fn resolve_deployment_stays_local_with_only_github_client_id() {
        let (deployment, oauth) =
            resolve_deployment_from_env("device-client-id", "").expect("deployment should resolve");

        assert_eq!(deployment, DeploymentType::Local);
        assert!(oauth.is_none());
    }

    #[test]
    fn resolve_deployment_uses_hosted_when_namespaced_github_oauth_pair_is_set() {
        let (deployment, oauth) = resolve_deployment_from_env("oauth-client-id", "oauth-secret")
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
        let err = resolve_deployment_from_env("", "oauth-secret")
            .expect_err("deployment should reject a secret without a client id");

        assert!(err.to_string().contains(GITHUB_CLIENT_ID_ENV));
        assert!(err.to_string().contains(GITHUB_CLIENT_SECRET_ENV));
    }

    #[test]
    fn admin_env_path_uses_xdg_config_home() {
        let path = admin_env_path_from(Some("/tmp/xdg".into()), Some("/tmp/home".into())).unwrap();

        assert_eq!(path, PathBuf::from("/tmp/xdg/rototo/admin.env"));
    }

    #[test]
    fn admin_env_path_falls_back_to_home_config() {
        let path = admin_env_path_from(None, Some("/tmp/home".into())).unwrap();

        assert_eq!(path, PathBuf::from("/tmp/home/.config/rototo/admin.env"));
    }

    #[test]
    fn parse_admin_env_supports_common_dotenv_syntax() {
        let values = parse_admin_env(
            std::path::Path::new("/tmp/admin.env"),
            r#"
                # comment
                ROTOTO_GITHUB_CLIENT_ID=client-id
                export ROTOTO_GITHUB_CLIENT_SECRET='client secret'
                ROTOTO_CONSOLE_PUBLIC_URL="https://dev.rototo.dev"
                ROTOTO_WORKSPACE_TOKEN=ghp_hash#kept
                ROTOTO_CONSOLE_DATA_DIR=/tmp/rototo # trailing comment
            "#,
        )
        .unwrap();

        assert_eq!(
            values.get("ROTOTO_GITHUB_CLIENT_ID").map(String::as_str),
            Some("client-id")
        );
        assert_eq!(
            values
                .get("ROTOTO_GITHUB_CLIENT_SECRET")
                .map(String::as_str),
            Some("client secret")
        );
        assert_eq!(
            values.get("ROTOTO_CONSOLE_PUBLIC_URL").map(String::as_str),
            Some("https://dev.rototo.dev")
        );
        assert_eq!(
            values.get("ROTOTO_WORKSPACE_TOKEN").map(String::as_str),
            Some("ghp_hash#kept")
        );
        assert_eq!(
            values.get("ROTOTO_CONSOLE_DATA_DIR").map(String::as_str),
            Some("/tmp/rototo")
        );
    }

    #[test]
    fn parse_admin_env_rejects_invalid_lines() {
        let err = parse_admin_env(std::path::Path::new("/tmp/admin.env"), "not a binding")
            .expect_err("invalid line should fail");

        assert!(err.to_string().contains("line 1"));
        assert!(err.to_string().contains("expected KEY=value"));
    }

    #[test]
    fn console_admin_env_process_values_override_file_values() {
        let admin_env = ConsoleAdminEnv {
            values: HashMap::from([("ROTOTO_GITHUB_CLIENT_ID".to_owned(), "file".to_owned())]),
        };

        assert_eq!(
            admin_env
                .get_with_process_value("ROTOTO_GITHUB_CLIENT_ID", Some("process".to_owned()))
                .as_deref(),
            Some("process")
        );
        assert_eq!(
            admin_env
                .get_with_process_value("ROTOTO_GITHUB_CLIENT_ID", Some("".to_owned()))
                .as_deref(),
            Some("file")
        );
    }

    #[tokio::test]
    async fn console_admin_env_loads_existing_file_and_ignores_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("admin.env");
        tokio::fs::write(&path, "ROTOTO_GITHUB_CLIENT_ID=file-client\n")
            .await
            .unwrap();

        let loaded = ConsoleAdminEnv::load_from_path(Some(path)).await.unwrap();
        let missing = ConsoleAdminEnv::load_from_path(Some(dir.path().join("missing.env")))
            .await
            .unwrap();

        assert_eq!(
            loaded
                .get_with_process_value("ROTOTO_GITHUB_CLIENT_ID", None)
                .as_deref(),
            Some("file-client")
        );
        assert!(missing.values.is_empty());
    }

    #[test]
    fn allowed_origins_include_vite_dev_proxy() {
        let origins = allowed_origins("http://127.0.0.1:7686", 7686);

        assert!(
            origins
                .iter()
                .any(|origin| origin == "http://127.0.0.1:7686")
        );
        assert!(
            origins
                .iter()
                .any(|origin| origin == "http://127.0.0.1:5173")
        );
        assert!(
            origins
                .iter()
                .any(|origin| origin == "http://localhost:5173")
        );
        assert!(
            origins
                .iter()
                .any(|origin| origin == "http://dev.rototo.dev:5173")
        );
    }
}
