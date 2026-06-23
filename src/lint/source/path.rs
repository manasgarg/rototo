use std::path::{Component, Path};

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
