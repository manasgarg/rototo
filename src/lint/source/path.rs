use std::path::{Component, Path, PathBuf};

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

pub(crate) fn resolve_workspace_relative_path(
    document_path: &str,
    reference: &str,
) -> Option<String> {
    let reference = Path::new(reference);
    if reference.as_os_str().is_empty() || reference.is_absolute() {
        return None;
    }

    let base = Path::new(document_path).parent().unwrap_or(Path::new(""));
    let mut normalized = PathBuf::new();
    for component in base.join(reference).components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(workspace_path(&normalized))
    }
}

pub(crate) fn resolve_workspace_root_path(reference: &str) -> Option<String> {
    let reference = Path::new(reference);
    if reference.as_os_str().is_empty() || reference.is_absolute() {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in reference.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(workspace_path(&normalized))
    }
}

pub(super) fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}
