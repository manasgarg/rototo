use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};

use crate::error::{Result, RototoError};

const GITHUB_USER_AGENT: &str = "rototo-console";
const GITHUB_API: &str = "https://api.github.com";

#[derive(Clone, Debug, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitHubRepo {
    pub name: String,
    pub owner: GitHubRepoOwner,
    pub default_branch: String,
    pub permissions: Option<GitHubRepoPermissions>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitHubRepoOwner {
    pub login: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct GitHubRepoPermissions {
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub push: bool,
}

#[derive(Clone, Debug)]
pub struct DiscoveredWorkspace {
    pub path: String,
    pub git_ref: String,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct GitHubContentFile {
    pub sha: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitHubPullRequest {
    pub html_url: String,
    pub number: i64,
    pub state: Option<String>,
    pub merged_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitHubTreeEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
}

#[derive(Clone, Debug)]
pub struct RefComparison {
    pub ahead_by: i64,
    pub files: Vec<String>,
}

/// A GitHub API failure that keeps the response so callers can shape
/// user-facing messages the way the admin app did.
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

pub enum GitHubError {
    Api(GitHubApiError),
    Other(String),
}

impl GitHubError {
    fn other(err: impl std::fmt::Display) -> Self {
        Self::Other(err.to_string())
    }
}

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

const REPO_SPEC_ERROR: &str = "repo must be owner/name or a GitHub repository URL";

pub fn parse_repo_spec(value: &str) -> Result<(String, String)> {
    let trimmed = value.trim();
    let mut candidate = strip_prefix_ignore_ascii_case(trimmed, "git@github.com:")
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "ssh://git@github.com/"))
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "https://github.com/"))
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "http://github.com/"))
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "github.com/"))
        .unwrap_or(trimmed);
    candidate = candidate
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let Some((owner, mut name)) = candidate.split_once('/') else {
        return Err(RototoError::new(REPO_SPEC_ERROR));
    };
    name = name.strip_suffix(".git").unwrap_or(name);
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    if !valid(owner) || !valid(name) || name.contains('/') {
        return Err(RototoError::new(REPO_SPEC_ERROR));
    }
    Ok((owner.to_owned(), name.to_owned()))
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        value.get(prefix.len()..)
    } else {
        None
    }
}

#[derive(Clone)]
pub struct GitHubClient {
    http: reqwest::Client,
}

