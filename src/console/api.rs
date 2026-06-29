use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value as JsonValue, json};

use crate::error::RototoError;

use super::ConsoleStateMode;
use super::auth::{HostedOAuth, LocalAuth, session_from_headers};
use super::capabilities::{DeploymentType, WritePolicy};
use super::github::{self, GitHubClient, GitHubError};
use super::identity::resolve_git_config_identity;
use super::local_git;
use super::lsp::LspSessions;
use super::observability::{
    DevObservability, current_request_observability, scope_request_observability,
};
use super::runtime_config::{ConsoleRuntimeConfig, RequestObservabilityContext};
use super::stage::StageCache;
use super::store::{
    ActiveBranchWithPackageRecord, PackageRecord, RequestContextNames, SessionUser,
    SourceTreeWithPackages, Store,
};

mod auth_routes;
mod source_trees;

/// Process-wide console dependencies shared by every API route.
///
/// This is built once in `console::run`, wrapped in `Arc`, and then treated as
/// immutable route state. Interior mutability lives inside the store, stage
/// cache, LSP session map, local auth state, and optional observability sink so
/// request handlers can coordinate blocking resources without replacing the
/// state object itself.
pub struct ConsoleState {
    pub deployment: DeploymentType,
    pub oauth: Option<HostedOAuth>,
    pub state_mode: ConsoleStateMode,
    pub write_policy: WritePolicy,
    pub fixed_package_source: Option<String>,
    pub store: Store,
    pub github: GitHubClient,
    pub stage: StageCache,
    pub lsp: LspSessions,
    pub local: Option<LocalAuth>,
    /// Origin used for OAuth redirects and cookies, e.g. http://127.0.0.1:7686.
    pub public_url: String,
    pub allowed_origins: Vec<String>,
    pub secure_cookies: bool,
    pub observability: Option<DevObservability>,
    pub runtime_config: ConsoleRuntimeConfig,
}

/// Axum state handle cloned into routers, middleware, and background tasks.
///
/// The lifecycle is the server lifecycle: dropping the last clone shuts down
/// in-memory caches and sessions after the listener exits.
pub type SharedState = Arc<ConsoleState>;

/// API error envelope plus HTTP status.
///
/// Handlers construct this at the boundary where a domain, GitHub, auth, or
/// staging error becomes a browser-facing JSON response. It is not persisted;
/// `IntoResponse` consumes it into `{ "error": message }`.
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "unauthorized".to_owned(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub fn github(error: &GitHubError, action: &str) -> Self {
        Self::bad_request(github::github_error_message(error, action))
    }
}

