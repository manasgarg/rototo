use std::path::{Component, Path};

pub(super) async fn path_containment_error(root: &Path, path: &Path) -> Option<String> {
    let root = tokio::fs::canonicalize(root).await.ok()?;
    let path = tokio::fs::canonicalize(path).await.ok()?;
    if path.starts_with(&root) {
        None
    } else {
        Some("path escapes workspace".to_owned())
    }
}

pub(crate) fn workspace_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}
