use serde_json::Value as JsonValue;

/// A GitHub API failure that keeps the response so callers can shape
/// user-facing messages the way the admin app did.
///
/// The client creates it for non-success HTTP responses. It is not persisted;
/// route code either extracts a clearer message or truncates the response text
/// before returning an API error.
#[derive(Debug)]
pub struct GitHubApiError {
    pub status: u16,
    pub response_text: String,
}

impl GitHubApiError {
    pub fn response_message(&self) -> Option<String> {
        let body: JsonValue = serde_json::from_str(&self.response_text).ok()?;
        body.get("message")
            .and_then(JsonValue::as_str)
            .map(str::to_owned)
    }

    pub fn message(&self) -> String {
        let mut text = self.response_text.clone();
        text.truncate(300);
        format!("GitHub API {}: {}", self.status, text)
    }
}

/// Error type for GitHub client operations.
///
/// API failures preserve status and response text for user-facing guidance;
/// local transport, parsing, and validation failures become `Other`. Values
/// live for one failed operation and are converted to `ApiError` at route
/// boundaries.
pub enum GitHubError {
    Api(GitHubApiError),
    Other(String),
}

impl GitHubError {
    pub(super) fn other(err: impl std::fmt::Display) -> Self {
        Self::Other(err.to_string())
    }
}

/// Result alias for GitHub REST helpers.
///
/// It keeps GitHub-specific failures inside the client layer until a route can
/// add action context for the browser-facing message.
pub type GitHubResult<T> = std::result::Result<T, GitHubError>;

/// User-facing message shaping, including the 403 "Resource not accessible by
/// integration" case that needs OAuth-credential guidance.
pub fn github_error_message(error: &GitHubError, action: &str) -> String {
    match error {
        GitHubError::Api(api) => {
            if api.status == 403
                && api.response_message().as_deref()
                    == Some("Resource not accessible by integration")
            {
                return format!(
                    "{action} failed because the GitHub credential cannot write to this repository. \
                     Use GitHub OAuth App credentials, make sure the user has write access to the \
                     repository, then log out and sign in again so the token is authorized with the \
                     repo scope."
                );
            }
            api.message()
        }
        GitHubError::Other(message) => message.clone(),
    }
}
