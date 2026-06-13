use std::sync::Arc;

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
    AuthMode, GITHUB_OAUTH_SCOPES, LocalAuth, OAUTH_STATE_COOKIE, SESSION_COOKIE, cookie_value,
    session_from_headers, set_cookie,
};
use super::github::{self, GitHubClient, GitHubError};
use super::lsp::LspSessions;
use super::stage::StageCache;
use super::store::{NewSession, SessionUser, Store};

pub struct ConsoleState {
    pub mode: AuthMode,
    pub store: Store,
    pub github: GitHubClient,
    pub stage: StageCache,
    pub lsp: LspSessions,
    pub local: Option<LocalAuth>,
    /// Origin used for OAuth redirects and cookies, e.g. http://127.0.0.1:7686.
    pub public_url: String,
    pub allowed_origins: Vec<String>,
    pub secure_cookies: bool,
    /// The synthetic user id read-only deployments register workspaces under.
    pub read_only_user_id: String,
}

pub type SharedState = Arc<ConsoleState>;

/// Errors become the same `{ "error": message }` envelope the admin app
/// produced, with the status the route used.
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

pub type ApiResult<T> = std::result::Result<T, ApiError>;

pub fn router(state: SharedState) -> axum::Router {
    let api = axum::Router::new()
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

    axum::Router::new()
        .nest("/api", api)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            request_guard,
        ))
        .with_state(state)
}

/// Mutation guard: read-only deployments reject writes, and cross-site
/// requests are blocked by requiring a custom header plus an Origin check.
/// Custom headers cannot be attached cross-site without a CORS preflight,
/// which this server never grants.
async fn request_guard(State(state): State<SharedState>, request: Request, next: Next) -> Response {
    let mutating = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if mutating && request.uri().path().starts_with("/api") {
        if state.mode == AuthMode::ReadOnly {
            return ApiError {
                status: StatusCode::FORBIDDEN,
                message: "this console is read-only".to_owned(),
            }
            .into_response();
        }
        let headers = request.headers();
        if !headers.contains_key("x-rototo-console") {
            return ApiError {
                status: StatusCode::FORBIDDEN,
                message: "missing x-rototo-console request header".to_owned(),
            }
            .into_response();
        }
        if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
            && !state
                .allowed_origins
                .iter()
                .any(|allowed| allowed == origin)
        {
            return ApiError {
                status: StatusCode::FORBIDDEN,
                message: format!("origin {origin} is not allowed"),
            }
            .into_response();
        }
    }
    next.run(request).await
}

/// The authenticated user for a request, per auth mode.
pub async fn require_user(state: &ConsoleState, headers: &HeaderMap) -> ApiResult<SessionUser> {
    match &state.mode {
        AuthMode::Team { .. } => session_from_headers(&state.store, headers)
            .await
            .ok_or_else(ApiError::unauthorized),
        AuthMode::Local => {
            let local = state.local.as_ref().expect("local mode has local auth");
            local
                .identity(&state.github)
                .await
                .map_err(|err| ApiError {
                    status: StatusCode::UNAUTHORIZED,
                    message: err.to_string(),
                })?
                .ok_or_else(ApiError::unauthorized)
        }
        AuthMode::ReadOnly => Ok(read_only_user(state)),
    }
}

pub fn read_only_user(state: &ConsoleState) -> SessionUser {
    SessionUser {
        session_hash: "read-only".to_owned(),
        github_user_id: state.read_only_user_id.clone(),
        github_login: "read-only".to_owned(),
        github_name: None,
        github_avatar_url: None,
        github_token: String::new(),
    }
}

