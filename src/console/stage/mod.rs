use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::error::{Result, RototoError};
use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;
use crate::source::SourceOptions;

mod archive;
mod git;
mod path;
mod source;
mod types;

use self::archive::stage_archive_artifact;
use self::git::{run_git, stage_git_artifact};
use self::path::artifact_workspace_root;
use self::source::{ParsedSource, auth, invalidation_markers, parse_source, token_key, view_key};
use self::types::{
    ArtifactEntry, ArtifactHandle, ArtifactRefresh, ArtifactSlot, GitRepoSlot, GitRepoStore,
    ViewEntry, ViewSlot, ViewStage,
};

/// Staged workspace views serve stale-while-revalidate: after the fresh window,
/// the cached handle is returned immediately and background work refreshes the
/// underlying artifact. Saves invalidate their source so draft screens see new
/// branch content immediately.
const STAGE_FRESH: Duration = Duration::from_secs(30);

/// Shared cache for staged workspace artifacts and loaded workspace views.
///
/// The cache lives for the console process and is cloned into request handlers.
/// Artifact slots own temporary git checkouts or archive extractions; view
/// slots own `Workspace` handles over those artifacts. Entries are refreshed
/// stale-while-revalidate and are dropped when a draft save invalidates the
/// source.
#[derive(Clone, Default)]
pub struct StageCache {
    artifacts: Arc<Mutex<HashMap<String, ArtifactSlot>>>,
    views: Arc<Mutex<HashMap<String, ViewSlot>>>,
    git_repos: Arc<Mutex<HashMap<String, GitRepoSlot>>>,
}

