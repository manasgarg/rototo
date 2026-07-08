use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Result, RototoError};

/// The package collections the engine can see. Walking only these keeps a
/// snapshot to package files and out of `.git`, build output, and whatever
/// else shares the directory.
const TREE_FILES: &[&str] = &["rototo-package.toml", "governance.toml"];
const TREE_DIRS: &[&str] = &["variables", "lists", "layers", "model", "data", "lint"];

/// An in-memory snapshot of a staged package: package-relative paths (with
/// forward slashes) to file contents. The engine edits this shape, so the
/// same apply serves a local checkout, a staged pin, and a test fixture.
#[derive(Clone, Debug, Default)]
pub struct EditTree {
    files: BTreeMap<String, String>,
}

impl EditTree {
    /// Builds a tree from `(path, content)` pairs; paths are
    /// package-relative with forward slashes.
    pub fn from_files<P, C>(files: impl IntoIterator<Item = (P, C)>) -> Self
    where
        P: Into<String>,
        C: Into<String>,
    {
        Self {
            files: files
                .into_iter()
                .map(|(path, content)| (path.into(), content.into()))
                .collect(),
        }
    }

    /// Snapshots the package files under `root`. A missing root snapshots as
    /// an empty tree, so creation into a fresh directory works.
    pub async fn snapshot(root: &Path) -> Result<Self> {
        let mut files = BTreeMap::new();
        for name in TREE_FILES {
            if let Some(content) = read_optional(&root.join(name)).await? {
                files.insert((*name).to_owned(), content);
            }
        }
        for dir in TREE_DIRS {
            let mut pending = vec![root.join(dir)];
            while let Some(current) = pending.pop() {
                let mut entries = match tokio::fs::read_dir(&current).await {
                    Ok(entries) => entries,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => {
                        return Err(RototoError::new(format!(
                            "failed to read directory {}: {err}",
                            current.display()
                        )));
                    }
                };
                while let Some(entry) = entries.next_entry().await.map_err(|err| {
                    RototoError::new(format!(
                        "failed to read directory {}: {err}",
                        current.display()
                    ))
                })? {
                    let path = entry.path();
                    let file_type = entry.file_type().await.map_err(|err| {
                        RototoError::new(format!("failed to inspect {}: {err}", path.display()))
                    })?;
                    if file_type.is_dir() {
                        pending.push(path);
                    } else if file_type.is_file()
                        && let Some(relative) = relative_path(root, &path)
                        && let Some(content) = read_optional(&path).await?
                    {
                        files.insert(relative, content);
                    }
                }
            }
        }
        Ok(Self { files })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// Drops a file from the snapshot. Callers that intend to overwrite a
    /// file (`rototo init --force`) mask it first so `create_*` operations
    /// see a fresh slot; whether the overwrite is allowed stays the
    /// caller's decision.
    pub fn remove(&mut self, path: &str) {
        self.files.remove(path);
    }

    pub fn content(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
}

async fn read_optional(path: &PathBuf) -> Result<Option<String>> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        // A package file the engine cannot read as text is a real problem
        // only when an operation touches it; refusing the snapshot outright
        // would block edits to everything else. Binary files have no
        // business in the collections we walk, so surface the error.
        Err(err) => Err(RototoError::new(format!(
            "failed to read {}: {err}",
            path.display()
        ))),
    }
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        parts.push(component.as_os_str().to_str()?.to_owned());
    }
    Some(parts.join("/"))
}
