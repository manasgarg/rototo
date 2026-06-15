use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tempfile::TempDir;
use tokio::sync::Mutex;

use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;

pub(super) struct ViewEntry {
    pub workspace: Arc<Workspace>,
    /// Runtime handles compile in memory but resolve against the staged
    /// inspect checkout; keeping that handle alive keeps any borrowed tempdir
    /// alive.
    pub _keep_alive: Option<Arc<Workspace>>,
    /// Artifact-backed views borrow a root under this artifact checkout or
    /// extraction. Holding the handle keeps that root stable for this view.
    pub _artifact: Option<Arc<ArtifactHandle>>,
    /// One semantic model computation per staged inspect checkout.
    pub model: Arc<tokio::sync::OnceCell<Arc<WorkspaceSemanticModel>>>,
    pub staged_at: Instant,
    pub revalidating: bool,
}

pub(super) struct ArtifactEntry {
    pub handle: Arc<ArtifactHandle>,
    pub staged_at: Instant,
    pub revalidating: bool,
}

pub(super) struct ArtifactHandle {
    pub identity: String,
    pub root: PathBuf,
    pub fingerprint: String,
    pub immutable: bool,
    pub _keep_alive: ArtifactKeepAlive,
}

pub(super) enum ArtifactKeepAlive {
    Git {
        _repo: Arc<GitRepoStore>,
        _checkout: Arc<TempDir>,
    },
    Archive {
        _tempdir: Arc<TempDir>,
    },
}

pub(super) struct GitRepoStore {
    pub bare_dir: PathBuf,
    pub _tempdir: Arc<TempDir>,
    pub lock: Mutex<()>,
}

pub(super) type ViewSlot = Arc<Mutex<Option<ViewEntry>>>;
pub(super) type ArtifactSlot = Arc<Mutex<Option<ArtifactEntry>>>;
pub(super) type GitRepoSlot = Arc<Mutex<Option<Arc<GitRepoStore>>>>;

#[derive(Clone)]
pub(super) struct ViewStage {
    pub workspace: Arc<Workspace>,
    pub keep_alive: Option<Arc<Workspace>>,
    pub artifact: Option<Arc<ArtifactHandle>>,
}

pub(super) enum ArtifactRefresh {
    Unchanged(Arc<ArtifactHandle>),
    Changed(Arc<ArtifactHandle>),
}
