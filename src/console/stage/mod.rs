#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::{Result, RototoError};
use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;

pub type SourceTreeId = String;
pub type TrackedBranchId = String;
pub type GitRefName = String;
pub type GitCommit = String;
pub type BranchName = String;
pub type WorkspacePath = String;
pub type RepoRelativePath = String;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceTree {
    GitHub { owner: String, name: String },
    GitRemote { remote_url: String },
    LocalFolder { root: PathBuf },
    Archive { url: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TokenIdentity {
    Anonymous,
    Ambient,
    TokenHash(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceTreeCacheKey {
    pub principal_id: String,
    pub source_tree: SourceTree,
    pub token_identity: TokenIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceTreeSelection {
    BaseRef(GitRefName),
    Branch(BranchName),
    Commit(GitCommit),
    CurrentTree,
    ArchiveFingerprint(String),
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
