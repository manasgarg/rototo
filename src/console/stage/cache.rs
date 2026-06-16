use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OnceCell};

use super::branch_changes;
use super::discovery;
use super::load;
use super::runtime;
use super::source_tree;
use super::{
    BranchChanges, BranchName, CachedTreeSource, CachedWorkspaceSource, GitRefName,
    SemanticWorkspace, TreeRevision, TreeSource, WorkspaceDiscovery, WorkspacePath,
    WorkspaceSource,
};
use crate::error::Result;
use crate::sdk::Workspace;
use crate::source::StagedSourceTree;

#[derive(Clone, Default)]
pub struct StageCache {
    inner: Arc<StageCacheInner>,
}

#[derive(Default)]
struct StageCacheInner {
    tree_sources: Mutex<HashMap<CachedTreeSource, Arc<TreeSourceSlot>>>,
}

#[derive(Default)]
struct TreeSourceSlot {
    source_trees: Mutex<HashMap<SourceTreeKey, Arc<SourceTreeSlot>>>,
    workspace_discoveries: Mutex<HashMap<WorkspaceDiscoveryKey, Arc<WorkspaceDiscoverySlot>>>,
    workspace_views: Mutex<HashMap<WorkspaceViewKey, Arc<WorkspaceSlot>>>,
    branch_changes: Mutex<HashMap<BranchChangesKey, Arc<BranchChangesSlot>>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct SourceTreeKey {
    revision: TreeRevision,
}

impl SourceTreeKey {
    fn new(revision: TreeRevision) -> Self {
        Self { revision }
    }

    fn is_branch(&self, branch: &str) -> bool {
        matches!(&self.revision, TreeRevision::GitBranch(name) if name.as_str() == branch)
    }

    fn is_local_working_tree(&self) -> bool {
        matches!(self.revision, TreeRevision::LocalWorkingTree)
    }
}

#[derive(Default)]
struct SourceTreeSlot {
    staged_tree: OnceCell<Arc<StagedSourceTree>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct WorkspaceDiscoveryKey {
    revision: TreeRevision,
}

impl WorkspaceDiscoveryKey {
    fn new(revision: TreeRevision) -> Self {
        Self { revision }
    }

    fn is_branch(&self, branch: &str) -> bool {
        matches!(&self.revision, TreeRevision::GitBranch(name) if name.as_str() == branch)
    }

    fn is_local_working_tree(&self) -> bool {
        matches!(self.revision, TreeRevision::LocalWorkingTree)
    }
}

#[derive(Default)]
struct WorkspaceDiscoverySlot {
    discovery: OnceCell<WorkspaceDiscovery>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct WorkspaceViewKey {
    revision: TreeRevision,
    path: WorkspacePath,
}

impl WorkspaceViewKey {
    fn new(source: &WorkspaceSource) -> Self {
        Self {
            revision: source.revision.clone(),
            path: source.path.clone(),
        }
    }

    fn is_branch(&self, branch: &str) -> bool {
        matches!(&self.revision, TreeRevision::GitBranch(name) if name.as_str() == branch)
    }

    fn is_local_working_tree(&self) -> bool {
        matches!(self.revision, TreeRevision::LocalWorkingTree)
    }
}

#[derive(Default)]
struct WorkspaceSlot {
    inspected: OnceCell<Arc<Workspace>>,
    semantic: OnceCell<SemanticWorkspace>,
    runtime: OnceCell<Arc<Workspace>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct BranchChangesKey {
    branch: BranchName,
    base_ref: GitRefName,
}

impl BranchChangesKey {
    fn new(branch: BranchName, base_ref: GitRefName) -> Self {
        Self { branch, base_ref }
    }

    fn is_branch(&self, branch: &str) -> bool {
        self.branch.as_str() == branch
    }
}

#[derive(Default)]
struct BranchChangesSlot {
    changes: OnceCell<BranchChanges>,
}

impl StageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn discover_workspaces(
        &self,
        cached_tree: CachedTreeSource,
        revision: TreeRevision,
    ) -> Result<WorkspaceDiscovery> {
        let cache = self.clone();
        let tree_slot = self.tree_slot(cached_tree.clone()).await;
        let key = WorkspaceDiscoveryKey::new(revision.clone());
        let discovery_slot = {
            let mut workspace_discoveries = tree_slot.workspace_discoveries.lock().await;
            workspace_discoveries.entry(key).or_default().clone()
        };
        let discovery = discovery_slot
            .discovery
            .get_or_try_init(|| async move {
                let staged_tree = cache
                    .get_staged_source_tree(cached_tree.clone(), revision.clone())
                    .await?;
                discovery::discover_workspaces(cached_tree, revision, staged_tree.root()).await
            })
            .await?;
        Ok(discovery.clone())
    }

