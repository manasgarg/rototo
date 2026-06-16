#![allow(dead_code)]

use std::fmt::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::{Result, RototoError};

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
pub enum SourceTreeOrigin {
    GitHub { owner: String, name: String },
    GitRemote { remote_url: String },
    LocalFolder { root: PathBuf },
    Archive { url: String },
}

impl SourceTreeOrigin {
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

    pub fn from_console_token(token: impl AsRef<str>) -> Self {
        let token = token.as_ref();
        if token.is_empty() {
            Self::None
        } else {
            Self::from_bearer(token)
        }
    }

    pub fn from_bearer(token: impl AsRef<str>) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, token.as_ref().as_bytes());
        Self::Sha256Hex(hex_digest(digest.as_ref()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceTreeRevision {
    GitRef(GitRefName),
    GitBranch(BranchName),
    GitCommit(GitCommit),
    LocalWorkingTree,
    ArchiveSnapshot(String),
}

impl SourceTreeRevision {
    pub fn git_ref(value: impl AsRef<str>) -> Result<Self> {
        Ok(Self::GitRef(GitRefName::new(value)?))
    }

    pub fn git_ref_or_commit(value: impl AsRef<str>) -> Result<Self> {
        let value = value.as_ref().trim();
        if is_full_git_commit(value) {
            Self::git_commit(value)
        } else {
            Self::git_ref(value)
        }
    }

    pub fn git_branch(value: impl AsRef<str>) -> Result<Self> {
        Ok(Self::GitBranch(BranchName::new(value)?))
    }

    pub fn git_commit(value: impl AsRef<str>) -> Result<Self> {
        Ok(Self::GitCommit(GitCommit::new(value)?))
    }

    pub fn local_working_tree() -> Self {
        Self::LocalWorkingTree
    }

    pub fn archive_snapshot(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(RototoError::new("archive snapshot cannot be empty"));
        }
        Ok(Self::ArchiveSnapshot(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceTreeLocator {
    pub origin: SourceTreeOrigin,
    pub revision: SourceTreeRevision,
}

impl SourceTreeLocator {
    pub fn new(origin: SourceTreeOrigin, revision: SourceTreeRevision) -> Self {
        Self { origin, revision }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WorkspaceLocator {
    pub source_tree: SourceTreeLocator,
    pub path: WorkspacePath,
}

impl WorkspaceLocator {
    pub fn new(
        origin: SourceTreeOrigin,
        revision: SourceTreeRevision,
        path: WorkspacePath,
    ) -> Self {
        Self {
            source_tree: SourceTreeLocator::new(origin, revision),
            path,
        }
    }

    pub fn from_source_tree(source_tree: SourceTreeLocator, path: WorkspacePath) -> Self {
        Self { source_tree, path }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CachedSourceTreeOrigin {
    pub principal_id: String,
    pub origin: SourceTreeOrigin,
    pub token: TokenIdentity,
}

impl CachedSourceTreeOrigin {
    pub fn new(
        principal_id: impl Into<String>,
        origin: SourceTreeOrigin,
        token: TokenIdentity,
    ) -> Result<Self> {
        let principal_id = principal_id.into();
        if principal_id.trim().is_empty() {
            return Err(RototoError::new("principal id cannot be empty"));
        }
        Ok(Self {
            principal_id,
            origin,
            token,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CachedWorkspaceLocator {
    pub principal_id: String,
    pub token: TokenIdentity,
    pub workspace: WorkspaceLocator,
}

impl CachedWorkspaceLocator {
    pub fn new(
        principal_id: impl Into<String>,
        workspace: WorkspaceLocator,
        token: TokenIdentity,
    ) -> Result<Self> {
        let principal_id = principal_id.into();
        if principal_id.trim().is_empty() {
            return Err(RototoError::new("principal id cannot be empty"));
        }
        Ok(Self {
            principal_id,
            token,
            workspace,
        })
    }

    pub fn cached_source_tree_origin(&self) -> Result<CachedSourceTreeOrigin> {
        CachedSourceTreeOrigin::new(
            self.principal_id.clone(),
            self.workspace.source_tree.origin.clone(),
            self.token.clone(),
        )
    }
}

pub(super) fn is_full_git_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        value.get(prefix.len()..)
    } else {
        None
    }
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
    fn source_tree_origin_normalizes_github_identity() {
        let expected = SourceTreeOrigin::GitHub {
            owner: "rototo".to_owned(),
            name: "config".to_owned(),
        };

        assert_eq!(
            SourceTreeOrigin::github("Rototo", "Config").unwrap(),
            expected
        );
        assert_eq!(
            SourceTreeOrigin::git_remote(
                "git+https://github.com/Rototo/Config.git#main:workspaces/payments"
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            SourceTreeOrigin::git_remote(
                "git+ssh://git@github.com/Rototo/Config.git#feature/payments:."
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            SourceTreeOrigin::git_remote("git@github.com:Rototo/Config.git").unwrap(),
            expected
        );
    }

    #[test]
    fn source_tree_origin_normalizes_generic_git_remote_without_overfitting() {
        assert_eq!(
            SourceTreeOrigin::git_remote(
                "git+https://Git.Example.com/Team/Config.git#main:services/api"
            )
            .unwrap(),
            SourceTreeOrigin::GitRemote {
                remote_url: "git+https://git.example.com/Team/Config.git".to_owned()
            }
        );
    }

    #[test]
    fn source_tree_origin_normalizes_archive_url_identity() {
        assert_eq!(
            SourceTreeOrigin::archive(
                "https://EXAMPLE.com/releases/config.tar.gz#:workspaces/payments"
            )
            .unwrap(),
            SourceTreeOrigin::Archive {
                url: "https://example.com/releases/config.tar.gz".to_owned()
            }
        );
        assert!(SourceTreeOrigin::archive("http://example.com/config.tar.gz").is_err());
    }

    #[tokio::test]
    async fn source_tree_origin_canonicalizes_local_folder_identity() {
        let tempdir = TempDir::new().expect("tempdir");
        let nested = tempdir.path().join("repo");
        tokio::fs::create_dir(&nested)
            .await
            .expect("create repo dir");

        assert_eq!(
            SourceTreeOrigin::local_folder(&nested).await.unwrap(),
            SourceTreeOrigin::LocalFolder {
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
    fn cached_source_tree_origin_separates_principal_tree_and_token_identity() {
        let source = SourceTreeOrigin::github("Rototo", "Config").unwrap();
        let anonymous =
            CachedSourceTreeOrigin::new("user_123", source.clone(), TokenIdentity::none()).unwrap();
        let with_token = CachedSourceTreeOrigin::new(
            "user_123",
            source.clone(),
            TokenIdentity::from_bearer("ghp_secret"),
        )
        .unwrap();
        let other_principal = CachedSourceTreeOrigin::new(
            "user_456",
            source,
            TokenIdentity::from_bearer("ghp_secret"),
        )
        .unwrap();

        assert_ne!(anonymous, with_token);
        assert_ne!(with_token, other_principal);
    }

    #[test]
    fn source_tree_revision_validates_ref_branch_and_commit_identity() {
        assert!(SourceTreeRevision::git_ref("main").is_ok());
        assert!(SourceTreeRevision::git_ref("").is_err());
        assert!(SourceTreeRevision::git_ref("-main").is_err());

        assert!(SourceTreeRevision::git_branch("rototo-console/alice/change-checkout").is_ok());
        assert!(SourceTreeRevision::git_branch("-bad").is_err());

        assert_eq!(
            SourceTreeRevision::git_commit("8D3C4B5A6F7081920A1B2C3D4E5F60718293A4B5").unwrap(),
            SourceTreeRevision::GitCommit(
                GitCommit::new("8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5").unwrap()
            )
        );
        assert!(SourceTreeRevision::git_commit("not-a-commit").is_err());
    }
}