impl GitHubClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    pub async fn viewer(&self, token: &str) -> GitHubResult<GitHubUser> {
        self.get(token, "/user").await
    }

    pub async fn repo(&self, token: &str, owner: &str, name: &str) -> GitHubResult<GitHubRepo> {
        self.get(token, &format!("/repos/{}/{}", enc(owner), enc(name)))
            .await
    }

    pub async fn assert_repo_write_access(
        &self,
        token: &str,
        owner: &str,
        name: &str,
    ) -> GitHubResult<()> {
        let repo = self.repo(token, owner, name).await?;
        if let Some(permissions) = repo.permissions
            && !permissions.push
            && !permissions.admin
        {
            return Err(GitHubError::Other(format!(
                "Your GitHub credential can read {owner}/{name}, but cannot push to it. Grant \
                 repository write access before editing this workspace."
            )));
        }
        Ok(())
    }

    pub async fn discover_workspaces(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        git_ref: &str,
    ) -> GitHubResult<Vec<DiscoveredWorkspace>> {
        let tree = self.tree(token, owner, name, git_ref).await?;
        let mut workspaces: Vec<DiscoveredWorkspace> = tree
            .into_iter()
            .filter(|entry| {
                entry.entry_type == "blob" && entry.path.ends_with("rototo-workspace.toml")
            })
            .map(|entry| {
                let path = manifest_workspace_path(&entry.path);
                DiscoveredWorkspace {
                    source: workspace_git_source(owner, name, git_ref, &path),
                    path,
                    git_ref: git_ref.to_owned(),
                }
            })
            .collect();
        workspaces.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(workspaces)
    }

    pub async fn branch_head_sha(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        branch: &str,
    ) -> GitHubResult<String> {
        #[derive(Deserialize)]
        struct RefResponse {
            object: RefObject,
        }
        #[derive(Deserialize)]
        struct RefObject {
            sha: String,
            #[serde(rename = "type")]
            object_type: String,
        }
        let reference: RefResponse = self
            .get(
                token,
                &format!(
                    "/repos/{}/{}/git/ref/{}",
                    enc(owner),
                    enc(name),
                    enc(&format!("heads/{branch}"))
                ),
            )
            .await?;
        if reference.object.object_type != "commit" {
            return Err(GitHubError::Other(format!(
                "GitHub ref {branch} does not point to a commit"
            )));
        }
        Ok(reference.object.sha)
    }

    pub async fn list_branches(
        &self,
        token: &str,
        owner: &str,
        name: &str,
    ) -> GitHubResult<Vec<String>> {
        #[derive(Deserialize)]
        struct Branch {
            name: String,
        }
        let branches: Vec<Branch> = self
            .get(
                token,
                &format!("/repos/{}/{}/branches?per_page=100", enc(owner), enc(name)),
            )
            .await?;
        Ok(branches.into_iter().map(|branch| branch.name).collect())
    }

    pub async fn compare_refs(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        base: &str,
        head: &str,
    ) -> GitHubResult<RefComparison> {
        #[derive(Deserialize)]
        struct Comparison {
            ahead_by: i64,
            #[serde(default)]
            files: Vec<ComparisonFile>,
        }
        #[derive(Deserialize)]
        struct ComparisonFile {
            filename: String,
        }
        let comparison: Comparison = self
            .get(
                token,
                &format!(
                    "/repos/{}/{}/compare/{}...{}",
                    enc(owner),
                    enc(name),
                    enc(base),
                    enc(head)
                ),
            )
            .await?;
        Ok(RefComparison {
            ahead_by: comparison.ahead_by,
            files: comparison
                .files
                .into_iter()
                .map(|file| file.filename)
                .collect(),
        })
    }

    pub async fn create_branch(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        branch: &str,
        sha: &str,
    ) -> GitHubResult<()> {
        let _: JsonValue = self
            .send(
                token,
                reqwest::Method::POST,
                &format!("/repos/{}/{}/git/refs", enc(owner), enc(name)),
                Some(json!({ "ref": format!("refs/heads/{branch}"), "sha": sha })),
            )
            .await?;
        Ok(())
    }

    pub async fn rename_branch(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        branch: &str,
        new_name: &str,
    ) -> GitHubResult<String> {
        #[derive(Deserialize)]
        struct Renamed {
            name: String,
        }
        let renamed: Renamed = self
            .send(
                token,
                reqwest::Method::POST,
                &format!(
                    "/repos/{}/{}/branches/{}/rename",
                    enc(owner),
                    enc(name),
                    enc(branch)
                ),
                Some(json!({ "new_name": new_name })),
            )
            .await?;
        Ok(renamed.name)
    }

    pub async fn file(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        path: &str,
        git_ref: &str,
    ) -> GitHubResult<GitHubContentFile> {
        #[derive(Deserialize)]
        struct Content {
            #[serde(rename = "type")]
            content_type: String,
            sha: String,
            encoding: Option<String>,
            content: Option<String>,
        }
        let file: Content = self
            .get(
                token,
                &format!(
                    "/repos/{}/{}/contents/{}?ref={}",
                    enc(owner),
                    enc(name),
                    encode_repo_path(path),
                    enc(git_ref)
                ),
            )
            .await?;
        let (Some(encoding), Some(content)) = (file.encoding, file.content) else {
            return Err(GitHubError::Other(format!(
                "GitHub path is not a readable file: {path}"
            )));
        };
        if file.content_type != "file" || encoding != "base64" {
            return Err(GitHubError::Other(format!(
                "GitHub path is not a readable file: {path}"
            )));
        }
        let bytes = BASE64
            .decode(content.replace('\n', ""))
            .map_err(GitHubError::other)?;
        Ok(GitHubContentFile {
            sha: file.sha,
            content: String::from_utf8(bytes).map_err(GitHubError::other)?,
        })
    }

    #[expect(clippy::too_many_arguments)]
    pub async fn update_file(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        path: &str,
        branch: &str,
        sha: &str,
        content: &str,
        message: &str,
    ) -> GitHubResult<()> {
        let _: JsonValue = self
            .send(
                token,
                reqwest::Method::PUT,
                &format!(
                    "/repos/{}/{}/contents/{}",
                    enc(owner),
                    enc(name),
                    encode_repo_path(path)
                ),
                Some(json!({
                    "message": message,
                    "content": BASE64.encode(content.as_bytes()),
                    "sha": sha,
                    "branch": branch,
                })),
            )
            .await?;
        Ok(())
    }

    #[expect(clippy::too_many_arguments)]
    pub async fn create_file(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        path: &str,
        branch: &str,
        content: &str,
        message: &str,
    ) -> GitHubResult<()> {
        let _: JsonValue = self
            .send(
                token,
                reqwest::Method::PUT,
                &format!(
                    "/repos/{}/{}/contents/{}",
                    enc(owner),
                    enc(name),
                    encode_repo_path(path)
                ),
                Some(json!({
                    "message": message,
                    "content": BASE64.encode(content.as_bytes()),
                    "branch": branch,
                })),
            )
            .await?;
        Ok(())
    }

    #[expect(clippy::too_many_arguments)]
    pub async fn delete_file(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        path: &str,
        branch: &str,
        sha: &str,
        message: &str,
    ) -> GitHubResult<()> {
        let _: JsonValue = self
            .send(
                token,
                reqwest::Method::DELETE,
                &format!(
                    "/repos/{}/{}/contents/{}",
                    enc(owner),
                    enc(name),
                    encode_repo_path(path)
                ),
                Some(json!({ "message": message, "sha": sha, "branch": branch })),
            )
            .await?;
        Ok(())
    }

    pub async fn tree(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        git_ref: &str,
    ) -> GitHubResult<Vec<GitHubTreeEntry>> {
        #[derive(Deserialize)]
        struct TreeResponse {
            truncated: bool,
            tree: Vec<GitHubTreeEntry>,
        }
        let tree: TreeResponse = self
            .get(
                token,
                &format!(
                    "/repos/{}/{}/git/trees/{}?recursive=1",
                    enc(owner),
                    enc(name),
                    enc(git_ref)
                ),
            )
            .await?;
        if tree.truncated {
            return Err(GitHubError::Other(
                "GitHub tree response was truncated".to_owned(),
            ));
        }
        Ok(tree.tree)
    }

    #[expect(clippy::too_many_arguments)]
    pub async fn create_pull_request(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
    ) -> GitHubResult<GitHubPullRequest> {
        self.send(
            token,
            reqwest::Method::POST,
            &format!("/repos/{}/{}/pulls", enc(owner), enc(name)),
            Some(json!({
                "title": title,
                "body": body,
                "head": head,
                "base": base,
                "maintainer_can_modify": true,
            })),
        )
        .await
    }

    pub async fn pull_request(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        number: i64,
    ) -> GitHubResult<GitHubPullRequest> {
        self.get(
            token,
            &format!("/repos/{}/{}/pulls/{number}", enc(owner), enc(name)),
        )
        .await
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
    ) -> GitHubResult<T> {
        self.send(token, reqwest::Method::GET, path, None).await
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<JsonValue>,
    ) -> GitHubResult<T> {
        let started = std::time::Instant::now();
        let method_label = method.as_str().to_owned();
        let mut request = self
            .http
            .request(method, format!("{GITHUB_API}{path}"))
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("User-Agent", GITHUB_USER_AGENT)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(GitHubError::other)?;
        let status = response.status();
        let text = response.text().await.map_err(GitHubError::other)?;
        tracing::info!(
            operation = "github.rest",
            method = %method_label,
            path = %github_path_pattern(path),
            status = status.as_u16(),
            latency_ms = started.elapsed().as_millis(),
            "console GitHub REST call completed"
        );
        if !status.is_success() {
            return Err(GitHubError::Api(GitHubApiError {
                status: status.as_u16(),
                response_text: text,
            }));
        }
        serde_json::from_str(&text).map_err(GitHubError::other)
    }
}

