use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};

use super::error::{GitHubApiError, GitHubError, GitHubResult};
use super::source::{enc, encode_repo_path, manifest_package_path, package_git_source};
use super::{GITHUB_API, GITHUB_USER_AGENT};

/// GitHub viewer identity returned by `/user`.
///
/// Auth code converts this into `ActorIdentity` and session/user records. The
/// DTO itself is transient and exists only while handling token validation or
/// sign-in.
#[derive(Clone, Debug, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

/// Repository metadata returned by GitHub.
///
/// Registration and permission checks read this DTO, then persist only the
/// rototo console repo fields needed for future package discovery.
#[derive(Clone, Debug, Deserialize)]
pub struct GitHubRepo {
    pub name: String,
    pub owner: GitHubRepoOwner,
    pub default_branch: String,
    pub permissions: Option<GitHubRepoPermissions>,
}

/// Owner object nested inside GitHub repository responses.
///
/// It exists because GitHub returns owner metadata as an object while rototo's
/// store only needs the login string.
#[derive(Clone, Debug, Deserialize)]
pub struct GitHubRepoOwner {
    pub login: String,
}

/// GitHub permission flags used to decide whether a credential can write.
///
/// The client reads this during a permission check and discards it after
/// choosing whether a branch mutation may continue.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct GitHubRepoPermissions {
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub push: bool,
}

/// Rototo package found in a GitHub repository tree.
///
/// Discovery creates these from `rototo-package.toml` blobs. Store code
/// upserts them into durable `packages` rows for the registering principal.
#[derive(Clone, Debug)]
pub struct DiscoveredPackage {
    pub path: String,
    pub git_ref: String,
    pub source: String,
}

/// File content plus blob SHA returned by GitHub contents API.
///
/// Branch save/delete paths need both pieces: content to diff or edit, and SHA
/// to perform GitHub's optimistic update. The value lives only for one file
/// operation.
#[derive(Clone, Debug)]
pub struct GitHubContentFile {
    pub sha: String,
    pub content: String,
}

/// Pull request metadata returned by GitHub.
///
/// Publish and sync routes copy these fields into a branch row so the
/// UI can show PR state without requiring a GitHub request on every render.
#[derive(Clone, Debug, Deserialize)]
pub struct GitHubPullRequest {
    pub html_url: String,
    pub number: i64,
    pub state: Option<String>,
    pub merged_at: Option<String>,
}

/// One entry from a GitHub recursive tree response.
///
/// Discovery, entity creation conflict checks, and branch scans use these to
/// find blobs. They are transient results of GitHub API calls.
#[derive(Clone, Debug, Deserialize)]
pub struct GitHubTreeEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
}

/// Branch comparison summary returned by GitHub.
///
/// Branch-candidate scans use it to decide whether a branch changes only files
/// in the package path. It is filtered into a UI summary and not persisted.
#[derive(Clone, Debug)]
pub struct RefComparison {
    pub ahead_by: i64,
    pub files: Vec<String>,
}

/// Small async GitHub REST client used by the console server.
///
/// The client owns a reusable `reqwest::Client` and lives in `ConsoleState` for
/// the process lifetime. It does not store credentials; every call receives the
/// current user's token explicitly.
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
                 repository write access before editing this package."
            )));
        }
        Ok(())
    }

    pub async fn discover_packages(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        git_ref: &str,
    ) -> GitHubResult<Vec<DiscoveredPackage>> {
        let tree = self.tree(token, owner, name, git_ref).await?;
        let mut packages: Vec<DiscoveredPackage> = tree
            .into_iter()
            .filter(|entry| {
                entry.entry_type == "blob" && entry.path.ends_with("rototo-package.toml")
            })
            .map(|entry| {
                let path = manifest_package_path(&entry.path);
                DiscoveredPackage {
                    source: package_git_source(owner, name, git_ref, &path),
                    path,
                    git_ref: git_ref.to_owned(),
                }
            })
            .collect();
        packages.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(packages)
    }

    pub async fn branch_head_sha(
        &self,
        token: &str,
        owner: &str,
        name: &str,
        branch: &str,
    ) -> GitHubResult<String> {
        /// Git ref lookup response from GitHub.
        ///
        /// The method validates that the ref points at a commit and returns
        /// only the SHA needed for branch creation or write checks.
        #[derive(Deserialize)]
        struct RefResponse {
            object: RefObject,
        }
        /// Nested git object in a GitHub ref response.
        ///
        /// It lives only long enough to reject refs that do not resolve to a
        /// commit object.
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
        /// Branch list item returned by GitHub.
        ///
        /// The client keeps only branch names for branch-candidate scanning.
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
        /// GitHub compare response subset used by branch candidate scans.
        ///
        /// It is converted immediately into `RefComparison`.
        #[derive(Deserialize)]
        struct Comparison {
            ahead_by: i64,
            #[serde(default)]
            files: Vec<ComparisonFile>,
        }
        /// Changed-file item nested in a GitHub compare response.
        ///
        /// Only the filename is needed to decide whether a branch touches the
        /// package path.
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
        /// GitHub branch-rename response body.
        ///
        /// The route persists the returned name on the branch row and discards
        /// the raw response shape.
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
        /// GitHub contents response for a file.
        ///
        /// The client validates it is a base64 file blob, decodes the content,
        /// and returns `GitHubContentFile` for one branch operation.
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
        /// GitHub recursive tree response.
        ///
        /// The client rejects truncated trees because discovery and conflict
        /// checks need a complete file list for the requested ref.
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
        let path_pattern = github_path_pattern(path);
        tracing::debug!(
            operation = "github.rest",
            method = %method_label,
            path = %path_pattern,
            "console outbound GitHub REST call started"
        );
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
        let response = match request.send().await {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!(
                    operation = "github.rest",
                    method = %method_label,
                    path = %path_pattern,
                    error = %err,
                    latency_ms = started.elapsed().as_millis(),
                    "console outbound GitHub REST call failed before response"
                );
                return Err(GitHubError::other(err));
            }
        };
        let status = response.status();
        let text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                tracing::warn!(
                    operation = "github.rest",
                    method = %method_label,
                    path = %path_pattern,
                    status = status.as_u16(),
                    error = %err,
                    latency_ms = started.elapsed().as_millis(),
                    "console outbound GitHub REST response read failed"
                );
                return Err(GitHubError::other(err));
            }
        };
        if status.is_success() {
            tracing::info!(
                operation = "github.rest",
                method = %method_label,
                path = %path_pattern,
                status = status.as_u16(),
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub REST call completed"
            );
        } else {
            tracing::warn!(
                operation = "github.rest",
                method = %method_label,
                path = %path_pattern,
                status = status.as_u16(),
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub REST call returned error status"
            );
        }
        if !status.is_success() {
            return Err(GitHubError::Api(GitHubApiError {
                status: status.as_u16(),
                response_text: text,
            }));
        }
        serde_json::from_str(&text).map_err(|err| {
            tracing::warn!(
                operation = "github.rest",
                method = %method_label,
                path = %path_pattern,
                status = status.as_u16(),
                error = %err,
                latency_ms = started.elapsed().as_millis(),
                "console outbound GitHub REST response decode failed"
            );
            GitHubError::other(err)
        })
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
