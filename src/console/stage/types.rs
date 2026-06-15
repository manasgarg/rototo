use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tempfile::TempDir;
use tokio::sync::Mutex;

use crate::lint::WorkspaceSemanticModel;
use crate::sdk::Workspace;

/// Cached loaded view over a staged workspace source.
///
/// A view can be an inspect handle, semantic-model source, or runtime handle.
/// It stays in a view slot until it is invalidated or refreshed; the keep-alive
/// fields make any borrowed temp directories survive as long as the view does.
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

/// Cached source artifact such as a git checkout or extracted archive.
///
/// Artifact entries sit below views. They are refreshed independently so an
/// unchanged remote ref can keep existing views stable while changed refs get a
/// new handle and fingerprint.
pub(super) struct ArtifactEntry {
    pub handle: Arc<ArtifactHandle>,
    pub staged_at: Instant,
    pub revalidating: bool,
}

/// Handle to staged source files plus their refresh identity.
///
/// The handle owns or references the temp directory that contains source files.
/// Cloning the `Arc<ArtifactHandle>` keeps those files alive for every view or
/// language-server session that still points at them.
pub(super) struct ArtifactHandle {
    pub identity: String,
    pub root: PathBuf,
    pub fingerprint: String,
    pub immutable: bool,
    pub _keep_alive: ArtifactKeepAlive,
}

/// Ownership strategy for an artifact's temporary files.
///
/// Git artifacts keep both the bare repo store and a checkout tempdir alive.
/// Archive artifacts only need the extraction tempdir. Dropping the handle
/// releases the corresponding temp directories.
pub(super) enum ArtifactKeepAlive {
    Git {
        _repo: Arc<GitRepoStore>,
        _checkout: Arc<TempDir>,
    },
    Archive {
        _tempdir: Arc<TempDir>,
    },
}

/// Bare git repository cache for one authenticated remote.
///
/// The cache lets multiple refs share fetch state. The lock serializes fetches
/// into the bare repo, and the tempdir is deleted when the last artifact/cache
/// reference drops.
pub(super) struct GitRepoStore {
    pub bare_dir: PathBuf,
    pub _tempdir: Arc<TempDir>,
    pub lock: Mutex<()>,
}

/// Mutable slot for one cached view.
///
/// The outer `Arc` is shared by concurrent requests and background refresh
/// tasks; the `Option` lets invalidation or failed staging clear the entry.
pub(super) type ViewSlot = Arc<Mutex<Option<ViewEntry>>>;
/// Mutable slot for one cached artifact.
///
/// It has the same lifecycle as `ViewSlot`, but stores artifact handles below
/// loaded workspace views.
pub(super) type ArtifactSlot = Arc<Mutex<Option<ArtifactEntry>>>;
/// Mutable slot for one bare git repository cache.
///
/// All artifact refreshes for the same authenticated remote coordinate through
/// this slot before creating per-ref checkouts.
pub(super) type GitRepoSlot = Arc<Mutex<Option<Arc<GitRepoStore>>>>;

#[derive(Clone)]
/// Newly staged view returned before insertion into the cache.
///
/// Staging code builds this transfer object with the loaded workspace and the
/// handles needed to keep backing files alive. `StageCache` then converts it
/// into a durable `ViewEntry`.
pub(super) struct ViewStage {
    pub workspace: Arc<Workspace>,
    pub keep_alive: Option<Arc<Workspace>>,
    pub artifact: Option<Arc<ArtifactHandle>>,
}

/// Result of refreshing a cached artifact.
///
/// `Unchanged` keeps dependent view keys stable; `Changed` carries a new
/// fingerprint so subsequent views stage against fresh files.
pub(super) enum ArtifactRefresh {
    Unchanged(Arc<ArtifactHandle>),
    Changed(Arc<ArtifactHandle>),
}
