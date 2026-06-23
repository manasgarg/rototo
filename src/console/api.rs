use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value as JsonValue, json};

use crate::error::RototoError;

use super::ConsoleStateMode;
use super::auth::{
    GITHUB_OAUTH_SCOPES, GitHubCredentialSource, HostedOAuth, LocalAuth, OAUTH_STATE_COOKIE,
    SESSION_COOKIE, cookie_value, session_from_headers, set_cookie,
};
use super::capabilities::{DeploymentType, WritePolicy};
use super::github::{self, GitHubClient, GitHubError};
use super::identity::{ActorIdentity, resolve_git_config_identity};
use super::local_git;
use super::lsp::LspSessions;
use super::observability::{
    DevObservability, current_request_observability, scope_request_observability,
};
use super::runtime_config::{ConsoleRuntimeConfig, RequestObservabilityContext};
use super::stage::StageCache;
use super::store::{
    ActiveBranchWithWorkspaceRecord, NewSession, RequestContextNames, SessionUser,
    SourceTreeWithWorkspaces, Store, WorkspaceRecord,
};

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
    pub fixed_workspace_source: Option<String>,
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
        .route("/me", get(me))
        .route("/auth/logout", post(logout))
        .route("/auth/github/start", get(oauth_start))
        .route("/auth/github/callback", get(oauth_callback))
        .route("/auth/device/start", post(device_start))
        .route("/auth/device/poll", post(device_poll))
        .route("/console", get(console_data))
        .route(
            "/source-trees",
            get(source_trees_list).post(source_trees_register),
        )
        .route(
            "/source-trees/{source_tree_id}",
            axum::routing::delete(source_tree_delete),
        )
        .route(
            "/source-trees/{source_tree_id}/refresh",
            post(source_tree_refresh),
        )
        .merge(super::api_workspace::routes())
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
        workspace = observed.names.workspace.as_deref(),
        branch = observed.names.branch.as_deref(),
        workspace_id = observed.route.workspace_id.as_deref(),
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
                "workspace": observed.names.workspace.as_deref(),
                "branch": observed.names.branch.as_deref(),
                "mutating": observed.mutating,
                "workspace_id": observed.route.workspace_id.as_deref(),
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
            workspace: self.names.workspace.clone(),
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
/// group `/api/workspaces/<id>` style paths without storing every concrete id
/// as a separate route label.
struct RouteObservability {
    pattern: String,
    workspace_id: Option<String>,
    branch_id: Option<String>,
}