async fn me(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let mode = state.mode.label();
    let device_flow = state
        .local
        .as_ref()
        .map(|local| local.device_flow_available())
        .unwrap_or(false);
    let token_source = match state.local.as_ref() {
        Some(local) => local.token().await.map(|ambient| ambient.source),
        None => None,
    };

    let (status, auth_error, user) = match &state.mode {
        AuthMode::Team { .. } => match session_from_headers(&state.store, &headers).await {
            Some(user) => (StatusCode::OK, None, Some(user)),
            None => (StatusCode::UNAUTHORIZED, None, None),
        },
        AuthMode::Local => {
            let local = state.local.as_ref().expect("local mode has local auth");
            match local.identity(&state.github).await {
                Ok(Some(user)) => (StatusCode::OK, None, Some(user)),
                Ok(None) => (StatusCode::OK, None, None),
                Err(err) => (StatusCode::OK, Some(err.to_string()), None),
            }
        }
        AuthMode::ReadOnly => (StatusCode::OK, None, Some(read_only_user(&state))),
    };

    let user_json = user.map(|user| {
        json!({
            "githubUserId": user.github_user_id,
            "githubLogin": user.github_login,
            "githubName": user.github_name,
            "githubAvatarUrl": user.github_avatar_url,
        })
    });
    (
        status,
        Json(json!({
            "mode": mode,
            "deviceFlow": device_flow,
            "tokenSource": token_source,
            "authError": auth_error,
            "user": user_json,
        })),
    )
        .into_response()
}

async fn logout(State(state): State<SharedState>, headers: HeaderMap) -> ApiResult<Response> {
    if !matches!(state.mode, AuthMode::Team { .. }) {
        return Err(ApiError::bad_request(
            "logout only applies to team-mode consoles",
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
    let AuthMode::Team { client_id, .. } = &state.mode else {
        return Err(ApiError::internal(
            "GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET are required",
        ));
    };

    let state_token = random_token(24)?;
    state.store.create_oauth_state(&state_token).await?;

    let redirect_uri = format!("{}/api/auth/github/callback", state.public_url);
    let authorize_url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}",
        url_encode(client_id),
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
    let AuthMode::Team {
        client_id,
        client_secret,
    } = &state.mode
    else {
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
    let token = github::exchange_github_code(client_id, client_secret, &code)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let viewer =
        state.github.viewer(&token).await.map_err(|err| {
            ApiError::bad_request(github::github_error_message(&err, "Signing in"))
        })?;
    let session_token = state
        .store
        .create_session(NewSession {
            github_user_id: viewer.id.to_string(),
            github_login: viewer.login,
            github_name: viewer.name,
            github_avatar_url: viewer.avatar_url,
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
    let repos = state
        .store
        .list_repos_for_user(&user.github_user_id)
        .await?;
    let workspaces = state
        .store
        .list_workspaces_for_user(&user.github_user_id)
        .await?;
    let mut drafts = Vec::new();
    for workspace in &workspaces {
        for draft in state
            .store
            .list_draft_sessions_for_workspace(&workspace.id, &user.github_user_id)
            .await?
        {
            drafts.push(json!({ "draft": draft, "workspace": workspace }));
        }
    }
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
    let repos = state
        .store
        .list_repos_for_user(&user.github_user_id)
        .await?;
    Ok(Json(json!({ "repos": repos })))
}

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
    let (owner, name) = github::parse_repo_spec(body.repo.as_deref().unwrap_or(""))
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let repo = state
        .github
        .repo(&user.github_token, &owner, &name)
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
        .discover_workspaces(&user.github_token, &owner, &name, &git_ref)
        .await
        .map_err(|err| ApiError::github(&err, "Discovering workspaces"))?;
    let stored = state
        .store
        .upsert_repo_with_workspaces(
            user.github_user_id.clone(),
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
    Ok(Json(json!({ "repo": stored })))
}

async fn repo_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::extract::Path(repo_id): axum::extract::Path<String>,
) -> ApiResult<Json<JsonValue>> {
    let user = require_user(&state, &headers).await?;
    let removed = state
        .store
        .delete_repo_for_user(&repo_id, &user.github_user_id)
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
