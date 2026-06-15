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

use super::auth::{
    GITHUB_OAUTH_SCOPES, GitHubCredentialSource, HostedOAuth, LocalAuth, OAUTH_STATE_COOKIE,
    SESSION_COOKIE, cookie_value, session_from_headers, set_cookie,
};
use super::capabilities::{DeploymentType, WritePolicy};
use super::github::{self, GitHubClient, GitHubError};
use super::identity::{ActorIdentity, resolve_git_config_identity};
use super::local_git;
use super::lsp::LspSessions;
use super::observability::DevObservability;
use super::stage::StageCache;
use super::store::{NewSession, SessionUser, Store};

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
        .route("/repos", get(repos_list).post(repos_register))
        .route("/repos/{repo_id}", axum::routing::delete(repo_delete))
        .merge(super::api_workspace::routes())
        .merge(super::api_draft::routes());
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
    let mutating = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if mutating && request.uri().path().starts_with("/api") {
        let headers = request.headers();
        if !headers.contains_key("x-rototo-console") {
            let response = ApiError {
                status: StatusCode::FORBIDDEN,
                message: "missing x-rototo-console request header".to_owned(),
            }
            .into_response();
            record_api_request(&state, started, &method, &path, response.status()).await;
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
            record_api_request(&state, started, &method, &path, response.status()).await;
            return response;
        }
    }
    let response = next.run(request).await;
    record_api_request(&state, started, &method, &path, response.status()).await;
    response
}

async fn record_api_request(
    state: &ConsoleState,
    started: Instant,
    method: &Method,
    path: &str,
    status: StatusCode,
) {
    let Some(observability) = &state.observability else {
        return;
    };
    let route = route_observability(path);
    observability
        .record_api_request(json!({
            "method": method.as_str(),
            "path": path,
            "route": route.pattern,
            "status": status.as_u16(),
            "status_class": status_class(status),
            "latency_ms": started.elapsed().as_millis(),
            "deployment": state.deployment.label(),
            "workspace_id": route.workspace_id,
            "draft_id": route.draft_id,
            "error_class": error_class(status),
        }))
        .await;
}

/// Normalized request identity used by the development observability sink.
///
/// The middleware derives this from the raw path for one request so metrics can
/// group `/api/workspaces/<id>` style paths without storing every concrete id
/// as a separate route label.
struct RouteObservability {
    pattern: String,
    workspace_id: Option<String>,
    draft_id: Option<String>,
}

fn route_observability(path: &str) -> RouteObservability {
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut pattern = Vec::new();
    let mut workspace_id = None;
    let mut draft_id = None;
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
        if segment == "drafts" && index + 1 < segments.len() {
            draft_id = Some(segments[index + 1].to_owned());
            pattern.push(":draft_id".to_owned());
            index += 2;
            continue;
        }
        if segment == "repos" && index + 1 < segments.len() {
            pattern.push(":repo_id".to_owned());
            index += 2;
            continue;
        }
        index += 1;
    }
    RouteObservability {
        pattern: format!("/{}", pattern.join("/")),
        workspace_id,
        draft_id,
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
        DeploymentType::Hosted => session_from_headers(&state.store, headers)
            .await
            .ok_or_else(ApiError::unauthorized),
        DeploymentType::Local => {
            let local = state
                .local
                .as_ref()
                .expect("local deployment has local auth");
            match local.identity(&state.github).await {
                Ok(Some(user)) => Ok(user),
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

async fn dev_observability_event(
    State(state): State<SharedState>,
    Json(event): Json<JsonValue>,
) -> ApiResult<Json<JsonValue>> {
    let Some(observability) = &state.observability else {
        return Err(ApiError::not_found("dev observability is disabled"));
    };
    observability.record_ui_event(event).await;
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
        return Err(ApiError::bad_request(
            "logout only applies to hosted consoles",
        ));
    }
    if let Some(token) = cookie_value(&headers, SESSION_COOKIE) {
        state.store.delete_session(&token).await?;
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
    if let Some(source) = state.fixed_workspace_source.as_deref() {
        super::register_fixed_workspace(&state, &user.principal_id, source).await?;
    }
    let repos = state.store.list_repos_for_user(&user.principal_id).await?;
    let workspaces = state
        .store
        .list_workspaces_for_user(&user.principal_id)
        .await?;
    let drafts = state
        .store
        .list_draft_sessions_for_user(&user.principal_id)
        .await?;
    Ok(Json(json!({
        "repos": repos,
        "workspaces": workspaces,
        "drafts": drafts,
    })))
}

async fn repos_list(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let repos = state.store.list_repos_for_user(&user.principal_id).await?;
    Ok(Json(json!({ "repos": repos })))
}

/// Repository registration request body from the console form.
///
/// It exists to keep user input distinct from a verified GitHub repository.
/// The route trims and validates it, discovers workspaces, then persists the
/// resulting repo/workspace records through `Store`.
#[derive(serde::Deserialize)]
struct RegisterRepoBody {
    repo: Option<String>,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
}

async fn repos_register(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<RegisterRepoBody>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let token = require_github_token(&user, "Registering the repository")?;
    let (owner, name) = github::parse_repo_spec(body.repo.as_deref().unwrap_or(""))
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let repo = state
        .github
        .repo(token, &owner, &name)
        .await
        .map_err(|err| ApiError::github(&err, "Registering the repository"))?;
    let git_ref = body
        .git_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&repo.default_branch)
        .to_owned();
    let workspaces = state
        .github
        .discover_workspaces(token, &owner, &name, &git_ref)
        .await
        .map_err(|err| ApiError::github(&err, "Discovering workspaces"))?;
    let stored = state
        .store
        .upsert_repo_with_workspaces(
            user.principal_id.clone(),
            repo.owner.login,
            repo.name,
            git_ref,
            workspaces
                .into_iter()
                .map(|workspace| super::store::DiscoveredWorkspaceInput {
                    path: workspace.path,
                    git_ref: workspace.git_ref,
                    source: workspace.source,
                })
                .collect(),
        )
        .await?;
    warm_registered_workspaces(
        state.clone(),
        user.principal_id.clone(),
        token.to_owned(),
        stored.workspaces.clone(),
    );
    Ok(Json(json!({ "repo": stored })))
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

async fn repo_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::extract::Path(repo_id): axum::extract::Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let removed = state
        .store
        .delete_repo_for_user(&repo_id, &user.principal_id)
        .await?;
    if !removed {
        return Err(ApiError::not_found("repository not found"));
    }
    Ok(Json(json!({ "ok": true })))
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

    #[test]
    fn observability_route_extracts_workspace_and_draft_ids() {
        let route = route_observability("/api/workspaces/ws-1/drafts/draft-2/lsp");

        assert_eq!(
            route.pattern,
            "/api/workspaces/:workspace_id/drafts/:draft_id/lsp"
        );
        assert_eq!(route.workspace_id.as_deref(), Some("ws-1"));
        assert_eq!(route.draft_id.as_deref(), Some("draft-2"));
    }

    #[test]
    fn observability_route_replaces_repo_ids() {
        let route = route_observability("/api/repos/repo-1");

        assert_eq!(route.pattern, "/api/repos/:repo_id");
        assert_eq!(route.workspace_id, None);
        assert_eq!(route.draft_id, None);
    }
}
