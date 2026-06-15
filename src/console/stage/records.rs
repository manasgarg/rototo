#![allow(dead_code)]

use std::sync::Arc;

use super::{
    BranchName, CachedTreeSource, GitCommit, GitRefName, RepoRelativePath, TreeRevision,
    TreeSource, WorkspacePath,
};
use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;

pub type TreeSourceId = String;
pub type TrackedBranchId = String;

#[derive(Clone, Debug)]
pub struct TreeSourceRecord {
    pub id: TreeSourceId,
    pub principal_id: String,
    pub tree: TreeSource,
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
    pub tree_source_id: TreeSourceId,
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
    pub cached_tree: CachedTreeSource,
    pub revision: TreeRevision,
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