impl From<RototoError> for ApiError {
    fn from(err: RototoError) -> Self {
        Self::internal(err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

/// Route result alias that makes every fallible handler return `ApiError`.
///
/// The value lives only for one request and is converted by axum before the
/// response leaves the process.
pub type ApiResult<T> = std::result::Result<T, ApiError>;

pub fn router(state: SharedState) -> axum::Router {
    let mut api = axum::Router::new()
        .route("/me", get(auth_routes::me))
        .route("/auth/logout", post(auth_routes::logout))
        .route("/auth/github/start", get(auth_routes::oauth_start))
        .route("/auth/github/callback", get(auth_routes::oauth_callback))
        .route("/auth/device/start", post(auth_routes::device_start))
        .route("/auth/device/poll", post(auth_routes::device_poll))
        .route("/console", get(console_data))
        .route(
            "/source-trees",
            get(source_trees::source_trees_list).post(source_trees::source_trees_register),
        )
        .route(
            "/source-trees/{source_tree_id}",
            axum::routing::delete(source_trees::source_tree_delete),
        )
        .route(
            "/source-trees/{source_tree_id}/refresh",
            post(source_trees::source_tree_refresh),
        )
        .merge(super::api_package::routes())
        .merge(super::api_branch::routes());
    if state.observability.is_some() {
        api = api.route("/dev/observability/events", post(dev_observability_event));
    }

    axum::Router::new()
        .nest("/api", api)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            request_guard,
        ))
        .with_state(state)
}

/// Mutation guard: cross-site requests are blocked by requiring a custom header
/// plus an Origin check.
/// Custom headers cannot be attached cross-site without a CORS preflight,
/// which this server never grants.
async fn request_guard(State(state): State<SharedState>, request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let route = route_observability(&path);
    let host = request_host(request.headers());
    let names = request_context_names(&state, &route).await;
    let mutating = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let observed = ObservedApiRequest {
        method,
        path,
        route,
        host,
        names,
        mutating,
    };
    let pre_policy = state
        .runtime_config
        .resolve_request_observability(observed.runtime_context(false, 0, "none", 0))
        .await;
    if observed.mutating && observed.path.starts_with("/api") {
        let headers = request.headers();
        if !headers.contains_key("x-rototo-console") {
            let response = ApiError {
                status: StatusCode::FORBIDDEN,
                message: "missing x-rototo-console request header".to_owned(),
            }
            .into_response();
            record_api_request(&state, started, &observed, response.status()).await;
            return response;
        }
        if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
            && !state
                .allowed_origins
                .iter()
                .any(|allowed| allowed == origin)
        {
            let response = ApiError {
                status: StatusCode::FORBIDDEN,
                message: format!("origin {origin} is not allowed"),
            }
            .into_response();
            record_api_request(&state, started, &observed, response.status()).await;
            return response;
        }
    }
    let response = scope_request_observability(pre_policy, next.run(request)).await;
    record_api_request(&state, started, &observed, response.status()).await;
    response
}

async fn record_api_request(
    state: &ConsoleState,
    started: Instant,
    observed: &ObservedApiRequest,
    status: StatusCode,
) {
    let latency_ms = started.elapsed().as_millis();
    let policy = state
        .runtime_config
        .resolve_request_observability(observed.runtime_context(
            true,
            status.as_u16(),
            status_class(status),
            latency_ms,
        ))
        .await;
    tracing::info!(
        operation = "console.api.request",
        method = observed.method.as_str(),
        route = %observed.route.pattern,
        status = status.as_u16(),
        status_class = status_class(status),
        latency_ms,
        deployment = state.deployment.label(),
        host = observed.host.as_deref(),
        repo = observed.names.repo.as_deref(),
        package = observed.names.package.as_deref(),
        branch = observed.names.branch.as_deref(),
        package_id = observed.route.package_id.as_deref(),
        branch_id = observed.route.branch_id.as_deref(),
        error_class = error_class(status),
        request_tracing_filter = %policy.tracing.filter,
        "console API request completed"
    );
    let Some(observability) = &state.observability else {
        return;
    };
    observability
        .record_api_request(
            json!({
                "method": observed.method.as_str(),
                "path": observed.path.as_str(),
                "route": observed.route.pattern.as_str(),
                "status": status.as_u16(),
                "status_class": status_class(status),
                "latency_ms": latency_ms,
                "deployment": state.deployment.label(),
                "host": observed.host.as_deref(),
                "repo": observed.names.repo.as_deref(),
                "package": observed.names.package.as_deref(),
                "branch": observed.names.branch.as_deref(),
                "mutating": observed.mutating,
                "package_id": observed.route.package_id.as_deref(),
                "branch_id": observed.route.branch_id.as_deref(),
                "error_class": error_class(status),
            }),
            &policy,
        )
        .await;
}

struct ObservedApiRequest {
    method: Method,
    path: String,
    route: RouteObservability,
    host: Option<String>,
    names: RequestContextNames,
    mutating: bool,
}

impl ObservedApiRequest {
    fn runtime_context(
        &self,
        response_present: bool,
        status: u16,
        status_class: &'static str,
        latency_ms: u128,
    ) -> RequestObservabilityContext {
        RequestObservabilityContext {
            host: self.host.clone(),
            method: self.method.as_str().to_owned(),
            route: self.route.pattern.clone(),
            repo: self.names.repo.clone(),
            package: self.names.package.clone(),
            branch: self.names.branch.clone(),
            mutating: self.mutating,
            response_present,
            status,
            status_class,
            latency_ms,
        }
    }
}

/// Normalized request identity used by the development observability sink.
///
/// The middleware derives this from the raw path for one request so metrics can
/// group `/api/packages/<id>` style paths without storing every concrete id
/// as a separate route label.
struct RouteObservability {
    pattern: String,
    package_id: Option<String>,
    branch_id: Option<String>,
}

async fn request_context_names(
    state: &ConsoleState,
    route: &RouteObservability,
) -> RequestContextNames {
    match state
        .store
        .request_context_names(route.package_id.as_deref(), route.branch_id.as_deref())
        .await
    {
        Ok(names) => names,
        Err(err) => {
            tracing::warn!(
                operation = "console.api.request_context",
                error = %err,
                package_id = route.package_id.as_deref(),
                branch_id = route.branch_id.as_deref(),
                "console request context names could not be loaded"
            );
            RequestContextNames::default()
        }
    }
}

fn request_host(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn route_observability(path: &str) -> RouteObservability {
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut pattern = Vec::new();
    let mut package_id = None;
    let mut branch_id = None;
    let mut index = 0;
    while index < segments.len() {
        let segment = segments[index];
        pattern.push(segment.to_owned());
        if segment == "packages" && index + 1 < segments.len() {
            package_id = Some(segments[index + 1].to_owned());
            pattern.push(":package_id".to_owned());
            index += 2;
            continue;
        }
        if segment == "branches" && index + 1 < segments.len() {
            branch_id = Some(segments[index + 1].to_owned());
            pattern.push(":branch_id".to_owned());
            index += 2;
            continue;
        }
        if segment == "source-trees" && index + 1 < segments.len() {
            pattern.push(":source_tree_id".to_owned());
            index += 2;
            continue;
        }
        index += 1;
    }
    RouteObservability {
        pattern: format!("/{}", pattern.join("/")),
        package_id,
        branch_id,
    }
}

fn status_class(status: StatusCode) -> &'static str {
    if status.is_server_error() {
        "server_error"
    } else if status.is_client_error() {
        "client_error"
    } else if status.is_redirection() {
        "redirect"
    } else {
        "success"
    }
}

fn error_class(status: StatusCode) -> Option<&'static str> {
    if status.is_server_error() {
        Some("server_error")
    } else if status.is_client_error() {
        Some("client_error")
    } else {
        None
    }
}

