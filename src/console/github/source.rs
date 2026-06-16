use crate::error::{Result, RototoError};

const REPO_SPEC_ERROR: &str = "repo must be owner/name or a GitHub repository URL";

pub fn parse_repo_spec(value: &str) -> Result<(String, String)> {
    let trimmed = value.trim();
    let trimmed = strip_prefix_ignore_ascii_case(trimmed, "git+").unwrap_or(trimmed);
    if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "https://api.github.com/repos/") {
        let rest = rest
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        let mut parts = rest.split('/');
        let Some(owner) = parts.next() else {
            return Err(RototoError::new(REPO_SPEC_ERROR));
        };
        let Some(name) = parts.next() else {
            return Err(RototoError::new(REPO_SPEC_ERROR));
        };
        let archive_kind = parts.next();
        if !matches!(archive_kind, Some("tarball" | "zipball")) {
            return Err(RototoError::new(REPO_SPEC_ERROR));
        }
        let Some(_) = parts.next() else {
            return Err(RototoError::new(REPO_SPEC_ERROR));
        };
        if parts.next().is_some() {
            return Err(RototoError::new(REPO_SPEC_ERROR));
        }
        validate_repo_parts(owner, name)?;
        return Ok((owner.to_owned(), name.to_owned()));
    }
    let mut candidate = strip_prefix_ignore_ascii_case(trimmed, "git@github.com:")
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "ssh://git@github.com/"))
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "https://github.com/"))
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "http://github.com/"))
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "github.com/"))
        .unwrap_or(trimmed);
    candidate = candidate
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let Some((owner, mut name)) = candidate.split_once('/') else {
        return Err(RototoError::new(REPO_SPEC_ERROR));
    };
    name = name.strip_suffix(".git").unwrap_or(name);
    validate_repo_parts(owner, name)?;
    Ok((owner.to_owned(), name.to_owned()))
}

fn validate_repo_parts(owner: &str, name: &str) -> Result<()> {
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    if !valid(owner) || !valid(name) || name.contains('/') {
        return Err(RototoError::new(REPO_SPEC_ERROR));
    }
    Ok(())
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        value.get(prefix.len()..)
    } else {
        None
    }
}

pub fn workspace_git_source(owner: &str, name: &str, git_ref: &str, path: &str) -> String {
    let remote = format!("git+https://github.com/{}/{}.git", enc(owner), enc(name));
    if path == "." {
        format!("{remote}#{git_ref}")
    } else {
        format!("{remote}#{git_ref}:{path}")
    }
}

pub fn stable_workspace_key(source_tree_label: &str, path: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("{source_tree_label}:{path}").as_bytes(),
    );
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()[..12]
        .to_owned()
}

pub fn workspace_repo_path(workspace_path: &str, relative_path: &str) -> String {
    if workspace_path == "." {
        relative_path.to_owned()
    } else {
        format!("{workspace_path}/{relative_path}")
    }
}

pub(super) fn encode_repo_path(path: &str) -> String {
    path.split('/').map(enc).collect::<Vec<_>>().join("/")
}

pub(super) fn manifest_workspace_path(manifest_path: &str) -> String {
    let path = manifest_path
        .strip_suffix("/rototo-workspace.toml")
        .unwrap_or_else(|| {
            if manifest_path == "rototo-workspace.toml" {
                ""
            } else {
                manifest_path
            }
        });
    if path.is_empty() {
        ".".to_owned()
    } else {
        path.to_owned()
    }
}

/// Percent-encode a single URL path segment or query value.
pub(super) fn enc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_spec_parses_owner_name() {
        assert_eq!(
            parse_repo_spec(" octo/configs ").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("https://github.com/octo/configs.git").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("git@github.com:octo/configs.git").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("ssh://git@github.com/octo/configs.git").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("git+https://github.com/octo/configs.git#main:apps").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("git+ssh://git@github.com/octo/configs.git#feature/x").unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert_eq!(
            parse_repo_spec("https://api.github.com/repos/octo/configs/tarball/main#:apps")
                .unwrap(),
            ("octo".to_owned(), "configs".to_owned())
        );
        assert!(parse_repo_spec("octo").is_err());
        assert!(parse_repo_spec("octo/configs/extra").is_err());
        assert!(parse_repo_spec("https://example.com/octo/configs").is_err());
        assert!(parse_repo_spec("git+https://example.com/octo/configs.git").is_err());
        assert!(parse_repo_spec("https://github.com/octo/configs/tree/main").is_err());
        assert!(parse_repo_spec("octo/with space").is_err());
    }

    #[test]
    fn git_source_appends_ref_and_subdir_fragment() {
        assert_eq!(
            workspace_git_source("o", "r", "main", "."),
            "git+https://github.com/o/r.git#main"
        );
        assert_eq!(
            workspace_git_source("o", "r", "main", "payments/flags"),
            "git+https://github.com/o/r.git#main:payments/flags"
        );
    }

    #[test]
    fn manifest_paths_map_to_workspace_paths() {
        assert_eq!(manifest_workspace_path("rototo-workspace.toml"), ".");
        assert_eq!(
            manifest_workspace_path("payments/flags/rototo-workspace.toml"),
            "payments/flags"
        );
    }

    #[test]
    fn stable_workspace_key_is_short_hex() {
        let key = stable_workspace_key("octo/configs", ".");
        assert_eq!(key.len(), 12);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(key, stable_workspace_key("octo/configs", "."));
    }
}
