//! Pin-keyed staging (`design/console-git-ops.md`, rule 2: cache by commit,
//! not by branch). A pin is a full commit SHA; pinned content can never go
//! stale, so this store has no invalidation logic at all. Entries are built
//! once with a shallow fetch of one commit, reused from disk afterwards, and
//! evicted only when the store outgrows its size budget.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use tokio::process::Command;

use crate::error::{Result, RototoError};

use super::git::{is_full_git_commit, scrub_git_process_variables};
use super::types::SourceOptions;

/// Entries used more recently than this are safe from eviction, so a caller
/// that just staged a pin can read the returned tree without racing the
/// size budget. Eviction is a capacity concern, never a correctness one.
const EVICTION_GRACE: Duration = Duration::from_secs(60);

/// A size-bounded, on-disk store of staged git trees keyed by
/// `(remote, pin)`.
pub struct PinStore {
    root: PathBuf,
    max_bytes: u64,
    /// Per-key build locks: one shallow fetch per pin even under
    /// concurrent requests in this process.
    building: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl PinStore {
    /// A store rooted at `root` (created on first use), evicting
    /// least-recently-used pins once the staged trees exceed `max_bytes`.
    pub fn new(root: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Self {
            root: root.into(),
            max_bytes,
            building: Mutex::new(HashMap::new()),
        }
    }

    /// The staged tree for `(remote, pin)`: a directory holding exactly the
    /// files at that commit. `pin` must be a full commit SHA; resolving a
    /// branch name to a pin is the caller's job, done early, so everything
    /// below the resolution works on content that cannot change.
    pub async fn stage(&self, remote: &str, pin: &str, options: &SourceOptions) -> Result<PathBuf> {
        if !is_full_git_commit(pin) {
            return Err(RototoError::new(format!(
                "the pin store is keyed by commit; `{pin}` is not a full commit SHA"
            )));
        }
        let key = entry_key(remote, pin);
        let entry_dir = self.root.join(&key);
        let tree = entry_dir.join("tree");

        let lock = self.build_lock(&key);
        let guard = lock.lock().await;
        if !is_dir(&tree).await {
            self.build(remote, pin, &entry_dir, options).await?;
        }
        touch(&entry_dir.join("used")).await;
        drop(guard);

        self.evict_to_budget(&key).await;
        Ok(tree)
    }

    fn build_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut building = self.building.lock().expect("pin store lock poisoned");
        building
            .entry(key.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    async fn build(
        &self,
        remote: &str,
        pin: &str,
        entry_dir: &Path,
        options: &SourceOptions,
    ) -> Result<()> {
        tokio::fs::create_dir_all(&self.root).await.map_err(|err| {
            RototoError::new(format!(
                "failed to create pin store {}: {err}",
                self.root.display()
            ))
        })?;
        let staging = tempfile::Builder::new()
            .prefix(".build-")
            .tempdir_in(&self.root)
            .map_err(|err| RototoError::new(format!("failed to create staging dir: {err}")))?;
        let tree = staging.path().join("tree");
        tokio::fs::create_dir_all(&tree)
            .await
            .map_err(|err| RototoError::new(format!("failed to create staging tree: {err}")))?;

        run_git(None, &["init", "--quiet", &path_str(&tree)?], options).await?;
        // Shallow fetch of exactly one commit. Servers that refuse SHA
        // wants (GitHub allows them) get a full fetch of every ref instead;
        // the checkout below verifies the pin is actually present.
        let shallow = run_git(
            Some(&tree),
            &["fetch", "--quiet", "--depth=1", remote, pin],
            options,
        )
        .await;
        if shallow.is_err() {
            run_git(
                Some(&tree),
                &["fetch", "--quiet", remote, "+refs/*:refs/pins/*"],
                options,
            )
            .await?;
        }
        run_git(Some(&tree), &["checkout", "--quiet", pin], options).await?;
        tokio::fs::remove_dir_all(tree.join(".git"))
            .await
            .map_err(|err| RototoError::new(format!("failed to strip staged repo: {err}")))?;

        let size = dir_size(&tree).await?;
        tokio::fs::write(staging.path().join("size"), size.to_string())
            .await
            .map_err(|err| RototoError::new(format!("failed to record entry size: {err}")))?;
        tokio::fs::write(staging.path().join("remote"), remote)
            .await
            .map_err(|err| RototoError::new(format!("failed to record entry remote: {err}")))?;
        touch(&staging.path().join("used")).await;

        let staged = staging.keep();
        match tokio::fs::rename(&staged, entry_dir).await {
            Ok(()) => Ok(()),
            Err(err) => {
                let _ = tokio::fs::remove_dir_all(&staged).await;
                if is_dir(&entry_dir.join("tree")).await {
                    // Another process staged the same pin first; identical
                    // content, so theirs is as good as ours.
                    Ok(())
                } else {
                    Err(RototoError::new(format!(
                        "failed to move staged pin into place: {err}"
                    )))
                }
            }
        }
    }

    /// Removes least-recently-used entries until the store fits its budget.
    /// Best-effort: eviction failures leave extra bytes behind, never wrong
    /// content. The just-staged entry and anything used within the grace
    /// window stay.
    async fn evict_to_budget(&self, keep: &str) {
        let Ok(mut dir) = tokio::fs::read_dir(&self.root).await else {
            return;
        };
        let mut entries: Vec<(String, PathBuf, u64, SystemTime)> = Vec::new();
        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if !is_dir(&path).await {
                continue;
            }
            let size = match tokio::fs::read_to_string(path.join("size")).await {
                Ok(size) => size.trim().parse::<u64>().unwrap_or(0),
                Err(_) => dir_size(&path.join("tree")).await.unwrap_or(0),
            };
            let used = tokio::fs::metadata(path.join("used"))
                .await
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((name, path, size, used));
        }

        let mut total: u64 = entries.iter().map(|(_, _, size, _)| *size).sum();
        if total <= self.max_bytes {
            return;
        }
        entries.sort_by_key(|(_, _, _, used)| *used);
        let now = SystemTime::now();
        for (name, path, size, used) in entries {
            if total <= self.max_bytes {
                break;
            }
            if name == keep {
                continue;
            }
            if now
                .duration_since(used)
                .map(|age| age < EVICTION_GRACE)
                .unwrap_or(true)
            {
                continue;
            }
            if tokio::fs::remove_dir_all(&path).await.is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }
}

/// The cache key: a digest of the remote plus the pin itself, so one store
/// serves many repositories without their pins colliding.
fn entry_key(remote: &str, pin: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, remote.as_bytes());
    let prefix: String = digest.as_ref()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{prefix}-{pin}")
}

async fn run_git(dir: Option<&Path>, args: &[&str], options: &SourceOptions) -> Result<String> {
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.args(args);
    scrub_git_process_variables(&mut command);
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new(format!("git {} timed out", args[0])))?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git {} failed: {}",
            args[0],
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn dir_size(root: &Path) -> Result<u64> {
    let mut total = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(current) = pending.pop() {
        let mut dir = tokio::fs::read_dir(&current).await.map_err(|err| {
            RototoError::new(format!("failed to read {}: {err}", current.display()))
        })?;
        while let Some(entry) = dir.next_entry().await.map_err(|err| {
            RototoError::new(format!("failed to read {}: {err}", current.display()))
        })? {
            let meta = entry.metadata().await.map_err(|err| {
                RototoError::new(format!(
                    "failed to inspect {}: {err}",
                    entry.path().display()
                ))
            })?;
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

async fn is_dir(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
}

async fn touch(path: &Path) {
    let _ = tokio::fs::write(path, b"").await;
}

fn path_str(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| RototoError::new(format!("path is not valid UTF-8: {}", path.display())))
}