/// The actor for a request, per deployment type.
pub async fn require_user(state: &ConsoleState, headers: &HeaderMap) -> ApiResult<SessionUser> {
    match state.deployment {
        DeploymentType::Hosted => {
            let user = session_from_headers(&state.store, headers).await;
            match user {
                Some(user) => {
                    tracing::debug!(
                        operation = "console.auth.require_user",
                        deployment = "hosted",
                        principal_id = %user.principal_id,
                        "console request authorized from hosted session"
                    );
                    Ok(user)
                }
                None => {
                    tracing::info!(
                        operation = "console.auth.require_user",
                        deployment = "hosted",
                        "console request rejected without hosted session"
                    );
                    Err(ApiError::unauthorized())
                }
            }
        }
        DeploymentType::Local => {
            let local = state
                .local
                .as_ref()
                .expect("local deployment has local auth");
            match local.identity(&state.github).await {
                Ok(Some(user)) => {
                    tracing::debug!(
                        operation = "console.auth.require_user",
                        deployment = "local",
                        principal_id = %user.principal_id,
                        source = "github_token",
                        "console request authorized from local GitHub identity"
                    );
                    Ok(user)
                }
                Ok(None) => {
                    let local_root = state
                        .fixed_package_source
                        .as_deref()
                        .and_then(|source| local_git::package_root(source).ok());
                    let identity = resolve_git_config_identity(local_root.as_deref())
                        .await
                        .map_err(ApiError::from)?;
                    Ok(SessionUser {
                        session_hash: "local-git".to_owned(),
                        principal_id: identity.principal_id(),
                        identity,
                        github_token: None,
                    })
                    .inspect(|user| {
                        tracing::debug!(
                            operation = "console.auth.require_user",
                            deployment = "local",
                            principal_id = %user.principal_id,
                            source = "git_config",
                            "console request authorized from git config fallback"
                        );
                    })
                }
                Err(err) => {
                    let local_root = state
                        .fixed_package_source
                        .as_deref()
                        .and_then(|source| local_git::package_root(source).ok());
                    let identity = resolve_git_config_identity(local_root.as_deref())
                        .await
                        .map_err(|_| ApiError {
                            status: StatusCode::UNAUTHORIZED,
                            message: err.to_string(),
                        })?;
                    Ok(SessionUser {
                        session_hash: "local-git".to_owned(),
                        principal_id: identity.principal_id(),
                        identity,
                        github_token: None,
                    })
                    .inspect(|user| {
                        tracing::info!(
                            operation = "console.auth.require_user",
                            deployment = "local",
                            principal_id = %user.principal_id,
                            source = "git_config",
                            token_error = %err,
                            "console request authorized from git config after token auth failed"
                        );
                    })
                }
            }
        }
    }
}

