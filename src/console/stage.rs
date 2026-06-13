use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::error::Result;
use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;
use crate::source::{SourceAuth, SourceOptions};

/// Staging a workspace source downloads and extracts the GitHub tarball, so
/// doing it per request makes every screen wait on GitHub (and burns API
/// quota). Staged handles serve stale-while-revalidate: after the fresh
/// window, the cached handle is returned immediately and a background restage
/// replaces it. Saves invalidate their source so draft screens see fresh
/// content immediately.
const STAGE_FRESH: Duration = Duration::from_secs(30);

struct Entry {
    workspace: Arc<Workspace>,
    /// Runtime handles compile in memory but resolve against the staged
    /// inspect checkout; keeping that handle alive keeps its tempdir alive.
    _keep_alive: Option<Arc<Workspace>>,
    /// One semantic model computation per staged checkout.
    model: Arc<tokio::sync::OnceCell<Arc<WorkspaceSemanticModel>>>,
    staged_at: Instant,
    revalidating: bool,
}

type Slot = Arc<Mutex<Option<Entry>>>;
type Staged = (Arc<Workspace>, Option<Arc<Workspace>>);

#[derive(Clone, Default)]
pub struct StageCache {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
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
            .entry("load", token, source, move || {
                let cache = cache.clone();
                let source = source_owned.clone();
                let token = token_owned.clone();
                async move {
                    // Reuse the staged inspect checkout instead of downloading
                    // the source a second time; loading from the local root
                    // applies the same lint-deny gate.
                    let inspected = cache.inspect(&token, &source).await?;
                    let root = inspected.root().display().to_string();
                    let runtime = Workspace::load(&root).await?;
                    Ok((Arc::new(runtime), Some(inspected)))
                }
            })
            .await?;
        Ok(workspace)
    }

    /// Drops every cached handle for a source, regardless of token or kind.
    pub async fn invalidate_source(&self, source: &str) {
        let suffix = format!(":{source}");
        let mut slots = self.slots.lock().await;
        slots.retain(|key, _| !key.ends_with(&suffix));
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
        self.entry("inspect", token, source, move || {
            let source = source_owned.clone();
            let token = token_owned.clone();
            async move {
                let workspace = Workspace::inspect_with_source_options(
                    &source,
                    &SourceOptions::default().with_auth(auth(&token)),
                )
                .await?;
                Ok((Arc::new(workspace), None))
            }
        })
        .await
    }

    async fn entry<F, Fut>(
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
        Fut: Future<Output = Result<Staged>> + Send + 'static,
    {
        let key = cache_key(kind, token, source);
        let slot = self.slot(&key).await;
        let mut guard = slot.lock().await;
        if let Some(entry) = guard.as_mut() {
            if entry.staged_at.elapsed() >= STAGE_FRESH && !entry.revalidating {
                entry.revalidating = true;
                let slot = slot.clone();
                let restage = stage();
                tokio::spawn(async move {
                    let fresh = restage.await;
                    let mut guard = slot.lock().await;
                    match (fresh, guard.as_mut()) {
                        (Ok((workspace, keep_alive)), _) => {
                            *guard = Some(Entry {
                                workspace,
                                _keep_alive: keep_alive,
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

        // First staging for this key: other callers queue on the slot lock,
        // so a burst of requests stages the source once.
        let started = Instant::now();
        let (workspace, keep_alive) = stage().await?;
        tracing::info!(
            operation = "workspace.stage",
            stage_kind = kind,
            cache = "miss",
            latency_ms = started.elapsed().as_millis(),
            "console workspace staged"
        );
        let model = Arc::new(tokio::sync::OnceCell::new());
        *guard = Some(Entry {
            workspace: workspace.clone(),
            _keep_alive: keep_alive,
            model: model.clone(),
            staged_at: Instant::now(),
            revalidating: false,
        });
        Ok((workspace, model))
    }

    async fn slot(&self, key: &str) -> Slot {
        let mut slots = self.slots.lock().await;
        slots.entry(key.to_owned()).or_default().clone()
    }
}

fn cache_key(kind: &str, token: &str, source: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, token.as_bytes());
    let token_key: String = digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()[..12]
        .to_owned();
    format!("{kind}:{token_key}:{source}")
}

fn auth(token: &str) -> SourceAuth {
    if token.is_empty() {
        SourceAuth::None
    } else {
        SourceAuth::Bearer(token.to_owned())
    }
}
