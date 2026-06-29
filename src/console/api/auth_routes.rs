use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value as JsonValue, json};

use crate::console::auth::{
    GITHUB_OAUTH_SCOPES, GitHubCredentialSource, OAUTH_STATE_COOKIE, SESSION_COOKIE, cookie_value,
    session_from_headers, set_cookie,
};
use crate::console::capabilities::DeploymentType;
use crate::console::github;
use crate::console::identity::{ActorIdentity, resolve_git_config_identity};
use crate::console::local_git;
use crate::console::store::{NewSession, SessionUser};

use super::{ApiError, ApiResult, SharedState};

pub(super) async fn me(State(state): State<SharedState>, headers: HeaderMap) -> Response {
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
                        .fixed_package_source
                        .as_deref()
                        .and_then(|source| local_git::package_root(source).ok());
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
                        .fixed_package_source
                        .as_deref()
                        .and_then(|source| local_git::package_root(source).ok());
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

pub(super) async fn logout(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Response> {
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

pub(super) async fn oauth_start(State(state): State<SharedState>) -> ApiResult<Response> {
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
pub(super) struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

pub(super) async fn oauth_callback(
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

pub(super) async fn device_start(State(state): State<SharedState>) -> ApiResult<Json<JsonValue>> {
    let Some(local) = state.local.as_ref() else {
        return Err(ApiError::bad_request(
            "device flow only applies to local-mode consoles",
        ));
    };
    let Some(client_id) = local.device_client_id() else {
        return Err(ApiError::bad_request(
            "device flow is not configured; set ROTOTO_GITHUB_CLIENT_ID or supply a token via ROTOTO_PACKAGE_TOKEN",
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
    *local.device_flow.lock().await = Some(crate::console::auth::DeviceFlowState {
        device_code: device.device_code,
    });
    Ok(Json(response))
}

pub(super) async fn device_poll(State(state): State<SharedState>) -> ApiResult<Json<JsonValue>> {
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

fn random_token(bytes: usize) -> ApiResult<String> {
    let mut buffer = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut buffer)
        .map_err(|_| ApiError::internal("failed to generate a random token"))?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

fn url_encode(value: &str) -> String {
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