impl StageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// An inspect-level handle (staged files + inspection, no lint gate).
    pub async fn inspect(&self, token: &str, source: &str) -> Result<Arc<Workspace>> {
        Ok(self.inspect_entry(token, source).await?.0)
    }

    /// The staged inspect handle plus its semantic model, computed once per
    /// staged checkout.
    pub async fn semantic_model(
        &self,
        token: &str,
        source: &str,
    ) -> Result<(Arc<Workspace>, Arc<WorkspaceSemanticModel>)> {
        let (workspace, model_cell) = self.inspect_entry(token, source).await?;
        let started = Instant::now();
        let model = model_cell
            .get_or_try_init(|| async { workspace.semantic_model().await.map(Arc::new) })
            .await?
            .clone();
        tracing::info!(
            operation = "workspace.semantic_model",
            latency_ms = started.elapsed().as_millis(),
            "console workspace semantic model ready"
        );
        Ok((workspace, model))
    }

    /// A runtime-capable handle for resolution previews. The runtime model is
    /// only compiled under lint mode "deny", so previews exist exactly when
    /// the workspace lints clean — the same workspaces applications can load.
    pub async fn runtime(&self, token: &str, source: &str) -> Result<Arc<Workspace>> {
        let cache = self.clone();
        let source_owned = source.to_owned();
        let token_owned = token.to_owned();
        let (workspace, _) = self
            .view("load", token, source, move || {
                let cache = cache.clone();
                let source = source_owned.clone();
                let token = token_owned.clone();
                async move {
                    // Reuse the staged inspect checkout instead of downloading
                    // or fetching the source a second time; loading from the
                    // local root applies the same lint-deny gate.
                    let inspected = cache.inspect(&token, &source).await?;
                    let root = inspected.root().display().to_string();
                    let runtime = Workspace::load(&root).await?;
                    Ok(ViewStage {
                        workspace: Arc::new(runtime),
                        keep_alive: Some(inspected),
                        artifact: None,
                    })
                }
            })
            .await?;
        Ok(workspace)
    }

    /// Drops every cached handle for a source, regardless of token or kind.
    pub async fn invalidate_source(&self, source: &str) {
        let markers = invalidation_markers(source);
        let mut views = self.views.lock().await;
        views.retain(|key, _| !markers.iter().any(|marker| key.contains(marker)));
        drop(views);

        let mut artifacts = self.artifacts.lock().await;
        artifacts.retain(|key, _| !markers.iter().any(|marker| key.contains(marker)));
    }

    async fn inspect_entry(
        &self,
        token: &str,
        source: &str,
    ) -> Result<(
        Arc<Workspace>,
        Arc<tokio::sync::OnceCell<Arc<WorkspaceSemanticModel>>>,
    )> {
        let source_owned = source.to_owned();
        let token_owned = token.to_owned();
        let cache = self.clone();
        self.view("inspect", token, source, move || {
            let source = source_owned.clone();
            let token = token_owned.clone();
            let cache = cache.clone();
            async move {
                let parsed = parse_source(&token, &source)?;
                let (workspace, artifact) = match parsed {
                    ParsedSource::Direct { source } => {
                        let workspace = Workspace::inspect_with_source_options(
                            &source,
                            &SourceOptions::default().with_auth(auth(&token)),
                        )
                        .await?;
                        (workspace, None)
                    }
                    ParsedSource::Git { .. } | ParsedSource::HttpsArchive { .. } => {
                        let subdir = match &parsed {
                            ParsedSource::Git { subdir, .. }
                            | ParsedSource::HttpsArchive { subdir, .. } => subdir.clone(),
                            ParsedSource::Direct { .. } => None,
                        };
                        let artifact = cache.artifact_from_parsed(&token, &source, parsed).await?;
                        let root = artifact_workspace_root(&artifact, subdir.as_deref()).await?;
                        let workspace = Workspace::inspect_with_source_options(
                            root.display().to_string(),
                            &SourceOptions::default(),
                        )
                        .await?;
                        (workspace, Some(artifact))
                    }
                };
                Ok(ViewStage {
                    workspace: Arc::new(workspace),
                    keep_alive: None,
                    artifact,
                })
            }
        })
        .await
    }

    async fn view<F, Fut>(
        &self,
        kind: &str,
        token: &str,
        source: &str,
        stage: F,
    ) -> Result<(
        Arc<Workspace>,
        Arc<tokio::sync::OnceCell<Arc<WorkspaceSemanticModel>>>,
    )>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ViewStage>> + Send + 'static,
    {
        let parsed = parse_source(token, source)?;
        let mut key_artifact = None;
        let key = match &parsed {
            ParsedSource::Direct { source } => {
                format!("view:{kind}:direct:{}:{source}", token_key(token))
            }
            ParsedSource::Git { subdir, .. } | ParsedSource::HttpsArchive { subdir, .. } => {
                let artifact = self
                    .artifact_from_parsed(token, source, parsed.clone())
                    .await?;
                let key = view_key(kind, &artifact, subdir.as_deref());
                key_artifact = Some(artifact);
                key
            }
        };

        let slot = self.view_slot(&key).await;
        let mut guard = slot.lock().await;
        if let Some(entry) = guard.as_mut() {
            if matches!(parsed, ParsedSource::Direct { .. })
                && entry.staged_at.elapsed() >= STAGE_FRESH
                && !entry.revalidating
            {
                entry.revalidating = true;
                let slot = slot.clone();
                let restage = stage();
                tokio::spawn(async move {
                    let fresh = restage.await;
                    let mut guard = slot.lock().await;
                    match (fresh, guard.as_mut()) {
                        (Ok(fresh), _) => {
                            *guard = Some(ViewEntry {
                                workspace: fresh.workspace,
                                _keep_alive: fresh.keep_alive,
                                _artifact: fresh.artifact,
                                model: Arc::new(tokio::sync::OnceCell::new()),
                                staged_at: Instant::now(),
                                revalidating: false,
                            });
                        }
                        (Err(_), Some(entry)) => entry.revalidating = false,
                        (Err(_), None) => {}
                    }
                });
            }
            tracing::info!(
                operation = "workspace.stage",
                stage_kind = kind,
                cache = "hit",
                "console workspace stage cache hit"
            );
            return Ok((entry.workspace.clone(), entry.model.clone()));
        }

        let started = Instant::now();
        let staged = stage().await?;
        tracing::info!(
            operation = "workspace.stage",
            stage_kind = kind,
            cache = "miss",
            latency_ms = started.elapsed().as_millis(),
            "console workspace staged"
        );
        let model = Arc::new(tokio::sync::OnceCell::new());
        *guard = Some(ViewEntry {
            workspace: staged.workspace.clone(),
            _keep_alive: staged.keep_alive,
            _artifact: staged.artifact.or(key_artifact),
            model: model.clone(),
            staged_at: Instant::now(),
            revalidating: false,
        });
        Ok((staged.workspace, model))
    }

    async fn artifact_from_parsed(
        &self,
        token: &str,
        source: &str,
        parsed: ParsedSource,
    ) -> Result<Arc<ArtifactHandle>> {
        let (key, marker) = match &parsed {
            ParsedSource::Direct { .. } => {
                return Err(RototoError::new(
                    "direct sources do not use artifact staging",
                ));
            }
            ParsedSource::Git {
                artifact_key,
                invalidation_marker,
                ..
            }
            | ParsedSource::HttpsArchive {
                artifact_key,
                invalidation_marker,
                ..
            } => (artifact_key.clone(), invalidation_marker.clone()),
        };
        let slot = self.artifact_slot(&key).await;
        let mut guard = slot.lock().await;
        if let Some(entry) = guard.as_mut() {
            if entry.staged_at.elapsed() >= STAGE_FRESH
                && !entry.revalidating
                && !entry.handle.immutable
            {
                entry.revalidating = true;
                let slot = slot.clone();
                let cache = self.clone();
                let token = token.to_owned();
                let source = source.to_owned();
                let parsed = parsed.clone();
                let identity = key.clone();
                let previous = entry.handle.clone();
                tokio::spawn(async move {
                    let refreshed = cache
                        .refresh_artifact(&token, &source, parsed, identity, Some(previous))
                        .await;
                    let mut guard = slot.lock().await;
                    match (refreshed, guard.as_mut()) {
                        (Ok(ArtifactRefresh::Unchanged(handle)), Some(entry)) => {
                            entry.handle = handle;
                            entry.staged_at = Instant::now();
                            entry.revalidating = false;
                        }
                        (Ok(ArtifactRefresh::Changed(handle)), _) => {
                            *guard = Some(ArtifactEntry {
                                handle,
                                staged_at: Instant::now(),
                                revalidating: false,
                            });
                        }
                        (Ok(ArtifactRefresh::Unchanged(_)), None) => {}
                        (Err(_), Some(entry)) => entry.revalidating = false,
                        (Err(_), None) => {}
                    }
                });
            }
            tracing::info!(
                operation = "workspace.artifact",
                cache = "hit",
                source = %marker,
                "console workspace artifact cache hit"
            );
            return Ok(entry.handle.clone());
        }

        let started = Instant::now();
        let refreshed = self
            .refresh_artifact(token, source, parsed, key.clone(), None)
            .await?;
        let handle = match refreshed {
            ArtifactRefresh::Unchanged(handle) | ArtifactRefresh::Changed(handle) => handle,
        };
        tracing::info!(
            operation = "workspace.artifact",
            cache = "miss",
            source = %marker,
            latency_ms = started.elapsed().as_millis(),
            "console workspace artifact staged"
        );
        *guard = Some(ArtifactEntry {
            handle: handle.clone(),
            staged_at: Instant::now(),
            revalidating: false,
        });
        Ok(handle)
    }

    async fn refresh_artifact(
        &self,
        token: &str,
        source: &str,
        parsed: ParsedSource,
        identity: String,
        previous: Option<Arc<ArtifactHandle>>,
    ) -> Result<ArtifactRefresh> {
        match parsed {
            ParsedSource::Git {
                remote,
                ref_,
                repo_key,
                ..
            } => {
                let repo = self.git_repo(&repo_key).await?;
                stage_git_artifact(repo, token, &remote, ref_.as_deref(), identity, previous).await
            }
            ParsedSource::HttpsArchive { url, .. } => {
                stage_archive_artifact(token, &url, source, identity, previous).await
            }
            ParsedSource::Direct { .. } => Err(RototoError::new(
                "direct sources do not use artifact staging",
            )),
        }
    }

    async fn git_repo(&self, key: &str) -> Result<Arc<GitRepoStore>> {
        let slot = {
            let mut repos = self.git_repos.lock().await;
            repos.entry(key.to_owned()).or_default().clone()
        };
        let mut guard = slot.lock().await;
        if let Some(repo) = guard.as_ref() {
            return Ok(repo.clone());
        }
        let tempdir = Arc::new(
            TempDir::new()
                .map_err(|err| RototoError::new(format!("failed to create git cache: {err}")))?,
        );
        let bare_dir = tempdir.path().join("repo.git");
        let mut command = Command::new("git");
        command.arg("init").arg("--bare").arg(&bare_dir);
        run_git(&mut command, Duration::from_secs(60)).await?;
        let repo = Arc::new(GitRepoStore {
            bare_dir,
            _tempdir: tempdir,
            lock: Mutex::new(()),
        });
        *guard = Some(repo.clone());
        Ok(repo)
    }

    async fn artifact_slot(&self, key: &str) -> ArtifactSlot {
        let mut artifacts = self.artifacts.lock().await;
        artifacts.entry(key.to_owned()).or_default().clone()
    }

    async fn view_slot(&self, key: &str) -> ViewSlot {
        let mut views = self.views.lock().await;
        views.entry(key.to_owned()).or_default().clone()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::StageCache;

    fn run_repo_git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap_or_else(|err| panic!("failed to run git {}: {err}", args.join(" ")));
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn git_artifact_is_shared_across_workspace_subdirs() {
        let repo = TempDir::new().expect("repo tempdir");
        tokio::fs::create_dir_all(repo.path().join("payments/variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(repo.path().join("checkout/variables"))
            .await
            .unwrap();
        tokio::fs::write(
            repo.path().join("payments/rototo-workspace.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            repo.path().join("checkout/rototo-workspace.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();
        run_repo_git(repo.path(), &["init"]);
        run_repo_git(repo.path(), &["config", "user.email", "rototo@example.com"]);
        run_repo_git(repo.path(), &["config", "user.name", "Rototo Test"]);
        run_repo_git(repo.path(), &["add", "."]);
        run_repo_git(repo.path(), &["commit", "-m", "add workspaces"]);

        let cache = StageCache::new();
        let remote = format!("git+file://{}#HEAD", repo.path().display());
        let payments = cache
            .inspect("", &format!("{remote}:payments"))
            .await
            .expect("payments stages");
        let checkout = cache
            .inspect("", &format!("{remote}:checkout"))
            .await
            .expect("checkout stages");

        assert!(payments.root().ends_with("payments"));
        assert!(checkout.root().ends_with("checkout"));
        assert_eq!(cache.git_repos.lock().await.len(), 1);
        assert_eq!(cache.artifacts.lock().await.len(), 1);
    }
}
