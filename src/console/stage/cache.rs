#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::discovery;
use super::{
    BranchChanges, BranchName, GitRefName, SemanticWorkspace, SourceTreeCacheKey,
    SourceTreeSelection, WorkspaceDiscovery, WorkspaceSelector,
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
        source_tree: SourceTreeCacheKey,
        selection: SourceTreeSelection,
    ) -> Result<WorkspaceDiscovery> {
        discovery::discover_workspaces(source_tree, selection).await
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