async fn request_context_names(
    state: &ConsoleState,
    route: &RouteObservability,
) -> RequestContextNames {
    match state
        .store
        .request_context_names(route.workspace_id.as_deref(), route.branch_id.as_deref())
        .await
    {
        Ok(names) => names,
        Err(err) => {
            tracing::warn!(
                operation = "console.api.request_context",
                error = %err,
                workspace_id = route.workspace_id.as_deref(),
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
    let mut workspace_id = None;
    let mut branch_id = None;
    let mut index = 0;
    while index < segments.len() {
        let segment = segments[index];
        pattern.push(segment.to_owned());
        if segment == "workspaces" && index + 1 < segments.len() {
            workspace_id = Some(segments[index + 1].to_owned());
            pattern.push(":workspace_id".to_owned());
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
        workspace_id,
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
                        .fixed_workspace_source
                        .as_deref()
                        .and_then(|source| local_git::workspace_root(source).ok());
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
                        .fixed_workspace_source
                        .as_deref()
                        .and_then(|source| local_git::workspace_root(source).ok());
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
) -> ApiResult<Option<SourceTreeWithWorkspaces>> {
    let Some(source) = state.fixed_workspace_source.as_deref() else {
        return Ok(None);
    };
    let source_tree = super::register_fixed_workspace(state, principal_id, source).await?;
    Ok(Some(source_tree))
}

pub(crate) fn fixed_source_workspace_ids(
    fixed_source: &SourceTreeWithWorkspaces,
) -> HashSet<String> {
    fixed_source
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect()
}

pub(crate) fn workspace_belongs_to_fixed_source(
    workspace: &WorkspaceRecord,
    fixed_source: &SourceTreeWithWorkspaces,
) -> bool {
    workspace.source_tree_id == fixed_source.source_tree.id
}

fn fixed_source_branch_filter(
    branches: Vec<ActiveBranchWithWorkspaceRecord>,
    fixed_source: &SourceTreeWithWorkspaces,
) -> Vec<ActiveBranchWithWorkspaceRecord> {
    branches
        .into_iter()
        .filter(|entry| workspace_belongs_to_fixed_source(&entry.workspace, fixed_source))
        .collect()
}

fn source_tree_management_allowed(state: &ConsoleState) -> ApiResult<()> {
    if state.fixed_workspace_source.is_some() {
        return Err(ApiError::bad_request(
            "this console was started with a fixed workspace source",
        ));
    }
    Ok(())
}

fn console_state_json(state: &ConsoleState) -> JsonValue {
    json!({
        "mode": state.state_mode.label(),
        "fixedWorkspace": state.fixed_workspace_source.is_some(),
        "canManageSourceTrees": state.fixed_workspace_source.is_none(),
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

async fn me(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let deployment = state.deployment.label();
    let device_flow = state
        .local
        .as_ref()
        .map(|local| local.device_flow_available())
        .unwrap_or(false);
    let (status, auth_error, user) = match state.deployment {
        DeploymentType::Hosted => match session_from_headers(&state.store, &headers).await {
            Some(user) => (StatusCode::OK, None, Some(user)),
            None => (StatusCode::UNAUTHORIZED, None, None),
        },
        DeploymentType::Local => {
            let local = state
                .local
                .as_ref()
                .expect("local deployment has local auth");
            match local.identity(&state.github).await {
                Ok(Some(user)) => (StatusCode::OK, None, Some(user)),
                Ok(None) => {
                    let local_root = state
                        .fixed_workspace_source
                        .as_deref()
                        .and_then(|source| local_git::workspace_root(source).ok());
                    match resolve_git_config_identity(local_root.as_deref()).await {
                        Ok(identity) => (
                            StatusCode::OK,
                            None,
                            Some(SessionUser {
                                session_hash: "local-git".to_owned(),
                                principal_id: identity.principal_id(),
                                identity,
                                github_token: None,
                            }),
                        ),
                        Err(err) => (StatusCode::OK, Some(err.to_string()), None),
                    }
                }
                Err(err) => {
                    let local_root = state
                        .fixed_workspace_source
                        .as_deref()
                        .and_then(|source| local_git::workspace_root(source).ok());
                    let user = resolve_git_config_identity(local_root.as_deref())
                        .await
                        .ok()
                        .map(|identity| SessionUser {
                            session_hash: "local-git".to_owned(),
                            principal_id: identity.principal_id(),
                            identity,
                            github_token: None,
                        });
                    (StatusCode::OK, Some(err.to_string()), user)
                }
            }
        }
    };
    let token_source = match state.deployment {
        DeploymentType::Local => match state.local.as_ref() {
            Some(local) => local.token().await.map(|ambient| ambient.source),
            None => None,
        },
        DeploymentType::Hosted => user
            .as_ref()
            .and_then(|user| user.github_token.as_ref())
            .map(|_| GitHubCredentialSource::OAuthSession),
    };

    let user_json = user.map(|user| {
        let identity = user.identity.clone();
        json!({
            "principalId": user.principal_id,
            "identity": identity,
            "displayName": user.identity.display_login(),
            "avatarUrl": user.identity.avatar_url(),
            "hasGithubToken": user.github_token.is_some(),
        })
    });
    (
        status,
        Json(json!({
            "deployment": deployment,
            "writePolicy": state.write_policy.label(),
            "deviceFlow": device_flow,
            "tokenSource": token_source,
            "authError": auth_error,
            "user": user_json,
        })),
    )
        .into_response()
}

async fn logout(State(state): State<SharedState>, headers: HeaderMap) -> ApiResult<Response> {
    if state.deployment != DeploymentType::Hosted {
        tracing::info!(
            operation = "console.auth.logout",
            deployment = state.deployment.label(),
            "console logout rejected outside hosted deployment"
        );
        return Err(ApiError::bad_request(
            "logout only applies to hosted consoles",
        ));
    }
    if let Some(token) = cookie_value(&headers, SESSION_COOKIE) {
        state.store.delete_session(&token).await?;
        tracing::info!(
            operation = "console.auth.logout",
            deployment = "hosted",
            had_cookie = true,
            "console hosted session deleted"
        );
    } else {
        tracing::info!(
            operation = "console.auth.logout",
            deployment = "hosted",
            had_cookie = false,
            "console logout completed without session cookie"
        );
    }
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        set_cookie(SESSION_COOKIE, "", state.secure_cookies, Some(0))
            .parse()
            .expect("cookie value is valid"),
    );
    Ok(response)
}

async fn oauth_start(State(state): State<SharedState>) -> ApiResult<Response> {
    let Some(oauth) = &state.oauth else {
        return Err(ApiError::internal(
            "GitHub OAuth client id and secret are required".to_owned(),
        ));
    };

    let state_token = random_token(24)?;
    state.store.create_oauth_state(&state_token).await?;

    let redirect_uri = format!("{}/api/auth/github/callback", state.public_url);
    let authorize_url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}",
        url_encode(&oauth.client_id),
        url_encode(&redirect_uri),
        url_encode(GITHUB_OAUTH_SCOPES),
        url_encode(&state_token),
    );
    let mut response = Redirect::temporary(&authorize_url).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        set_cookie(
            OAUTH_STATE_COOKIE,
            &state_token,
            state.secure_cookies,
            Some(600),
        )
        .parse()
        .expect("cookie value is valid"),
    );
    Ok(response)
}

/// GitHub OAuth callback query parameters.
///
/// GitHub owns the values; the console validates them against the short-lived
/// state cookie and stored nonce, then discards the struct after the callback
/// creates a durable session.
#[derive(serde::Deserialize)]
struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn oauth_callback(
    State(state): State<SharedState>,
    axum::extract::Query(query): axum::extract::Query<OAuthCallbackQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let Some(oauth) = &state.oauth else {
        return Err(ApiError::bad_request("GitHub OAuth is not configured"));
    };

    let cookie_state = cookie_value(&headers, OAUTH_STATE_COOKIE);
    let valid = match (&query.code, &query.state, &cookie_state) {
        (Some(_), Some(query_state), Some(cookie_state)) if query_state == cookie_state => {
            state.store.consume_oauth_state(query_state).await?
        }
        _ => false,
    };
    if !valid {
        tracing::warn!(
            operation = "console.auth.oauth_callback",
            valid = false,
            has_code = query.code.is_some(),
            has_query_state = query.state.is_some(),
            has_cookie_state = cookie_state.is_some(),
            "console GitHub OAuth callback rejected invalid state"
        );
        return Err(ApiError::bad_request("invalid GitHub OAuth state"));
    }

    let code = query.code.expect("validated above");
    let token = github::exchange_github_code(&oauth.client_id, &oauth.client_secret, &code)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let viewer =
        state.github.viewer(&token).await.map_err(|err| {
            ApiError::bad_request(github::github_error_message(&err, "Signing in"))
        })?;
    let principal_id = format!("github:{}", viewer.id);
    let session_token = state
        .store
        .create_session(NewSession {
            identity: ActorIdentity::GitHub {
                id: viewer.id.to_string(),
                login: viewer.login,
                name: viewer.name,
                avatar_url: viewer.avatar_url,
            },
            github_token: token,
        })
        .await?;
    tracing::info!(
        operation = "console.auth.oauth_callback",
        valid = true,
        principal_id = %principal_id,
        "console GitHub OAuth session created"
    );

    let mut response = Redirect::temporary(&format!("{}/app", state.public_url)).into_response();
    let cookies = response.headers_mut();
    cookies.append(
        header::SET_COOKIE,
        set_cookie(SESSION_COOKIE, &session_token, state.secure_cookies, None)
            .parse()
            .expect("cookie value is valid"),
    );
    cookies.append(
        header::SET_COOKIE,
        set_cookie(OAUTH_STATE_COOKIE, "", state.secure_cookies, Some(0))
            .parse()
            .expect("cookie value is valid"),
    );
    Ok(response)
}

async fn device_start(State(state): State<SharedState>) -> ApiResult<Json<JsonValue>> {
    let Some(local) = state.local.as_ref() else {
        return Err(ApiError::bad_request(
            "device flow only applies to local-mode consoles",
        ));
    };
    let Some(client_id) = local.device_client_id() else {
        return Err(ApiError::bad_request(
            "device flow is not configured; set ROTOTO_GITHUB_CLIENT_ID or supply a token via ROTOTO_WORKSPACE_TOKEN",
        ));
    };
    let device = github::start_device_flow(client_id)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let response = json!({
        "userCode": device.user_code,
        "verificationUri": device.verification_uri,
        "intervalSeconds": device.interval_seconds,
        "expiresInSeconds": device.expires_in_seconds,
    });
    *local.device_flow.lock().await = Some(super::auth::DeviceFlowState {
        device_code: device.device_code,
    });
    Ok(Json(response))
}

async fn device_poll(State(state): State<SharedState>) -> ApiResult<Json<JsonValue>> {
    let Some(local) = state.local.as_ref() else {
        return Err(ApiError::bad_request(
            "device flow only applies to local-mode consoles",
        ));
    };
    let Some(client_id) = local.device_client_id().map(str::to_owned) else {
        return Err(ApiError::bad_request("device flow is not configured"));
    };
    let device_code = {
        let device_flow = local.device_flow.lock().await;
        let Some(device) = device_flow.as_ref() else {
            return Err(ApiError::bad_request("no device flow in progress"));
        };
        device.device_code.clone()
    };
    match github::poll_device_flow(&client_id, &device_code)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?
    {
        github::DevicePoll::Pending => Ok(Json(json!({ "status": "pending" }))),
        github::DevicePoll::SlowDown => Ok(Json(json!({ "status": "slow-down" }))),
        github::DevicePoll::Token(token) => {
            local.set_device_token(token).await?;
            *local.device_flow.lock().await = None;
            Ok(Json(json!({ "status": "authorized" })))
        }
        github::DevicePoll::Failed(message) => {
            *local.device_flow.lock().await = None;
            Err(ApiError::bad_request(message))
        }
    }
}

async fn console_data(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let fixed_source = fixed_source_scope(&state, &user.principal_id).await?;
    let (source_trees, workspaces) = match fixed_source.as_ref() {
        Some(source_tree) => (vec![source_tree.clone()], source_tree.workspaces.clone()),
        None => (
            state
                .store
                .list_source_trees_for_user(&user.principal_id)
                .await?,
            state
                .store
                .list_workspaces_for_user(&user.principal_id)
                .await?,
        ),
    };
    let mut branches = state
        .store
        .list_active_branches_with_workspaces_for_user(&user.principal_id)
        .await?;
    if let Some(source_tree) = fixed_source.as_ref() {
        branches = fixed_source_branch_filter(branches, source_tree);
    }
    Ok(Json(json!({
        "state": console_state_json(&state),
        "sourceTrees": source_trees,
        "workspaces": workspaces,
        "branches": branches,
    })))
}

async fn source_trees_list(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let source_trees = match fixed_source_scope(&state, &user.principal_id).await? {
        Some(source_tree) => vec![source_tree],
        None => {
            state
                .store
                .list_source_trees_for_user(&user.principal_id)
                .await?
        }
    };
    Ok(Json(json!({ "sourceTrees": source_trees })))
}

/// Source tree registration request body from the console form.
///
/// It exists to keep user input distinct from a verified GitHub repository.
/// The route trims and validates it, discovers workspaces, then persists the
/// resulting source tree/workspace records through `Store`.
#[derive(serde::Deserialize)]
struct RegisterSourceTreeBody {
    #[serde(rename = "sourceTree")]
    source_tree: Option<String>,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
}

async fn source_trees_register(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<RegisterSourceTreeBody>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    source_tree_management_allowed(&state)?;
    let source_tree = body
        .source_tree
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("configuration source is required"))?;
    if should_register_as_github(source_tree).await {
        tracing::info!(
            operation = "source_tree.register",
            principal_id = %user.principal_id,
            backend = "github",
            requested_ref = ?body.git_ref.as_deref(),
            "console source tree registration selected GitHub backend"
        );
        return register_github_source_tree(state, user, source_tree, body.git_ref).await;
    }
    tracing::info!(
        operation = "source_tree.register",
        principal_id = %user.principal_id,
        backend = "read_only",
        requested_ref = ?body.git_ref.as_deref(),
        "console source tree registration selected read-only backend"
    );
    register_read_only_source_tree(state, user, source_tree, body.git_ref).await
}

async fn register_github_source_tree(
    state: SharedState,
    user: super::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<Json<JsonValue>> {
    let (stored, token) = upsert_github_source_tree(&state, &user, source_tree, git_ref).await?;
    warm_registered_workspaces(
        state.clone(),
        user.principal_id.clone(),
        token,
        stored.workspaces.clone(),
    );
    Ok(Json(json!({ "sourceTree": stored })))
}

async fn upsert_github_source_tree(
    state: &SharedState,
    user: &super::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<(super::store::SourceTreeWithWorkspaces, String)> {
    let token = require_github_token(user, "Registering the configuration source")?;
    let (owner, name) = github::parse_repo_spec(source_tree)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let github_repo = state
        .github
        .repo(token, &owner, &name)
        .await
        .map_err(|err| ApiError::github(&err, "Registering the configuration source"))?;
    let requested_ref = git_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| source_tree_ref_hint(source_tree));
    let git_ref = requested_ref.unwrap_or_else(|| github_repo.default_branch.clone());
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        kind = "github",
        repository = %format!("{}/{}", github_repo.owner.login, github_repo.name),
        git_ref = %git_ref,
        "console source tree workspace discovery starting"
    );
    let workspaces = state
        .github
        .discover_workspaces(token, &owner, &name, &git_ref)
        .await
        .map_err(|err| ApiError::github(&err, "Discovering workspaces"))?;
    let stored = state
        .store
        .upsert_source_tree_with_workspaces(super::store::RegisterSourceTreeInput {
            principal_id: user.principal_id.clone(),
            kind: super::store::SourceTreeKind::GitHub,
            source: format!(
                "git+https://github.com/{}/{}.git#{}",
                github_repo.owner.login, github_repo.name, git_ref
            ),
            display_name: format!("{}/{}", github_repo.owner.login, github_repo.name),
            default_revision: git_ref.clone(),
            workspaces: workspaces
                .into_iter()
                .map(|workspace| super::store::DiscoveredWorkspaceInput {
                    path: workspace.path,
                    revision: workspace.git_ref,
                    source: workspace.source,
                })
                .collect(),
        })
        .await?;
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        source_tree_id = %stored.source_tree.id,
        kind = "github",
        workspaces = stored.workspaces.len(),
        "console source tree upserted"
    );
    Ok((stored, token.to_owned()))
}

async fn register_read_only_source_tree(
    state: SharedState,
    user: super::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<Json<JsonValue>> {
    let (stored, token) = upsert_read_only_source_tree(&state, &user, source_tree, git_ref).await?;
    warm_registered_workspaces(
        state.clone(),
        user.principal_id.clone(),
        token,
        stored.workspaces.clone(),
    );
    Ok(Json(json!({ "sourceTree": stored })))
}

async fn upsert_read_only_source_tree(
    state: &SharedState,
    user: &super::store::SessionUser,
    source_tree: &str,
    git_ref: Option<String>,
) -> ApiResult<(super::store::SourceTreeWithWorkspaces, String)> {
    let source = read_only_registration_source(source_tree, git_ref.as_deref())?;
    let registration = super::fixed_workspace::registration(&source)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        kind = ?registration.kind,
        workspaces = registration.workspaces.len(),
        "console read-only source tree registration resolved"
    );
    let stored = state
        .store
        .upsert_source_tree_with_workspaces(super::store::RegisterSourceTreeInput {
            principal_id: user.principal_id.clone(),
            kind: registration.kind,
            source: registration.source,
            display_name: registration.display_name,
            default_revision: registration.default_revision,
            workspaces: registration.workspaces,
        })
        .await?;
    tracing::info!(
        operation = "source_tree.upsert",
        principal_id = %user.principal_id,
        source_tree_id = %stored.source_tree.id,
        kind = ?stored.source_tree.kind,
        workspaces = stored.workspaces.len(),
        "console source tree upserted"
    );
    Ok((stored, source_token(user).to_owned()))
}

async fn should_register_as_github(source_tree: &str) -> bool {
    if source_tree.starts_with("file://")
        || source_tree.starts_with("git+file://")
        || source_tree.starts_with('/')
        || source_tree.starts_with('.')
        || source_tree.starts_with('~')
    {
        return false;
    }
    if tokio::fs::metadata(source_tree).await.is_ok() {
        return false;
    }
    github::parse_repo_spec(source_tree).is_ok()
}

fn source_tree_ref_hint(source_tree: &str) -> Option<String> {
    let git_ref = if let Some(fragment) = source_tree.split_once('#').map(|(_, fragment)| fragment)
    {
        fragment
            .split_once(':')
            .map(|(git_ref, _)| git_ref)
            .unwrap_or(fragment)
            .trim()
    } else if let Some(rest) = source_tree.strip_prefix("https://api.github.com/repos/") {
        rest.split('/').nth(3).unwrap_or("").trim()
    } else {
        ""
    };
    (!git_ref.is_empty()).then(|| git_ref.to_owned())
}

fn read_only_registration_source(source_tree: &str, git_ref: Option<&str>) -> ApiResult<String> {
    let source = source_tree.trim();
    let git_ref = git_ref.map(str::trim).filter(|value| !value.is_empty());
    if let Some(git_ref) = git_ref {
        if source.starts_with("git+") && !source.contains('#') {
            return Ok(format!("{source}#{git_ref}"));
        }
        if !source.starts_with("git+") {
            return Err(ApiError::bad_request(
                "ref only applies to GitHub or git configuration sources",
            ));
        }
    }
    Ok(source.to_owned())
}

fn warm_registered_workspaces(
    state: SharedState,
    principal_id: String,
    token: String,
    workspaces: Vec<super::store::WorkspaceRecord>,
) {
    if workspaces.is_empty() {
        return;
    }

    tokio::spawn(async move {
        for workspace in workspaces {
            let started = Instant::now();
            match super::workspace_source::semantic_workspace_for_base(
                &state,
                &principal_id,
                &token,
                &workspace,
            )
            .await
            {
                Ok(_) => {
                    tracing::debug!(
                        operation = "workspace.warm",
                        workspace_id = %workspace.id,
                        source = %workspace.source,
                        latency_ms = started.elapsed().as_millis(),
                        "console workspace warm-up completed"
                    );
                }
                Err(err) => {
                    tracing::debug!(
                        operation = "workspace.warm",
                        workspace_id = %workspace.id,
                        source = %workspace.source,
                        error = %err.message,
                        latency_ms = started.elapsed().as_millis(),
                        "console workspace warm-up failed"
                    );
                }
            }
        }
    });
}

async fn source_tree_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::extract::Path(source_tree_id): axum::extract::Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    source_tree_management_allowed(&state)?;
    let removed = state
        .store
        .delete_source_tree_for_user(&source_tree_id, &user.principal_id)
        .await?;
    if !removed {
        return Err(ApiError::not_found("configuration source not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn source_tree_refresh(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::extract::Path(source_tree_id): axum::extract::Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let stored = refresh_source_tree_for_user(&state, &user, &source_tree_id).await?;
    Ok(Json(json!({ "sourceTree": stored })))
}

async fn refresh_source_tree_for_user(
    state: &SharedState,
    user: &super::store::SessionUser,
    source_tree_id: &str,
) -> ApiResult<super::store::SourceTreeWithWorkspaces> {
    let fixed_source = fixed_source_scope(state, &user.principal_id).await?;
    if let Some(source_tree) = fixed_source.as_ref()
        && source_tree.source_tree.id != source_tree_id
    {
        return Err(ApiError::not_found("configuration source not found"));
    }
    let existing = state
        .store
        .get_source_tree_for_user(source_tree_id, &user.principal_id)
        .await?
        .ok_or_else(|| ApiError::not_found("configuration source not found"))?;
    let source_tree = existing.source_tree;
    tracing::info!(
        operation = "source_tree.refresh",
        principal_id = %user.principal_id,
        source_tree_id,
        kind = ?source_tree.kind,
        "console source tree refresh selected backend"
    );
    let (stored, token) = match source_tree.kind {
        super::store::SourceTreeKind::GitHub => {
            upsert_github_source_tree(
                state,
                user,
                &source_tree.source,
                Some(source_tree.default_revision.clone()),
            )
            .await?
        }
        super::store::SourceTreeKind::GitRemote => {
            upsert_read_only_source_tree(
                state,
                user,
                &source_tree.source,
                Some(source_tree.default_revision.clone()),
            )
            .await?
        }
        super::store::SourceTreeKind::LocalFolder | super::store::SourceTreeKind::Archive => {
            upsert_read_only_source_tree(state, user, &source_tree.source, None).await?
        }
    };
    warm_registered_workspaces(
        state.clone(),
        user.principal_id.clone(),
        token,
        stored.workspaces.clone(),
    );
    Ok(stored)
}

pub fn random_token(bytes: usize) -> ApiResult<String> {
    let mut buffer = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut buffer)
        .map_err(|_| ApiError::internal("failed to generate a random token"))?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

pub fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::console::identity::ActorIdentity;
    use crate::console::token_crypto::TokenCrypto;
    use tempfile::TempDir;

    #[test]
    fn observability_route_extracts_workspace_and_branch_ids() {
        let route = route_observability("/api/workspaces/ws-1/branches/branch-2/lsp");

        assert_eq!(
            route.pattern,
            "/api/workspaces/:workspace_id/branches/:branch_id/lsp"
        );
        assert_eq!(route.workspace_id.as_deref(), Some("ws-1"));
        assert_eq!(route.branch_id.as_deref(), Some("branch-2"));
    }

    #[test]
    fn observability_route_replaces_source_tree_ids() {
        let route = route_observability("/api/source-trees/tree-1");

        assert_eq!(route.pattern, "/api/source-trees/:source_tree_id");
        assert_eq!(route.workspace_id, None);
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
    async fn source_tree_refresh_rediscovers_local_workspace_paths() {
        let tree = TempDir::new().expect("source tree tempdir");
        write_workspace(tree.path()).await;
        let state = test_state();
        let user = test_user();
        let source = tree.path().to_str().expect("utf8 temp path");
        let (registered, _) =
            expect_api_ok(upsert_read_only_source_tree(&state, &user, source, None).await);
        assert_eq!(workspace_paths(&registered.workspaces), vec!["."]);

        write_workspace(&tree.path().join("workspaces/payments")).await;
        let refreshed = expect_api_ok(
            refresh_source_tree_for_user(&state, &user, &registered.source_tree.id).await,
        );

        assert_eq!(
            workspace_paths(&refreshed.workspaces),
            vec![".", "workspaces/payments"]
        );
        assert!(refreshed.source_tree.last_discovered_at.is_some());
    }

    #[tokio::test]
    async fn fixed_workspace_scope_rejects_stale_workspace_ids() {
        let fixed = TempDir::new().expect("fixed source tempdir");
        write_workspace(fixed.path()).await;
        let stale = TempDir::new().expect("stale source tempdir");
        write_workspace(stale.path()).await;
        let fixed_source = fixed.path().to_str().expect("utf8 temp path");
        let stale_source = stale.path().to_str().expect("utf8 temp path");
        let state = test_state_with_fixed_source(fixed_source);
        let user = test_user();

        let (stale_registered, _) =
            expect_api_ok(upsert_read_only_source_tree(&state, &user, stale_source, None).await);
        let stale_workspace = stale_registered
            .workspaces
            .first()
            .expect("stale workspace should be discovered");

        let err = super::super::api_workspace::load_workspace(&state, &user, &stale_workspace.id)
            .await
            .expect_err("fixed source should hide stale workspace rows");

        assert_eq!(err.status, StatusCode::NOT_FOUND);
        let fixed_registered = expect_api_ok(fixed_source_scope(&state, &user.principal_id).await)
            .expect("fixed source should register");
        assert_eq!(fixed_registered.source_tree.source, fixed_source);
        assert_eq!(workspace_paths(&fixed_registered.workspaces), vec!["."]);
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

    fn test_state_with_fixed_source_option(fixed_workspace_source: Option<String>) -> SharedState {
        let fixed_workspace = fixed_workspace_source.is_some();
        Arc::new(ConsoleState {
            deployment: DeploymentType::Local,
            oauth: None,
            state_mode: ConsoleStateMode::Ephemeral,
            write_policy: WritePolicy::Disabled,
            fixed_workspace_source,
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
                    fixed_workspace,
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

    async fn write_workspace(path: &std::path::Path) {
        tokio::fs::create_dir_all(path).await.unwrap();
        tokio::fs::write(path.join("rototo-workspace.toml"), "schema_version = 1\n")
            .await
            .unwrap();
    }

    fn workspace_paths(workspaces: &[super::super::store::WorkspaceRecord]) -> Vec<&str> {
        workspaces
            .iter()
            .map(|workspace| workspace.path.as_str())
            .collect()
    }
}