fn github_path_pattern(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.chars().all(|ch| ch.is_ascii_digit()) {
                ":number"
            } else if segment.len() >= 32 && segment.chars().all(|ch| ch.is_ascii_hexdigit()) {
                ":sha"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// GitHub OAuth web-flow code exchange.
pub async fn exchange_github_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<String> {
    #[derive(Deserialize)]
    struct Exchange {
        access_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let response = reqwest::Client::new()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .header("User-Agent", GITHUB_USER_AGENT)
        .json(&json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
        }))
        .send()
        .await
        .map_err(|err| RototoError::new(format!("GitHub OAuth exchange failed: {err}")))?;
    let ok = response.status().is_success();
    let body: Exchange = response
        .json()
        .await
        .map_err(|err| RototoError::new(format!("GitHub OAuth exchange failed: {err}")))?;
    match body.access_token {
        Some(token) if ok => Ok(token),
        _ => Err(RototoError::new(
            body.error_description
                .or(body.error)
                .unwrap_or_else(|| "GitHub OAuth failed".to_owned()),
        )),
    }
}

/// GitHub device-flow: request a user code, then poll for the token.
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
}

pub async fn start_device_flow(client_id: &str) -> Result<DeviceCode> {
    #[derive(Deserialize)]
    struct DeviceResponse {
        device_code: String,
        user_code: String,
        verification_uri: String,
        #[serde(default)]
        interval: u64,
        expires_in: u64,
    }
    let response = reqwest::Client::new()
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .header("User-Agent", GITHUB_USER_AGENT)
        .json(&json!({ "client_id": client_id, "scope": "read:user repo" }))
        .send()
        .await
        .map_err(|err| RototoError::new(format!("GitHub device flow start failed: {err}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(RototoError::new(format!(
            "GitHub device flow start failed: {status}: {text}"
        )));
    }
    let body: DeviceResponse = response
        .json()
        .await
        .map_err(|err| RototoError::new(format!("GitHub device flow start failed: {err}")))?;
    Ok(DeviceCode {
        device_code: body.device_code,
        user_code: body.user_code,
        verification_uri: body.verification_uri,
        interval_seconds: body.interval.max(5),
        expires_in_seconds: body.expires_in,
    })
}

