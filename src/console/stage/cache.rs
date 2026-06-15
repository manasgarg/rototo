#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::discovery;
use super::load;
use super::runtime;
use super::semantic;
use super::{
    BranchChanges, BranchName, CachedTreeSource, CachedWorkspaceSource, GitRefName,
    SemanticWorkspace, TreeRevision, WorkspaceDiscovery,
};
use crate::error::{Result, RototoError};
use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;

#[derive(Clone, Debug, Default)]
pub struct StageCache {
    _inner: Arc<StageCacheInner>,
}

#[derive(Debug, Default)]
struct StageCacheInner {
    _tree_sources: Mutex<HashMap<CachedTreeSource, TreeSourceSlot>>,
}

#[derive(Debug, Default)]
struct TreeSourceSlot;

impl StageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn discover_workspaces(
        &self,
        cached_tree: CachedTreeSource,
        revision: TreeRevision,
    ) -> Result<WorkspaceDiscovery> {
        discovery::discover_workspaces(cached_tree, revision).await
    }

    pub async fn get_branch_changes(
        &self,
        _cached_tree: CachedTreeSource,
        _branch: BranchName,
        _base_ref: GitRefName,
    ) -> Result<BranchChanges> {
        Err(stage_rewrite_error("get_branch_changes"))
    }

    pub async fn get_inspected_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<Arc<Workspace>> {
        load::get_inspected_workspace(selector, source_token).await
    }

    pub async fn get_semantic_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<SemanticWorkspace> {
        semantic::get_semantic_workspace(selector, source_token).await
    }

    pub async fn get_runtime_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<Arc<Workspace>> {
        runtime::get_runtime_workspace(selector, source_token).await
    }

    pub async fn invalidate_workspace(&self, _selector: &CachedWorkspaceSource) {}

    pub async fn invalidate_branch(&self, _cached_tree: &CachedTreeSource, _branch: &str) {}

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
