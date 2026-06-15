#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::{Result, RototoError};
use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;

pub type SourceTreeId = String;
pub type TrackedBranchId = String;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GitRefName(String);

impl GitRefName {
    pub fn new(value: impl AsRef<str>) -> Result<Self> {
        let value = normalize_git_ref(value.as_ref(), "git ref")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GitRefName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for GitRefName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GitCommit(String);

impl GitCommit {
    pub fn new(value: impl AsRef<str>) -> Result<Self> {
        let value = value.as_ref().trim();
        if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(RototoError::new("git commit must be a 40-character hex id"));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GitCommit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for GitCommit {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(value: impl AsRef<str>) -> Result<Self> {
        let value = normalize_git_ref(value.as_ref(), "branch name")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for BranchName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WorkspacePath(String);

impl WorkspacePath {
    pub fn new(value: impl AsRef<str>) -> Result<Self> {
        let value = normalize_tree_relative_path(value.as_ref(), true, "workspace path")?;
        Ok(Self(value))
    }

    pub fn root() -> Self {
        Self(".".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for WorkspacePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RepoRelativePath(String);

impl RepoRelativePath {
    pub fn new(value: impl AsRef<str>) -> Result<Self> {
        let value = normalize_tree_relative_path(value.as_ref(), false, "repo-relative path")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoRelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for RepoRelativePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceTree {
    GitHub { owner: String, name: String },
    GitRemote { remote_url: String },
    LocalFolder { root: PathBuf },
    Archive { url: String },
}

impl SourceTree {
    pub fn github(owner: impl AsRef<str>, name: impl AsRef<str>) -> Result<Self> {
        Ok(Self::GitHub {
            owner: normalize_github_name(owner.as_ref(), "GitHub owner")?,
            name: normalize_github_name(name.as_ref(), "GitHub repository")?,
        })
    }

    pub fn git_remote(remote_url: impl AsRef<str>) -> Result<Self> {
        let remote_url = remote_url.as_ref().trim();
        if remote_url.is_empty() {
            return Err(RototoError::new("git remote URL cannot be empty"));
        }

        if let Some(path) = github_remote_path(remote_url) {
            let (owner, name) = parse_github_path(path)?;
            return Self::github(owner, name);
        }

        Ok(Self::GitRemote {
            remote_url: normalize_git_remote_url(remote_url),
        })
    }

    pub async fn local_folder(root: impl AsRef<Path>) -> Result<Self> {
        let root = tokio::fs::canonicalize(root.as_ref())
            .await
            .map_err(|err| {
                RototoError::new(format!(
                    "failed to canonicalize local source tree `{}`: {err}",
                    root.as_ref().display()
                ))
            })?;
        Ok(Self::LocalFolder { root })
    }

    pub fn archive(url: impl AsRef<str>) -> Result<Self> {
        let url = trim_source_fragment(url.as_ref().trim());
        if url.is_empty() {
            return Err(RototoError::new("archive URL cannot be empty"));
        }
        let Some((scheme, _)) = url.split_once("://") else {
            return Err(RototoError::new("archive URL must be an HTTPS URL"));
        };
        if !scheme.eq_ignore_ascii_case("https") {
            return Err(RototoError::new("archive URL must be an HTTPS URL"));
        }
        Ok(Self::Archive {
            url: normalize_url_scheme_host(url),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TokenIdentity {
    None,
    Sha256Hex(String),
}

impl TokenIdentity {
    pub fn none() -> Self {
        Self::None
    }

    pub fn from_bearer(token: impl AsRef<str>) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, token.as_ref().as_bytes());
        Self::Sha256Hex(hex_digest(digest.as_ref()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceTreeCacheKey {
    pub principal_id: String,
    pub source: SourceTree,
    pub token: TokenIdentity,
}

impl SourceTreeCacheKey {
    pub fn new(
        principal_id: impl Into<String>,
        source: SourceTree,
        token: TokenIdentity,
    ) -> Result<Self> {
        let principal_id = principal_id.into();
        if principal_id.trim().is_empty() {
            return Err(RototoError::new("principal id cannot be empty"));
        }
        Ok(Self {
            principal_id,
            source,
            token,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceTreeSelection {
    BaseRef(GitRefName),
    Branch(BranchName),
    Commit(GitCommit),
    CurrentTree,
    ArchiveFingerprint(String),
}

impl SourceTreeSelection {
    pub fn base_ref(value: impl AsRef<str>) -> Result<Self> {
        Ok(Self::BaseRef(GitRefName::new(value)?))
    }

    pub fn branch(value: impl AsRef<str>) -> Result<Self> {
        Ok(Self::Branch(BranchName::new(value)?))
    }

    pub fn commit(value: impl AsRef<str>) -> Result<Self> {
        Ok(Self::Commit(GitCommit::new(value)?))
    }

    pub fn current_tree() -> Self {
        Self::CurrentTree
    }

    pub fn archive_fingerprint(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(RototoError::new("archive fingerprint cannot be empty"));
        }
        Ok(Self::ArchiveFingerprint(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WorkspaceSelector {
    pub source_tree: SourceTreeCacheKey,
    pub path: WorkspacePath,
    pub selection: SourceTreeSelection,
}

#[derive(Clone, Debug)]
pub struct SourceTreeRecord {
    pub id: SourceTreeId,
    pub principal_id: String,
    pub source: SourceTree,
    pub default_ref: Option<GitRefName>,
    pub display_name: String,
    pub created_at: String,
    pub last_opened_at: String,
    pub last_validated_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PullRequestRef {
    pub number: i64,
    pub url: String,
    pub state: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BranchTrackingState {
    Active,
    Recent,
    Archived,
}

#[derive(Clone, Debug)]
pub struct TrackedBranchRecord {
    pub id: TrackedBranchId,
    pub source_tree_id: SourceTreeId,
    pub branch: BranchName,
    pub base_ref: GitRefName,
    pub base_commit: Option<GitCommit>,
    pub pull_request: Option<PullRequestRef>,
    pub last_selected_workspace: Option<WorkspacePath>,
    pub last_seen_commit: Option<GitCommit>,
    pub tracking: BranchTrackingState,
    pub created_at: String,
    pub last_opened_at: String,
    pub last_edited_at: Option<String>,
    pub archived_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceDiscovery {
    pub source_tree: SourceTreeCacheKey,
    pub selection: SourceTreeSelection,
    pub workspaces: Vec<WorkspacePath>,
}

#[derive(Clone, Debug)]
pub struct BranchChanges {
    pub branch: BranchName,
    pub base_ref: GitRefName,
    pub changed_files: Vec<RepoRelativePath>,
    pub affected_workspaces: Vec<WorkspacePath>,
}

#[derive(Clone, Debug)]
pub struct SemanticWorkspace {
    pub workspace: Arc<Workspace>,
    pub model: Arc<WorkspaceSemanticModel>,
}

#[derive(Clone, Debug, Default)]
pub struct StageCache {
    _inner: Arc<StageCacheInner>,
}

#[derive(Debug, Default)]
struct StageCacheInner {
    _source_trees: Mutex<HashMap<SourceTreeCacheKey, SourceTreeSlot>>,
}

#[derive(Debug, Default)]
struct SourceTreeSlot;

impl StageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn discover_workspaces(
        &self,
        _source_tree: SourceTreeCacheKey,
        _selection: SourceTreeSelection,
    ) -> Result<WorkspaceDiscovery> {
        Err(stage_rewrite_error("discover_workspaces"))
    }

    pub async fn get_branch_changes(
        &self,
        _source_tree: SourceTreeCacheKey,
        _branch: BranchName,
        _base_ref: GitRefName,
    ) -> Result<BranchChanges> {
        Err(stage_rewrite_error("get_branch_changes"))
    }

    pub async fn get_inspected_workspace(
        &self,
        _selector: WorkspaceSelector,
    ) -> Result<Arc<Workspace>> {
        Err(stage_rewrite_error("get_inspected_workspace"))
    }

    pub async fn get_semantic_workspace(
        &self,
        _selector: WorkspaceSelector,
    ) -> Result<SemanticWorkspace> {
        Err(stage_rewrite_error("get_semantic_workspace"))
    }

    pub async fn get_runtime_workspace(
        &self,
        _selector: WorkspaceSelector,
    ) -> Result<Arc<Workspace>> {
        Err(stage_rewrite_error("get_runtime_workspace"))
    }

    pub async fn invalidate_workspace(&self, _selector: &WorkspaceSelector) {}

    pub async fn invalidate_branch(&self, _source_tree: &SourceTreeCacheKey, _branch: &str) {}

    pub async fn inspect(&self, _token: &str, _source: &str) -> Result<Arc<Workspace>> {
        Err(stage_rewrite_error("inspect"))
    }

    pub async fn semantic_model(
        &self,
        _token: &str,
        _source: &str,
    ) -> Result<(Arc<Workspace>, Arc<WorkspaceSemanticModel>)> {
        Err(stage_rewrite_error("semantic_model"))
    }

    pub async fn runtime(&self, _token: &str, _source: &str) -> Result<Arc<Workspace>> {
        Err(stage_rewrite_error("runtime"))
    }

    pub async fn invalidate_source(&self, _source: &str) {}
}

fn stage_rewrite_error(operation: &str) -> RototoError {
    RototoError::new(format!(
        "console stage operation `{operation}` is unavailable while the stage cache is being rebuilt"
    ))
}

fn normalize_tree_relative_path(value: &str, allow_root: bool, kind: &str) -> Result<String> {
    let value = value.trim().replace('\\', "/");
    if value.starts_with('/') || is_windows_absolute_path(&value) {
        return Err(RototoError::new(format!("{kind} must be relative")));
    }

    let value = value.trim_end_matches('/').to_owned();
    if value.is_empty() || value == "." {
        if allow_root {
            return Ok(".".to_owned());
        }
        return Err(RototoError::new(format!("{kind} must identify a file")));
    }

    for component in value.split('/') {
        if component.is_empty() {
            return Err(RototoError::new(format!(
                "{kind} cannot contain empty path components"
            )));
        }
        if component == "." || component == ".." {
            return Err(RototoError::new(format!(
                "{kind} cannot contain `.` or `..` components"
            )));
        }
    }

    Ok(value)
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

fn normalize_git_ref(value: &str, kind: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(RototoError::new(format!("{kind} cannot be empty")));
    }
    if value.starts_with('-') {
        return Err(RototoError::new(format!("{kind} cannot begin with `-`")));
    }
    Ok(value.to_owned())
}

fn normalize_github_name(value: &str, kind: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return Err(RototoError::new(format!("{kind} is not valid")));
    }
    Ok(value.to_ascii_lowercase())
}

fn normalize_git_remote_url(value: &str) -> String {
    let value = trim_source_fragment(value.trim()).trim_end_matches('/');
    normalize_url_scheme_host(value)
}

fn normalize_url_scheme_host(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_owned();
    };
    let (authority, suffix) = split_authority_suffix(rest);
    format!(
        "{}://{}{}",
        scheme.to_ascii_lowercase(),
        normalize_authority_host(authority),
        suffix
    )
}

fn split_authority_suffix(value: &str) -> (&str, &str) {
    let split_at = value
        .char_indices()
        .find_map(|(index, c)| matches!(c, '/' | '?').then_some(index))
        .unwrap_or(value.len());
    value.split_at(split_at)
}

fn normalize_authority_host(authority: &str) -> String {
    let (userinfo, host_port) = authority
        .rsplit_once('@')
        .map(|(userinfo, host_port)| (Some(userinfo), host_port))
        .unwrap_or((None, authority));
    let host_port = normalize_host_port(host_port);
    match userinfo {
        Some(userinfo) => format!("{userinfo}@{host_port}"),
        None => host_port,
    }
}

fn normalize_host_port(host_port: &str) -> String {
    if host_port.starts_with('[') {
        return host_port.to_ascii_lowercase();
    }

    if let Some((host, port)) = host_port.rsplit_once(':')
        && port.chars().all(|c| c.is_ascii_digit())
    {
        return format!("{}:{port}", host.to_ascii_lowercase());
    }

    host_port.to_ascii_lowercase()
}

fn github_remote_path(value: &str) -> Option<&str> {
    let value = trim_source_fragment(value.trim()).trim_end_matches('/');
    if let Some(path) = strip_prefix_ignore_ascii_case(value, "git@github.com:") {
        return Some(path);
    }

    let value = strip_prefix_ignore_ascii_case(value, "git+").unwrap_or(value);
    let (_, rest) = value.split_once("://")?;
    let (authority, suffix) = split_authority_suffix(rest);
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host_port)| host_port)
        .unwrap_or(authority);
    let host = host_port
        .strip_prefix('[')
        .and_then(|host| host.split_once(']').map(|(host, _)| host))
        .or_else(|| host_port.split_once(':').map(|(host, _)| host))
        .unwrap_or(host_port);
    if host.eq_ignore_ascii_case("github.com") {
        return suffix.strip_prefix('/');
    }
    None
}

fn parse_github_path(path: &str) -> Result<(String, String)> {
    let path = path.trim_matches('/');
    let mut parts = path.split('/');
    let Some(owner) = parts.next() else {
        return Err(RototoError::new(
            "GitHub remote must include owner and repo",
        ));
    };
    let Some(name) = parts.next() else {
        return Err(RototoError::new(
            "GitHub remote must include owner and repo",
        ));
    };
    if parts.next().is_some() {
        return Err(RototoError::new(
            "GitHub remote path must be owner/repo, not a nested path",
        ));
    }
    let name = name.strip_suffix(".git").unwrap_or(name);
    Ok((
        normalize_github_name(owner, "GitHub owner")?,
        normalize_github_name(name, "GitHub repository")?,
    ))
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        value.get(prefix.len()..)
    } else {
        None
    }
}

fn trim_source_fragment(value: &str) -> &str {
    value.split_once('#').map(|(head, _)| head).unwrap_or(value)
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn workspace_paths_normalize_tree_relative_identity() {
        assert_eq!(WorkspacePath::new("").unwrap().as_str(), ".");
        assert_eq!(WorkspacePath::new(".").unwrap().as_str(), ".");
        assert_eq!(
            WorkspacePath::new("workspaces/payments").unwrap().as_str(),
            "workspaces/payments"
        );
        assert_eq!(
            WorkspacePath::new("workspaces/payments/").unwrap().as_str(),
            "workspaces/payments"
        );
        assert_eq!(
            WorkspacePath::new("workspaces\\payments").unwrap().as_str(),
            "workspaces/payments"
        );

        assert!(WorkspacePath::new("/workspaces/payments").is_err());
        assert!(WorkspacePath::new("C:\\workspaces\\payments").is_err());
        assert!(WorkspacePath::new("../payments").is_err());
        assert!(WorkspacePath::new("workspaces/../api").is_err());
        assert!(WorkspacePath::new("workspaces//api").is_err());
    }

    #[test]
    fn repo_relative_paths_reject_workspace_root_identity() {
        assert_eq!(
            RepoRelativePath::new("workspaces/payments/variables/checkout.toml")
                .unwrap()
                .as_str(),
            "workspaces/payments/variables/checkout.toml"
        );
        assert_eq!(
            RepoRelativePath::new("rototo-workspace.toml")
                .unwrap()
                .as_str(),
            "rototo-workspace.toml"
        );

        assert!(RepoRelativePath::new(".").is_err());
        assert!(RepoRelativePath::new("").is_err());
        assert!(RepoRelativePath::new("/rototo-workspace.toml").is_err());
        assert!(RepoRelativePath::new("workspaces/payments/../api/file.toml").is_err());
    }

    #[test]
    fn source_tree_normalizes_github_identity() {
        let expected = SourceTree::GitHub {
            owner: "rototo".to_owned(),
            name: "config".to_owned(),
        };

        assert_eq!(SourceTree::github("Rototo", "Config").unwrap(), expected);
        assert_eq!(
            SourceTree::git_remote(
                "git+https://github.com/Rototo/Config.git#main:workspaces/payments"
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            SourceTree::git_remote("git+ssh://git@github.com/Rototo/Config.git#feature/payments:.")
                .unwrap(),
            expected
        );
        assert_eq!(
            SourceTree::git_remote("git@github.com:Rototo/Config.git").unwrap(),
            expected
        );
    }

    #[test]
    fn source_tree_normalizes_generic_git_remote_without_overfitting() {
        assert_eq!(
            SourceTree::git_remote("git+https://Git.Example.com/Team/Config.git#main:services/api")
                .unwrap(),
            SourceTree::GitRemote {
                remote_url: "git+https://git.example.com/Team/Config.git".to_owned()
            }
        );
    }

    #[test]
    fn source_tree_normalizes_archive_url_identity() {
        assert_eq!(
            SourceTree::archive("https://EXAMPLE.com/releases/config.tar.gz#:workspaces/payments")
                .unwrap(),
            SourceTree::Archive {
                url: "https://example.com/releases/config.tar.gz".to_owned()
            }
        );
        assert!(SourceTree::archive("http://example.com/config.tar.gz").is_err());
    }

    #[tokio::test]
    async fn source_tree_canonicalizes_local_folder_identity() {
        let tempdir = TempDir::new().expect("tempdir");
        let nested = tempdir.path().join("repo");
        tokio::fs::create_dir(&nested)
            .await
            .expect("create repo dir");

        assert_eq!(
            SourceTree::local_folder(&nested).await.unwrap(),
            SourceTree::LocalFolder {
                root: nested.canonicalize().unwrap()
            }
        );
    }

    #[test]
    fn token_identity_hashes_raw_bearer_token() {
        assert_eq!(
            TokenIdentity::from_bearer("ghp_secret"),
            TokenIdentity::Sha256Hex(
                "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434".to_owned()
            )
        );
    }

    #[test]
    fn cache_key_separates_principal_source_and_token_identity() {
        let source = SourceTree::github("Rototo", "Config").unwrap();
        let anonymous =
            SourceTreeCacheKey::new("user_123", source.clone(), TokenIdentity::none()).unwrap();
        let with_token = SourceTreeCacheKey::new(
            "user_123",
            source.clone(),
            TokenIdentity::from_bearer("ghp_secret"),
        )
        .unwrap();
        let other_principal =
            SourceTreeCacheKey::new("user_456", source, TokenIdentity::from_bearer("ghp_secret"))
                .unwrap();

        assert_ne!(anonymous, with_token);
        assert_ne!(with_token, other_principal);
    }

    #[test]
    fn source_tree_selection_validates_ref_branch_and_commit_identity() {
        assert!(SourceTreeSelection::base_ref("main").is_ok());
        assert!(SourceTreeSelection::base_ref("").is_err());
        assert!(SourceTreeSelection::base_ref("-main").is_err());

        assert!(SourceTreeSelection::branch("rototo-console/alice/change-checkout").is_ok());
        assert!(SourceTreeSelection::branch("-bad").is_err());

        assert_eq!(
            SourceTreeSelection::commit("8D3C4B5A6F7081920A1B2C3D4E5F60718293A4B5").unwrap(),
            SourceTreeSelection::Commit(GitCommit(
                "8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5".to_owned()
            ))
        );
        assert!(SourceTreeSelection::commit("not-a-commit").is_err());
    }
}