    pub(crate) async fn get_staged_source_tree(
        &self,
        cached_tree: CachedTreeSource,
        revision: TreeRevision,
    ) -> Result<Arc<StagedSourceTree>> {
        let tree_slot = self.tree_slot(cached_tree.clone()).await;
        let key = SourceTreeKey::new(revision.clone());
        let source_tree_slot = {
            let mut source_trees = tree_slot.source_trees.lock().await;
            source_trees.entry(key).or_default().clone()
        };
        let staged_tree = source_tree_slot
            .staged_tree
            .get_or_try_init(|| async move {
                source_tree::stage_tree_for_revision(cached_tree, revision)
                    .await
                    .map(Arc::new)
            })
            .await?;
        Ok(Arc::clone(staged_tree))
    }

    pub async fn get_branch_changes(
        &self,
        cached_tree: CachedTreeSource,
        branch: BranchName,
        base_ref: GitRefName,
    ) -> Result<BranchChanges> {
        let tree_slot = self.tree_slot(cached_tree.clone()).await;
        let key = BranchChangesKey::new(branch.clone(), base_ref.clone());
        let changes_slot = {
            let mut branch_changes = tree_slot.branch_changes.lock().await;
            branch_changes.entry(key).or_default().clone()
        };
        let cache = self.clone();
        let changes = changes_slot
            .changes
            .get_or_try_init(|| async move {
                let revision = branch_changes::revision_for_changes(&cached_tree.tree, &branch)?;
                let discovery = cache
                    .discover_workspaces(cached_tree.clone(), revision)
                    .await?;
                branch_changes::get_branch_changes(
                    cached_tree,
                    branch,
                    base_ref,
                    &discovery.workspaces,
                )
                .await
            })
            .await?;
        Ok(changes.clone())
    }

    pub async fn get_inspected_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<Arc<Workspace>> {
        let slot = self.workspace_slot(&selector).await?;
        let workspace = slot
            .inspected
            .get_or_try_init(|| {
                let selector = selector.clone();
                let source_token = source_token.to_owned();
                async move { load::get_inspected_workspace(selector, &source_token).await }
            })
            .await?;
        Ok(Arc::clone(workspace))
    }

    pub async fn get_semantic_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<SemanticWorkspace> {
        let slot = self.workspace_slot(&selector).await?;
        let semantic = slot
            .semantic
            .get_or_try_init(|| {
                let cache = self.clone();
                let selector = selector.clone();
                let source_token = source_token.to_owned();
                async move {
                    let workspace = cache
                        .get_inspected_workspace(selector, &source_token)
                        .await?;
                    let model = workspace.semantic_model().await?;
                    Ok(SemanticWorkspace {
                        workspace,
                        model: Arc::new(model),
                    })
                }
            })
            .await?;
        Ok(semantic.clone())
    }

    pub async fn get_runtime_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<Arc<Workspace>> {
        let slot = self.workspace_slot(&selector).await?;
        let runtime = slot
            .runtime
            .get_or_try_init(|| {
                let cache = self.clone();
                let selector = selector.clone();
                let source_token = source_token.to_owned();
                async move {
                    let inspected = cache
                        .get_inspected_workspace(selector, &source_token)
                        .await?;
                    runtime::get_runtime_workspace_from_inspected(inspected, &source_token).await
                }
            })
            .await?;
        Ok(Arc::clone(runtime))
    }

    pub async fn invalidate_workspace(&self, selector: &CachedWorkspaceSource) {
        let Ok(cached_tree) = selector.cached_tree_source() else {
            return;
        };
        let Some(tree_slot) = self.tree_slot_if_present(&cached_tree).await else {
            return;
        };
        let key = WorkspaceViewKey::new(&selector.workspace);
        tree_slot.workspace_views.lock().await.remove(&key);
        if selector.workspace.revision == TreeRevision::LocalWorkingTree {
            tree_slot
                .source_trees
                .lock()
                .await
                .retain(|key, _| !key.is_local_working_tree());
            tree_slot
                .workspace_discoveries
                .lock()
                .await
                .retain(|key, _| !key.is_local_working_tree());
        }
    }