pub fn require_github_token<'a>(user: &'a SessionUser, action: &str) -> ApiResult<&'a str> {
    user.github_token
        .as_deref()
        .ok_or_else(|| ApiError::bad_request(format!("{action} requires a GitHub credential")))
}

pub fn source_token(user: &SessionUser) -> &str {
    user.github_token.as_deref().unwrap_or("")
}

pub(crate) async fn fixed_source_scope(
    state: &ConsoleState,
    principal_id: &str,
) -> ApiResult<Option<SourceTreeWithPackages>> {
    let Some(source) = state.fixed_package_source.as_deref() else {
        return Ok(None);
    };
    let source_tree = super::register_fixed_package(state, principal_id, source).await?;
    Ok(Some(source_tree))
}

pub(crate) fn fixed_source_package_ids(fixed_source: &SourceTreeWithPackages) -> HashSet<String> {
    fixed_source
        .packages
        .iter()
        .map(|package| package.id.clone())
        .collect()
}

pub(crate) fn package_belongs_to_fixed_source(
    package: &PackageRecord,
    fixed_source: &SourceTreeWithPackages,
) -> bool {
    package.source_tree_id == fixed_source.source_tree.id
}

fn fixed_source_branch_filter(
    branches: Vec<ActiveBranchWithPackageRecord>,
    fixed_source: &SourceTreeWithPackages,
) -> Vec<ActiveBranchWithPackageRecord> {
    branches
        .into_iter()
        .filter(|entry| package_belongs_to_fixed_source(&entry.package, fixed_source))
        .collect()
}

pub(super) fn source_tree_management_allowed(state: &ConsoleState) -> ApiResult<()> {
    if state.fixed_package_source.is_some() {
        return Err(ApiError::bad_request(
            "this console was started with a fixed package source",
        ));
    }
    Ok(())
}

fn console_state_json(state: &ConsoleState) -> JsonValue {
    json!({
        "mode": state.state_mode.label(),
        "fixedPackage": state.fixed_package_source.is_some(),
        "canManageSourceTrees": state.fixed_package_source.is_none(),
    })
}

async fn dev_observability_event(
    State(state): State<SharedState>,
    Json(event): Json<JsonValue>,
) -> ApiResult<Json<JsonValue>> {
    let Some(observability) = &state.observability else {
        return Err(ApiError::not_found("dev observability is disabled"));
    };
    let policy = current_request_observability()
        .unwrap_or_else(|| state.runtime_config.default_request_observability());
    observability.record_ui_event(event, &policy).await;
    Ok(Json(json!({ "ok": true })))
}

