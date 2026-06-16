use std::sync::Arc;

use super::{RepoRelativePath, WorkspacePath};
use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;

#[derive(Clone, Debug)]
pub struct WorkspaceDiscovery {
    pub workspaces: Vec<WorkspacePath>,
}

#[derive(Clone, Debug)]
pub struct BranchChanges {
    pub changed_files: Vec<RepoRelativePath>,
}

#[derive(Clone, Debug)]
pub struct SemanticWorkspace {
    pub workspace: Arc<Workspace>,
    pub model: Arc<WorkspaceSemanticModel>,
}