    pub async fn invalidate_branch(&self, cached_tree: &CachedTreeSource, branch: &str) {
        let Some(tree_slot) = self.tree_slot_if_present(cached_tree).await else {
            return;
        };
        let invalidate_local_working_tree =
            matches!(cached_tree.tree, TreeSource::LocalFolder { .. });
        tree_slot.source_trees.lock().await.retain(|key, _| {
            !(key.is_branch(branch) || invalidate_local_working_tree && key.is_local_working_tree())
        });
        tree_slot
            .workspace_discoveries
            .lock()
            .await
            .retain(|key, _| {
                !(key.is_branch(branch)
                    || invalidate_local_working_tree && key.is_local_working_tree())
            });
        tree_slot.workspace_views.lock().await.retain(|key, _| {
            !(key.is_branch(branch) || invalidate_local_working_tree && key.is_local_working_tree())
        });
        tree_slot
            .branch_changes
            .lock()
            .await
            .retain(|key, _| !key.is_branch(branch));
    }

    async fn workspace_slot(&self, selector: &CachedWorkspaceSource) -> Result<Arc<WorkspaceSlot>> {
        let cached_tree = selector.cached_tree_source()?;
        let tree_slot = self.tree_slot(cached_tree).await;
        let view_key = WorkspaceViewKey::new(&selector.workspace);
        let mut workspace_views = tree_slot.workspace_views.lock().await;
        Ok(workspace_views.entry(view_key).or_default().clone())
    }

    async fn tree_slot(&self, cached_tree: CachedTreeSource) -> Arc<TreeSourceSlot> {
        let mut tree_sources = self.inner.tree_sources.lock().await;
        tree_sources.entry(cached_tree).or_default().clone()
    }

    async fn tree_slot_if_present(
        &self,
        cached_tree: &CachedTreeSource,
    ) -> Option<Arc<TreeSourceSlot>> {
        let tree_sources = self.inner.tree_sources.lock().await;
        tree_sources.get(cached_tree).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::{
        RepoRelativePath, TokenIdentity, TreeSource, WorkspacePath, WorkspaceSource,
    };

    #[tokio::test]
    async fn workspace_views_cache_inspected_semantic_and_runtime_handles() {
        let tree = TempDir::new().expect("tree tempdir");
        write_workspace(&tree.path().join("workspaces/payments")).await;
        let cache = StageCache::new();
        let selector = cached_workspace_source(
            TreeSource::local_folder(tree.path()).await.unwrap(),
            TreeRevision::LocalWorkingTree,
            "workspaces/payments",
        );

        let inspected = cache
            .get_inspected_workspace(selector.clone(), "")
            .await
            .unwrap();
        let inspected_again = cache
            .get_inspected_workspace(selector.clone(), "")
            .await
            .unwrap();
        let semantic = cache
            .get_semantic_workspace(selector.clone(), "")
            .await
            .unwrap();
        let semantic_again = cache
            .get_semantic_workspace(selector.clone(), "")
            .await
            .unwrap();
        let runtime = cache
            .get_runtime_workspace(selector.clone(), "")
            .await
            .unwrap();
        let runtime_again = cache.get_runtime_workspace(selector, "").await.unwrap();

        assert!(Arc::ptr_eq(&inspected, &inspected_again));
        assert!(Arc::ptr_eq(&inspected, &semantic.workspace));
        assert!(Arc::ptr_eq(&semantic.model, &semantic_again.model));
        assert!(Arc::ptr_eq(&runtime, &runtime_again));
    }

    #[tokio::test]
    async fn workspace_invalidation_drops_only_the_selected_view() {
        let tree = TempDir::new().expect("tree tempdir");
        write_workspace(&tree.path().join("workspaces/payments")).await;
        write_workspace(&tree.path().join("workspaces/search")).await;
        let cache = StageCache::new();
        let source_tree = TreeSource::local_folder(tree.path()).await.unwrap();
        let payments = cached_workspace_source(
            source_tree.clone(),
            TreeRevision::LocalWorkingTree,
            "workspaces/payments",
        );
        let search = cached_workspace_source(
            source_tree,
            TreeRevision::LocalWorkingTree,
            "workspaces/search",
        );

        let payments_before = cache
            .get_inspected_workspace(payments.clone(), "")
            .await
            .unwrap();
        let search_before = cache
            .get_inspected_workspace(search.clone(), "")
            .await
            .unwrap();

        cache.invalidate_workspace(&payments).await;

        let payments_after = cache.get_inspected_workspace(payments, "").await.unwrap();
        let search_after = cache.get_inspected_workspace(search, "").await.unwrap();
        assert!(!Arc::ptr_eq(&payments_before, &payments_after));
        assert!(Arc::ptr_eq(&search_before, &search_after));
    }

    #[tokio::test]
    async fn staged_source_trees_are_cached_by_revision() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_workspace(repo.path()).await;
        commit_all(repo.path(), "add root workspace");

        let cache = StageCache::new();
        let cached_tree = CachedTreeSource::new(
            "user_123",
            TreeSource::GitRemote {
                remote_url: format!("git+file://{}", repo.path().display()),
            },
            TokenIdentity::none(),
        )
        .unwrap();
        let revision = TreeRevision::GitRef(GitRefName::new("main").unwrap());

        let first = cache
            .get_staged_source_tree(cached_tree.clone(), revision.clone())
            .await
            .unwrap();
        let second = cache
            .get_staged_source_tree(cached_tree, revision)
            .await
            .unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.root().join("rototo-workspace.toml").is_file());
    }

