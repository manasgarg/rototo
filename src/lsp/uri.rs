use std::path::{Component, Path, PathBuf};

use serde_json::Value as JsonValue;

use crate::diagnostics::SourcePosition;
use crate::error::{Result, RototoError};

pub(super) async fn initialize_package_root(params: &JsonValue) -> Result<Option<PathBuf>> {
    if let Some(root_uri) = params.get("rootUri").and_then(JsonValue::as_str) {
        return canonicalize_package_root(path_from_file_uri(root_uri)?).await;
    }
    if let Some(root_path) = params.get("rootPath").and_then(JsonValue::as_str) {
        return canonicalize_package_root(PathBuf::from(root_path)).await;
    }
    if let Some(package_folder_uri) = params
        .get("workspaceFolders")
        .and_then(JsonValue::as_array)
        .and_then(|folders| folders.first())
        .and_then(|folder| folder.get("uri"))
        .and_then(JsonValue::as_str)
    {
        return canonicalize_package_root(path_from_file_uri(package_folder_uri)?).await;
    }
    canonicalize_package_root(
        std::env::current_dir()
            .map_err(|err| RototoError::new(format!("failed to read current directory: {err}")))?,
    )
    .await
}

async fn canonicalize_package_root(path: PathBuf) -> Result<Option<PathBuf>> {
    let root = tokio::fs::canonicalize(&path).await.map_err(|err| {
        RototoError::new(format!(
            "failed to canonicalize LSP package root {}: {err}",
            path.display()
        ))
    })?;
    Ok(Some(root))
}

pub(super) fn json_i32(value: Option<&JsonValue>) -> Option<i32> {
    value
        .and_then(JsonValue::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

pub(super) fn source_position_from_json(value: &JsonValue) -> Result<SourcePosition> {
    let line = value
        .get("line")
        .and_then(JsonValue::as_u64)
        .and_then(|line| usize::try_from(line).ok())
        .ok_or_else(|| RototoError::new("position missing line"))?;
    let character = value
        .get("character")
        .and_then(JsonValue::as_u64)
        .and_then(|character| usize::try_from(character).ok())
        .ok_or_else(|| RototoError::new("position missing character"))?;
    Ok(SourcePosition { line, character })
}

pub(super) fn path_from_file_uri(uri: &str) -> Result<PathBuf> {
    let path = uri
        .strip_prefix("file://")
        .ok_or_else(|| RototoError::new(format!("unsupported LSP URI: {uri}")))?;
    percent_decode_path(path).map(PathBuf::from)
}

fn percent_decode_path(path: &str) -> Result<String> {
    let mut decoded = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().and_then(hex_value);
            let low = bytes.get(index + 2).copied().and_then(hex_value);
            match (high, low) {
                (Some(high), Some(low)) => {
                    decoded.push((high << 4) | low);
                    index += 3;
                }
                _ => {
                    return Err(RototoError::new(format!(
                        "invalid percent-encoded LSP URI path: {path}"
                    )));
                }
            }
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|err| RototoError::new(format!("LSP URI path is not UTF-8: {err}")))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn package_relative_path(root: &Path, path: &Path) -> Result<String> {
    let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let relative = canonical_path.strip_prefix(root).map_err(|_| {
        RototoError::new(format!(
            "LSP document is outside package: {}",
            path.display()
        ))
    })?;
    let package_path = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if package_path.is_empty() {
        return Err(RototoError::new("LSP document path is package root"));
    }
    Ok(package_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_paths_percent_decode_special_and_multibyte_bytes() {
        // Editors send file paths as file:// URIs. Decoding must preserve
        // spaces, punctuation, percent signs, and multibyte UTF-8 path bytes.
        assert_eq!(
            path_from_file_uri("file:///tmp/rototo%20%23%C3%A9%25.toml")
                .unwrap()
                .to_string_lossy(),
            "/tmp/rototo #é%.toml"
        );
    }

    #[test]
    fn file_uri_paths_reject_bad_percent_encoding_and_utf8() {
        // Bad URI escapes and non-file schemes should fail before the server
        // tries to map an editor document into a package-relative path.
        assert!(path_from_file_uri("file:///tmp/%").is_err());
        assert!(path_from_file_uri("file:///tmp/%GG").is_err());
        assert!(path_from_file_uri("file:///tmp/%FF").is_err());
        assert!(path_from_file_uri("https://example.test/package").is_err());
    }
}
