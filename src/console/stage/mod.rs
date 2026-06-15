#![allow(dead_code, unused_imports)]

mod cache;
mod discovery;
mod identity;
mod load;
mod records;
mod selector;

pub use self::cache::StageCache;
pub use self::identity::{
    BranchName, GitCommit, GitRefName, RepoRelativePath, SourceTree, SourceTreeCacheKey,
    SourceTreeSelection, TokenIdentity, WorkspacePath,
};
pub use self::records::{
    BranchChanges, BranchTrackingState, PullRequestRef, SemanticWorkspace, SourceTreeId,
    SourceTreeRecord, TrackedBranchId, TrackedBranchRecord, WorkspaceDiscovery,
};
pub use self::selector::{WorkspaceSelector, WorkspaceSelectorInput};