    #[tokio::test]
    async fn branch_invalidation_drops_branch_views_but_keeps_base_views() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_workspace(repo.path()).await;
        commit_all(repo.path(), "add root workspace");
        run_git(repo.path(), &["checkout", "-b", "feature/payments"]);
        write_workspace(&repo.path().join("workspaces/payments")).await;
        commit_all(repo.path(), "add payments workspace");
        run_git(repo.path(), &["checkout", "main"]);

        let cache = StageCache::new();
        let tree = TreeSource::GitRemote {
            remote_url: format!("git+file://{}", repo.path().display()),
        };
        let cached_tree =
            CachedTreeSource::new("user_123", tree.clone(), TokenIdentity::none()).unwrap();
        let base = cached_workspace_source(
            tree.clone(),
            TreeRevision::GitRef(GitRefName::new("main").unwrap()),
            ".",
        );
        let branch = cached_workspace_source(
            tree,
            TreeRevision::git_branch("feature/payments").unwrap(),
            ".",
        );

        let base_before = cache
            .get_inspected_workspace(base.clone(), "")
            .await
            .unwrap();
        let branch_before = cache
            .get_inspected_workspace(branch.clone(), "")
            .await
            .unwrap();
        let base_tree_before = cache
            .get_staged_source_tree(
                cached_tree.clone(),
                TreeRevision::GitRef(GitRefName::new("main").unwrap()),
            )
            .await
            .unwrap();
        let branch_tree_before = cache
            .get_staged_source_tree(
                cached_tree.clone(),
                TreeRevision::git_branch("feature/payments").unwrap(),
            )
            .await
            .unwrap();

        cache
            .invalidate_branch(&cached_tree, "feature/payments")
            .await;

