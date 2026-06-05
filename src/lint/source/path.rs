use std::path::{Component, Path, PathBuf};

pub(super) async fn path_containment_error(root: &Path, path: &Path) -> Option<String> {
    let root = match tokio::fs::canonicalize(root).await {
        Ok(root) => root,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => return Some(format!("failed to canonicalize workspace root: {err}")),
    };
    let path = match tokio::fs::canonicalize(path).await {
        Ok(path) => path,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => return Some(format!("failed to canonicalize path: {err}")),
    };
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

pub(super) fn file_uri(path: &Path) -> String {
    format!("file://{}", percent_encode_path(&path.to_string_lossy()))
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~' | b':' => {
                encoded.push(char::from(*byte))
            }
            byte => {
                use std::fmt::Write;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_percent_encodes_special_and_multibyte_paths() {
        assert_eq!(
            file_uri(Path::new("/tmp/rototo #é%.toml")),
            "file:///tmp/rototo%20%23%C3%A9%25.toml"
        );
    }

    #[test]
    fn workspace_relative_paths_normalize_without_escaping() {
        assert_eq!(
            resolve_workspace_relative_path("variables/message.toml", "../schemas/value.json"),
            Some("schemas/value.json".to_owned())
        );
        assert_eq!(
            resolve_workspace_relative_path("variables/message.toml", "../../outside.json"),
            None
        );
        assert_eq!(
            resolve_workspace_relative_path("variables/message.toml", "/tmp/outside.json"),
            None
        );
        assert_eq!(
            resolve_workspace_relative_path("variables/message.toml", ""),
            None
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn path_containment_reports_symlink_escape() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path().join("workspace");
        let outside = tempdir.path().join("outside.toml");
        tokio::fs::create_dir(&root).await.unwrap();
        tokio::fs::write(&outside, "schema_version = 1")
            .await
            .unwrap();

        let link = root.join("linked.toml");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert_eq!(
            path_containment_error(&root, &link).await,
            Some("path escapes workspace".to_owned())
        );
    }
}