async fn console_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let fixed_source = fixed_source_scope(&state, &user.principal_id).await?;
    let (source_trees, packages) = match fixed_source.as_ref() {
        Some(source_tree) => (vec![source_tree.clone()], source_tree.packages.clone()),
        None => (
            state
                .store
                .list_source_trees_for_user(&user.principal_id)
                .await?,
            state
                .store
                .list_packages_for_user(&user.principal_id)
                .await?,
        ),
    };
    let mut branches = state
        .store
        .list_active_branches_with_packages_for_user(&user.principal_id)
        .await?;
    if let Some(source_tree) = fixed_source.as_ref() {
        branches = fixed_source_branch_filter(branches, source_tree);
    }
    Ok(Json(json!({
        "state": console_state_json(&state),
        "sourceTrees": source_trees,
        "packages": packages,
        "branches": branches,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use super::source_trees::{
        refresh_source_tree_for_user, should_register_as_github, source_tree_ref_hint,
        upsert_read_only_source_tree,
    };
    use crate::console::identity::ActorIdentity;
    use crate::console::token_crypto::TokenCrypto;
    use tempfile::TempDir;

    #[test]
    fn observability_route_extracts_package_and_branch_ids() {
        let route = route_observability("/api/packages/ws-1/branches/branch-2/lsp");

        assert_eq!(
            route.pattern,
            "/api/packages/:package_id/branches/:branch_id/lsp"
        );
        assert_eq!(route.package_id.as_deref(), Some("ws-1"));
        assert_eq!(route.branch_id.as_deref(), Some("branch-2"));
    }

    #[test]
    fn observability_route_replaces_source_tree_ids() {
        let route = route_observability("/api/source-trees/tree-1");

        assert_eq!(route.pattern, "/api/source-trees/:source_tree_id");
        assert_eq!(route.package_id, None);
        assert_eq!(route.branch_id, None);
    }

    #[test]
    fn observability_route_keeps_source_tree_refresh_action() {
        let route = route_observability("/api/source-trees/tree-1/refresh");

        assert_eq!(route.pattern, "/api/source-trees/:source_tree_id/refresh");
    }

    #[test]
    fn source_tree_ref_hint_reads_git_fragment_refs() {
        assert_eq!(
            source_tree_ref_hint("git+https://github.com/o/r.git#release:apps").as_deref(),
            Some("release")
        );
        assert_eq!(
            source_tree_ref_hint("git+https://github.com/o/r.git#release").as_deref(),
            Some("release")
        );
        assert_eq!(
            source_tree_ref_hint("git+https://github.com/o/r.git#:"),
            None
        );
        assert_eq!(source_tree_ref_hint("git+https://github.com/o/r.git"), None);
    }

    #[tokio::test]
    async fn github_registration_detection_accepts_git_github_sources() {
        assert!(
            should_register_as_github("git+https://github.com/octo/configs.git#main:apps").await
        );
        assert!(should_register_as_github("git+ssh://git@github.com/octo/configs.git#main").await);
        assert!(!should_register_as_github("git+file:///tmp/configs.git#main").await);
        assert!(!should_register_as_github("git+https://example.com/octo/configs.git#main").await);
    }

    #[tokio::test]
    async fn source_tree_refresh_rediscovers_local_package_paths() {
        let tree = TempDir::new().expect("source tree tempdir");
        write_package(tree.path()).await;
        let state = test_state();
        let user = test_user();
        let source = tree.path().to_str().expect("utf8 temp path");
        let (registered, _) =
            expect_api_ok(upsert_read_only_source_tree(&state, &user, source, None).await);
        assert_eq!(package_paths(&registered.packages), vec!["."]);

        write_package(&tree.path().join("packages/payments")).await;
        let refreshed = expect_api_ok(
            refresh_source_tree_for_user(&state, &user, &registered.source_tree.id).await,
        );

        assert_eq!(
            package_paths(&refreshed.packages),
            vec![".", "packages/payments"]
        );
        assert!(refreshed.source_tree.last_discovered_at.is_some());
    }

    #[tokio::test]
    async fn fixed_package_scope_rejects_stale_package_ids() {
        let fixed = TempDir::new().expect("fixed source tempdir");
        write_package(fixed.path()).await;
        let stale = TempDir::new().expect("stale source tempdir");
        write_package(stale.path()).await;
        let fixed_source = fixed.path().to_str().expect("utf8 temp path");
        let stale_source = stale.path().to_str().expect("utf8 temp path");
        let state = test_state_with_fixed_source(fixed_source);
        let user = test_user();

        let (stale_registered, _) =
            expect_api_ok(upsert_read_only_source_tree(&state, &user, stale_source, None).await);
        let stale_package = stale_registered
            .packages
            .first()
            .expect("stale package should be discovered");

        let err = super::super::api_package::load_package(&state, &user, &stale_package.id)
            .await
            .expect_err("fixed source should hide stale package rows");

        assert_eq!(err.status, StatusCode::NOT_FOUND);
        let fixed_registered = expect_api_ok(fixed_source_scope(&state, &user.principal_id).await)
            .expect("fixed source should register");
        assert_eq!(fixed_registered.source_tree.source, fixed_source);
        assert_eq!(package_paths(&fixed_registered.packages), vec!["."]);
    }

    fn expect_api_ok<T>(result: ApiResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{}", err.message),
        }
    }

    fn test_state() -> SharedState {
        test_state_with_fixed_source_option(None)
    }

    fn test_state_with_fixed_source(source: &str) -> SharedState {
        test_state_with_fixed_source_option(Some(source.to_owned()))
    }

    fn test_state_with_fixed_source_option(fixed_package_source: Option<String>) -> SharedState {
        let fixed_package = fixed_package_source.is_some();
        Arc::new(ConsoleState {
            deployment: DeploymentType::Local,
            oauth: None,
            state_mode: ConsoleStateMode::Ephemeral,
            write_policy: WritePolicy::Disabled,
            fixed_package_source,
            store: Store::open_in_memory(TokenCrypto::generate().unwrap()).unwrap(),
            github: GitHubClient::new(),
            stage: StageCache::new(),
            lsp: LspSessions::new(),
            local: None,
            public_url: "http://127.0.0.1:7686".to_owned(),
            allowed_origins: vec!["http://127.0.0.1:7686".to_owned()],
            secure_cookies: false,
            observability: None,
            runtime_config: ConsoleRuntimeConfig::built_in(
                super::super::runtime_config::ConsoleRuntimeBase {
                    deployment: DeploymentType::Local,
                    write_policy: WritePolicy::Disabled,
                    console_host: Some("127.0.0.1".to_owned()),
                    fixed_package,
                    secure_cookies: false,
                },
            ),
        })
    }

    fn test_user() -> super::super::store::SessionUser {
        let identity = ActorIdentity::GitConfig {
            name: Some("Console Test".to_owned()),
            email: Some("console@example.com".to_owned()),
        };
        super::super::store::SessionUser {
            session_hash: "test-session".to_owned(),
            principal_id: identity.principal_id(),
            identity,
            github_token: None,
        }
    }

    async fn write_package(path: &std::path::Path) {
        tokio::fs::create_dir_all(path).await.unwrap();
        tokio::fs::write(path.join("rototo-package.toml"), "schema_version = 1\n")
            .await
            .unwrap();
    }

    fn package_paths(packages: &[super::super::store::PackageRecord]) -> Vec<&str> {
        packages
            .iter()
            .map(|package| package.path.as_str())
            .collect()
    }
}
