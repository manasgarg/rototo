use crate::error::{Result, RototoError};
use crate::source::SourceAuth;

use super::types::ArtifactHandle;

#[derive(Clone, Debug)]
pub(super) enum ParsedSource {
    Direct {
        source: String,
    },
    Git {
        remote: String,
        ref_: Option<String>,
        subdir: Option<String>,
        artifact_key: String,
        repo_key: String,
        invalidation_marker: String,
    },
    HttpsArchive {
        url: String,
        subdir: Option<String>,
        artifact_key: String,
        invalidation_marker: String,
    },
}

pub(super) fn parse_source(token: &str, source: &str) -> Result<ParsedSource> {
    let Some((scheme, rest)) = source.split_once("://") else {
        return Ok(ParsedSource::Direct {
            source: source.to_owned(),
        });
    };
    let (base, fragment) = match rest.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (rest, None),
    };
    if base.is_empty() {
        return Err(RototoError::new(format!(
            "workspace source URI is invalid: {source}"
        )));
    }

    if scheme.starts_with("git+") {
        let inner_scheme = scheme.strip_prefix("git+").unwrap_or(scheme);
        if !matches!(inner_scheme, "file" | "https" | "ssh") {
            return Ok(ParsedSource::Direct {
                source: source.to_owned(),
            });
        }
        let remote = format!("{inner_scheme}://{base}");
        let (ref_, subdir) = parse_git_fragment(fragment);
        let ref_label = ref_.as_deref().unwrap_or("HEAD");
        let auth_key = token_key(token);
        let invalidation_marker = format!("{remote}#{ref_label}");
        return Ok(ParsedSource::Git {
            artifact_key: format!("git:{auth_key}:{remote}#{ref_label}"),
            repo_key: format!("git-repo:{auth_key}:{remote}"),
            remote,
            ref_,
            subdir,
            invalidation_marker,
        });
    }

    if scheme == "https" {
        let subdir = parse_archive_fragment(fragment)?;
        let url = format!("{scheme}://{base}");
        let auth_key = token_key(token);
        let invalidation_marker = url.clone();
        return Ok(ParsedSource::HttpsArchive {
            artifact_key: format!("archive:{auth_key}:{url}"),
            url,
            subdir,
            invalidation_marker,
        });
    }

    Ok(ParsedSource::Direct {
        source: source.to_owned(),
    })
}

pub(super) fn view_key(kind: &str, artifact: &ArtifactHandle, subdir: Option<&str>) -> String {
    format!(
        "view:{kind}:artifact:{}:{}:{}",
        artifact.identity,
        artifact.fingerprint,
        subdir.unwrap_or(".")
    )
}

pub(super) fn invalidation_markers(source: &str) -> Vec<String> {
    let token = "";
    match parse_source(token, source) {
        Ok(ParsedSource::Git {
            invalidation_marker,
            ..
        })
        | Ok(ParsedSource::HttpsArchive {
            invalidation_marker,
            ..
        }) => vec![invalidation_marker],
        _ => vec![source.to_owned()],
    }
}

pub(super) fn token_key(token: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, token.as_bytes());
    hex_digest(digest.as_ref())[..12].to_owned()
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub(super) fn auth(token: &str) -> SourceAuth {
    if token.is_empty() {
        SourceAuth::None
    } else {
        SourceAuth::Bearer(token.to_owned())
    }
}

fn parse_git_fragment(fragment: Option<&str>) -> (Option<String>, Option<String>) {
    match fragment {
        None | Some("") => (None, None),
        Some(fragment) => match fragment.split_once(':') {
            Some((ref_, subdir)) if !ref_.is_empty() && !subdir.is_empty() => {
                (Some(ref_.to_owned()), Some(subdir.to_owned()))
            }
            Some(("", subdir)) if !subdir.is_empty() => (None, Some(subdir.to_owned())),
            _ => (Some(fragment.to_owned()), None),
        },
    }
}

fn parse_archive_fragment(fragment: Option<&str>) -> Result<Option<String>> {
    match fragment {
        None | Some("") => Ok(None),
        Some(fragment) if fragment.starts_with(':') && fragment.len() > 1 => {
            Ok(Some(fragment[1..].to_owned()))
        }
        Some(fragment) => Err(RototoError::new(format!(
            "https workspace sources only support #:subdir fragments, got #{fragment}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_source_into_artifact_and_subdir() {
        let parsed = parse_source("secret", "git+https://example.com/repo.git#main:payments")
            .expect("source parses");
        let ParsedSource::Git {
            remote,
            ref_,
            subdir,
            artifact_key,
            repo_key,
            ..
        } = parsed
        else {
            panic!("expected git source");
        };
        assert_eq!(remote, "https://example.com/repo.git");
        assert_eq!(ref_.as_deref(), Some("main"));
        assert_eq!(subdir.as_deref(), Some("payments"));
        assert!(artifact_key.contains("https://example.com/repo.git#main"));
        assert!(repo_key.contains("https://example.com/repo.git"));
        assert!(!artifact_key.contains("payments"));
    }

    #[test]
    fn parses_https_archive_subdir_without_ref() {
        let parsed =
            parse_source("", "https://example.com/workspaces.tar.gz#:payments").expect("source");
        let ParsedSource::HttpsArchive { url, subdir, .. } = parsed else {
            panic!("expected archive source");
        };
        assert_eq!(url, "https://example.com/workspaces.tar.gz");
        assert_eq!(subdir.as_deref(), Some("payments"));
    }

    #[test]
    fn rejects_archive_ref_fragments() {
        let err = parse_source("", "https://example.com/workspaces.tar.gz#main:payments")
            .expect_err("archive refs are not supported");
        assert!(err.to_string().contains("only support #:subdir"));
    }
}