pub enum DevicePoll {
    Pending,
    SlowDown,
    Token(String),
    Failed(String),
}

pub async fn poll_device_flow(client_id: &str, device_code: &str) -> Result<DevicePoll> {
    #[derive(Deserialize)]
    struct PollResponse {
        access_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let response = reqwest::Client::new()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .header("User-Agent", GITHUB_USER_AGENT)
        .json(&json!({
            "client_id": client_id,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await
        .map_err(|err| RototoError::new(format!("GitHub device flow poll failed: {err}")))?;
    let body: PollResponse = response
        .json()
        .await
        .map_err(|err| RototoError::new(format!("GitHub device flow poll failed: {err}")))?;
    if let Some(token) = body.access_token {
        return Ok(DevicePoll::Token(token));
    }
    Ok(match body.error.as_deref() {
        Some("authorization_pending") => DevicePoll::Pending,
        Some("slow_down") => DevicePoll::SlowDown,
        Some(error) => DevicePoll::Failed(
            body.error_description
                .unwrap_or_else(|| format!("GitHub device flow failed: {error}")),
        ),
        None => DevicePoll::Failed("GitHub device flow failed".to_owned()),
    })
}

pub fn workspace_archive_source(owner: &str, name: &str, git_ref: &str, path: &str) -> String {
    let archive = format!(
        "{GITHUB_API}/repos/{}/{}/tarball/{}",
        enc(owner),
        enc(name),
        enc(git_ref)
    );
    if path == "." {
        archive
    } else {
        format!("{archive}#:{path}")
    }
}

pub fn workspace_git_source(owner: &str, name: &str, git_ref: &str, path: &str) -> String {
    let remote = format!("git+https://github.com/{}/{}.git", enc(owner), enc(name));
    if path == "." {
        format!("{remote}#{git_ref}")
    } else {
        format!("{remote}#{git_ref}:{path}")
    }
}

pub fn stable_workspace_key(owner: &str, name: &str, path: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("{owner}/{name}:{path}").as_bytes(),
    );
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()[..12]
        .to_owned()
}

pub fn workspace_repo_path(workspace_path: &str, relative_path: &str) -> String {
    if workspace_path == "." {
        relative_path.to_owned()
    } else {
        format!("{workspace_path}/{relative_path}")
    }
}

pub fn encode_repo_path(path: &str) -> String {
    path.split('/').map(enc).collect::<Vec<_>>().join("/")
}

fn manifest_workspace_path(manifest_path: &str) -> String {
    let path = manifest_path
        .strip_suffix("/rototo-workspace.toml")
        .unwrap_or_else(|| {
            if manifest_path == "rototo-workspace.toml" {
                ""
            } else {
                manifest_path
            }
        });
    if path.is_empty() {
        ".".to_owned()
    } else {
        path.to_owned()
    }
}

/// Percent-encode a single URL path segment or query value.
fn enc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_spec_parses_owner_name() {
        assert_eq!(
            parse_repo_spec(" octo/configs ").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("https://github.com/octo/configs.git").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("git@github.com:octo/configs.git").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("ssh://git@github.com/octo/configs.git").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert!(parse_repo_spec("octo").is_err());
        assert!(parse_repo_spec("octo/configs/extra").is_err());
        assert!(parse_repo_spec("https://example.com/octo/configs").is_err());
        assert!(parse_repo_spec("https://github.com/octo/configs/tree/main").is_err());
        assert!(parse_repo_spec("octo/with space").is_err());
    }

    #[test]
    fn archive_source_appends_subdir_fragment() {
        assert_eq!(
            workspace_archive_source("o", "r", "main", "."),
            "https://api.github.com/repos/o/r/tarball/main"
        );
        assert_eq!(
            workspace_archive_source("o", "r", "main", "payments/flags"),
            "https://api.github.com/repos/o/r/tarball/main#:payments/flags"
        );
    }

    #[test]
    fn git_source_appends_ref_and_subdir_fragment() {
        assert_eq!(
            workspace_git_source("o", "r", "main", "."),
            "git+https://github.com/o/r.git#main"
        );
        assert_eq!(
            workspace_git_source("o", "r", "main", "payments/flags"),
            "git+https://github.com/o/r.git#main:payments/flags"
        );
    }

    #[test]
    fn manifest_paths_map_to_workspace_paths() {
        assert_eq!(manifest_workspace_path("rototo-workspace.toml"), ".");
        assert_eq!(
            manifest_workspace_path("payments/flags/rototo-workspace.toml"),
            "payments/flags"
        );
    }

    #[test]
    fn stable_workspace_key_is_short_hex() {
        let key = stable_workspace_key("octo", "configs", ".");
        assert_eq!(key.len(), 12);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(key, stable_workspace_key("octo", "configs", "."));
    }
}