        let base_after = cache.get_inspected_workspace(base, "").await.unwrap();
        let branch_after = cache.get_inspected_workspace(branch, "").await.unwrap();
        let base_tree_after = cache
            .get_staged_source_tree(
                cached_tree.clone(),
                TreeRevision::GitRef(GitRefName::new("main").unwrap()),
            )
            .await
            .unwrap();
        let branch_tree_after = cache
            .get_staged_source_tree(
                cached_tree,
                TreeRevision::git_branch("feature/payments").unwrap(),
            )
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&base_before, &base_after));
        assert!(!Arc::ptr_eq(&branch_before, &branch_after));
        assert!(Arc::ptr_eq(&base_tree_before, &base_tree_after));
        assert!(!Arc::ptr_eq(&branch_tree_before, &branch_tree_after));
    }

    #[tokio::test]
    async fn local_branch_invalidation_drops_working_tree_views_and_discovery() {
        let tree = TempDir::new().expect("tree tempdir");
        write_workspace(tree.path()).await;

        let cache = StageCache::new();
        let source_tree = TreeSource::local_folder(tree.path()).await.unwrap();
        let cached_tree =
            CachedTreeSource::new("user_123", source_tree.clone(), TokenIdentity::none()).unwrap();
        let selector = cached_workspace_source(source_tree, TreeRevision::LocalWorkingTree, ".");

        let workspace_before = cache
            .get_inspected_workspace(selector.clone(), "")
            .await
            .unwrap();
        let discovery_before = cache
            .discover_workspaces(cached_tree.clone(), TreeRevision::LocalWorkingTree)
            .await
            .unwrap();
        let staged_before = cache
            .get_staged_source_tree(cached_tree.clone(), TreeRevision::LocalWorkingTree)
            .await
            .unwrap();
        let staged_cached = cache
            .get_staged_source_tree(cached_tree.clone(), TreeRevision::LocalWorkingTree)
            .await
            .unwrap();
        write_workspace(&tree.path().join("workspaces/payments")).await;
        let cached_discovery = cache
            .discover_workspaces(cached_tree.clone(), TreeRevision::LocalWorkingTree)
            .await
            .unwrap();

        cache.invalidate_branch(&cached_tree, "feature/local").await;
        let workspace_after = cache
            .get_inspected_workspace(selector.clone(), "")
            .await
            .unwrap();
        let discovery_after = cache
            .discover_workspaces(cached_tree.clone(), TreeRevision::LocalWorkingTree)
            .await
            .unwrap();
        let staged_after = cache
            .get_staged_source_tree(cached_tree, TreeRevision::LocalWorkingTree)
            .await
            .unwrap();

        assert_eq!(
            workspace_path_strings(&discovery_before.workspaces),
            vec!["."]
        );
        assert_eq!(
            workspace_path_strings(&cached_discovery.workspaces),
            vec!["."]
        );
        assert_eq!(
            workspace_path_strings(&discovery_after.workspaces),
            vec![".", "workspaces/payments"]
        );
        assert!(!Arc::ptr_eq(&workspace_before, &workspace_after));
        assert!(Arc::ptr_eq(&staged_before, &staged_cached));
        assert!(!Arc::ptr_eq(&staged_before, &staged_after));
    }

    #[tokio::test]
    async fn branch_invalidation_drops_cached_branch_changes() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_workspace(repo.path()).await;
        commit_all(repo.path(), "add root workspace");
        run_git(repo.path(), &["checkout", "-b", "feature/payments"]);
        tokio::fs::write(
            repo.path().join("variables/checkout.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();
        commit_all(repo.path(), "change checkout");

        let cache = StageCache::new();
        let tree = TreeSource::GitRemote {
            remote_url: format!("git+file://{}", repo.path().display()),
        };
        let cached_tree =
            CachedTreeSource::new("user_123", tree.clone(), TokenIdentity::none()).unwrap();
        let branch = BranchName::new("feature/payments").unwrap();
        let base_ref = GitRefName::new("main").unwrap();

        let first = cache
            .get_branch_changes(cached_tree.clone(), branch.clone(), base_ref.clone())
            .await
            .unwrap();
        tokio::fs::write(
            repo.path().join("variables/search.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();
        commit_all(repo.path(), "change search");

        let cached = cache
            .get_branch_changes(cached_tree.clone(), branch.clone(), base_ref.clone())
            .await
            .unwrap();
        cache
            .invalidate_branch(&cached_tree, "feature/payments")
            .await;
        let refreshed = cache
            .get_branch_changes(cached_tree, branch, base_ref)
            .await
            .unwrap();

        assert_eq!(
            repo_path_strings(&first.changed_files),
            vec!["variables/checkout.toml"]
        );
        assert_eq!(
            repo_path_strings(&cached.changed_files),
            vec!["variables/checkout.toml"]
        );
        assert_eq!(
            repo_path_strings(&refreshed.changed_files),
            vec!["variables/checkout.toml", "variables/search.toml"]
        );
    }

    async fn write_workspace(path: &Path) {
        tokio::fs::create_dir_all(path.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(path.join("rototo-workspace.toml"), "schema_version = 1\n")
            .await
            .unwrap();
        tokio::fs::write(
            path.join("variables/checkout.toml"),
            r#"
schema_version = 1
type = "bool"

[values]
enabled = true

[resolve]
default = "enabled"
"#
            .trim_start(),
        )
        .await
        .unwrap();
    }

    fn cached_workspace_source(
        tree: TreeSource,
        revision: TreeRevision,
        path: &str,
    ) -> CachedWorkspaceSource {
        CachedWorkspaceSource::new(
            "user_123",
            WorkspaceSource::new(tree, revision, WorkspacePath::new(path).unwrap()),
            TokenIdentity::none(),
        )
        .unwrap()
    }

    fn repo_path_strings(paths: &[RepoRelativePath]) -> Vec<&str> {
        paths.iter().map(RepoRelativePath::as_str).collect()
    }

    fn workspace_path_strings(paths: &[WorkspacePath]) -> Vec<&str> {
        paths.iter().map(WorkspacePath::as_str).collect()
    }

    fn init_repo(path: &Path) {
        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "console@example.com"]);
        run_git(path, &["config", "user.name", "Console Test"]);
    }

    fn commit_all(path: &Path, message: &str) {
        run_git(path, &["add", "."]);
        run_git(path, &["commit", "-m", message]);
    }

    fn run_git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .expect("run git");
        assert!(status.success(), "git {:?} failed", args);
    }
}
