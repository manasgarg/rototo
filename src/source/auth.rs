//! Bearer tokens for https:// package archive sources.
//!
//! A load graph can fetch archives from more than one place: a local overlay
//! extending a private team archive, or two teams publishing bases on
//! different hosts. One process-wide token would either broadcast a secret to
//! every origin the graph touches or lock the graph to a single private host.
//! Scoped tokens fix both: each token binds to a normalized URL prefix, a
//! request gets the token of the longest matching prefix, and a request no
//! prefix matches goes out anonymous.
//!
//! The single-token spelling stays as sugar for the common case, but it binds
//! to the load graph's one archive origin; a second distinct origin fails the
//! load instead of silently receiving someone else's secret.

use std::sync::{Arc, Mutex};

use crate::error::{Result, RototoError};

/// Authentication for https:// package archive downloads. Git sources are not
/// covered: git authenticates through its own per-host machinery.
#[derive(Clone, Debug)]
pub enum SourceAuth {
    None,
    /// One token for a load graph with exactly one archive origin. The token
    /// binds to the first origin fetched; a second distinct origin fails the
    /// load and points at scoped entries.
    Bearer(String),
    /// Tokens scoped to URL prefixes; longest matching prefix wins, no match
    /// means the request is anonymous.
    Scoped(ScopedBearerTokens),
}

/// A longest-prefix map from normalized https URL prefixes to bearer tokens.
#[derive(Clone, Debug, Default)]
pub struct ScopedBearerTokens {
    /// `(normalized prefix, token)`, unordered; lookups scan for the longest
    /// matching prefix.
    entries: Vec<(String, String)>,
}

impl ScopedBearerTokens {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a token scoped to `prefix`. The prefix must be an `https://` URL
    /// prefix without query, fragment, `=`, or whitespace; it is normalized
    /// (scheme and host lowercased, a default `:443` port elided, trailing
    /// slash dropped). A duplicate of an already-added prefix is an error.
    pub fn with_prefix(
        mut self,
        prefix: impl AsRef<str>,
        token: impl Into<String>,
    ) -> Result<Self> {
        let normalized = normalize_token_prefix(prefix.as_ref())?;
        if self
            .entries
            .iter()
            .any(|(existing, _)| existing == &normalized)
        {
            return Err(RototoError::new(format!(
                "duplicate package token prefix: {normalized}"
            )));
        }
        let token = token.into();
        if token.is_empty() {
            return Err(RototoError::new(format!(
                "package token for prefix {normalized} is empty"
            )));
        }
        self.entries.push((normalized, token));
        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The token of the longest prefix matching `url`, if any. Prefixes match
    /// on whole path segments: `https://host/team` covers `/team` and
    /// `/team/...`, never `/teammate`.
    pub fn token_for(&self, url: &str) -> Option<&str> {
        let normalized = normalize_archive_url(url);
        self.entries
            .iter()
            .filter(|(prefix, _)| {
                normalized == *prefix
                    || normalized
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, token)| token.as_str())
    }

    /// The prefix that would win for `url`, for error messages.
    pub(crate) fn matching_prefix(&self, url: &str) -> Option<&str> {
        let normalized = normalize_archive_url(url);
        self.entries
            .iter()
            .filter(|(prefix, _)| {
                normalized == *prefix
                    || normalized
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(prefix, _)| prefix.as_str())
    }
}

/// Builds a [`SourceAuth`] from `--package-token` entries. The same grammar
/// serves the CLI flag (one entry per occurrence) and the
/// `ROTOTO_PACKAGE_TOKEN` environment variable (whitespace-separated entries):
///
/// - an entry starting with `https://` is `PREFIX=TOKEN`, split at the first
///   `=` after the prefix (prefixes never contain `=`);
/// - any other entry is a bare token, even one ending in `=` padding.
///
/// A bare token is single-origin sugar and must be the only entry; mixing it
/// with scoped entries, or passing more than one bare token, is an error.
pub fn source_auth_from_package_token_entries(entries: &[String]) -> Result<SourceAuth> {
    let mut bare: Vec<&str> = Vec::new();
    let mut scoped = ScopedBearerTokens::new();
    for entry in entries {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        // Never sniff on '=': base64-padded bare tokens end in '='. Only the
        // https:// spelling marks a scoped entry.
        if entry.len() >= 8 && entry[..8].eq_ignore_ascii_case("https://") {
            let Some((prefix, token)) = entry.split_once('=') else {
                return Err(RototoError::new(format!(
                    "package token entry `{entry}` looks like a URL prefix but has no `=TOKEN`; write https://host/prefix=TOKEN"
                )));
            };
            scoped = scoped.with_prefix(prefix, token)?;
        } else {
            bare.push(entry);
        }
    }
    match (bare.len(), scoped.is_empty()) {
        (0, true) => Ok(SourceAuth::None),
        (1, true) => Ok(SourceAuth::Bearer(bare[0].to_owned())),
        (0, false) => Ok(SourceAuth::Scoped(scoped)),
        (_, false) => Err(RototoError::new(
            "a bare package token cannot be mixed with https://prefix=TOKEN entries; scope every token",
        )),
        (_, true) => Err(RototoError::new(
            "more than one bare package token; scope each one as https://host/prefix=TOKEN",
        )),
    }
}

/// The origin a bare token binds to once the first archive request goes out.
/// Shared across clones of one `SourceOptions` so every fetch in a load graph
/// sees the same binding.
#[derive(Clone, Debug, Default)]
pub(crate) struct BearerOriginBinding {
    origin: Arc<Mutex<Option<String>>>,
}

impl BearerOriginBinding {
    /// Binds the bare token to `url`'s origin, or errors when the load graph
    /// already bound it to a different one.
    pub(crate) fn bind(&self, url: &str) -> Result<()> {
        let origin = archive_origin(url);
        let mut bound = self
            .origin
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match bound.as_deref() {
            None => {
                *bound = Some(origin);
                Ok(())
            }
            Some(existing) if existing == origin => Ok(()),
            Some(existing) => Err(RototoError::new(format!(
                "a bare package token is bound to a single archive origin, but this load fetches both {existing} and {origin}; scope the tokens as {existing}=TOKEN {origin}=TOKEN"
            ))),
        }
    }
}

/// Normalizes and validates a token prefix. Https only, no query or fragment,
/// no `=` (the entry separator), no whitespace.
fn normalize_token_prefix(prefix: &str) -> Result<String> {
    if prefix.chars().any(char::is_whitespace) {
        return Err(RototoError::new(format!(
            "package token prefix contains whitespace: {prefix}"
        )));
    }
    if prefix.contains('=') {
        return Err(RototoError::new(format!(
            "package token prefix contains `=`: {prefix}; prefixes are path prefixes only"
        )));
    }
    if prefix.contains('?') || prefix.contains('#') {
        return Err(RototoError::new(format!(
            "package token prefix must not carry a query or fragment: {prefix}"
        )));
    }
    if !(prefix.len() > 8 && prefix[..8].eq_ignore_ascii_case("https://")) {
        return Err(RototoError::new(format!(
            "package token prefix must start with https://: {prefix}"
        )));
    }
    Ok(normalize_archive_url(prefix)
        .trim_end_matches('/')
        .to_owned())
}

/// Lowercases the scheme and host of an https URL and elides a default `:443`
/// port; the path stays byte-exact (paths are case-sensitive).
fn normalize_archive_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let mut authority = authority.to_ascii_lowercase();
    if let Some(stripped) = authority.strip_suffix(":443") {
        authority = stripped.to_owned();
    }
    format!("{}://{authority}{path}", scheme.to_ascii_lowercase())
}

/// The `scheme://host[:port]` origin of an archive URL, normalized.
pub(crate) fn archive_origin(url: &str) -> String {
    let normalized = normalize_archive_url(url);
    match normalized.find("://") {
        Some(scheme_end) => {
            let after = scheme_end + 3;
            match normalized[after..].find('/') {
                Some(index) => normalized[..after + index].to_owned(),
                None => normalized,
            }
        }
        None => normalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn no_entries_is_anonymous() {
        assert!(matches!(
            source_auth_from_package_token_entries(&[]).unwrap(),
            SourceAuth::None
        ));
    }

    #[test]
    fn one_bare_entry_is_the_single_origin_sugar() {
        let auth = source_auth_from_package_token_entries(&entries(&["secret"])).unwrap();
        assert!(matches!(auth, SourceAuth::Bearer(token) if token == "secret"));
    }

    #[test]
    fn base64_padding_never_makes_a_bare_token_scoped() {
        // A padded token contains '=' but does not start with https://, so it
        // stays a bare token.
        let auth = source_auth_from_package_token_entries(&entries(&["dG9rZW4="])).unwrap();
        assert!(matches!(auth, SourceAuth::Bearer(token) if token == "dG9rZW4="));
    }

    #[test]
    fn https_entries_split_at_the_first_equals() {
        let auth = source_auth_from_package_token_entries(&entries(&[
            "https://config.acme.com/team-a=tok=en=",
        ]))
        .unwrap();
        let SourceAuth::Scoped(scoped) = auth else {
            panic!("expected scoped auth");
        };
        // The token keeps every '=' after the first split.
        assert_eq!(
            scoped.token_for("https://config.acme.com/team-a/current.tar.gz"),
            Some("tok=en=")
        );
    }

    #[test]
    fn scoped_prefix_without_token_is_an_error() {
        let err = source_auth_from_package_token_entries(&entries(&["https://config.acme.com"]))
            .unwrap_err();
        assert!(err.to_string().contains("has no `=TOKEN`"), "{err}");
    }

    #[test]
    fn mixing_bare_and_scoped_entries_is_an_error() {
        let err = source_auth_from_package_token_entries(&entries(&[
            "secret",
            "https://config.acme.com=other",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("cannot be mixed"), "{err}");
    }

    #[test]
    fn two_bare_entries_are_an_error() {
        let err =
            source_auth_from_package_token_entries(&entries(&["first", "second"])).unwrap_err();
        assert!(err.to_string().contains("more than one bare"), "{err}");
    }

    #[test]
    fn longest_matching_prefix_wins() {
        let scoped = ScopedBearerTokens::new()
            .with_prefix("https://config.acme.com", "host-wide")
            .unwrap()
            .with_prefix("https://config.acme.com/team-a", "team-a")
            .unwrap();
        assert_eq!(
            scoped.token_for("https://config.acme.com/team-a/current.tar.gz"),
            Some("team-a")
        );
        assert_eq!(
            scoped.token_for("https://config.acme.com/other/current.tar.gz"),
            Some("host-wide")
        );
    }

    #[test]
    fn prefixes_match_whole_path_segments() {
        let scoped = ScopedBearerTokens::new()
            .with_prefix("https://config.acme.com/team", "team")
            .unwrap();
        assert_eq!(
            scoped.token_for("https://config.acme.com/team/pkg.tar.gz"),
            Some("team")
        );
        // `/teammate` is a different path, not a longer spelling of `/team`.
        assert_eq!(
            scoped.token_for("https://config.acme.com/teammate/pkg.tar.gz"),
            None
        );
    }

    #[test]
    fn no_matching_prefix_means_anonymous() {
        let scoped = ScopedBearerTokens::new()
            .with_prefix("https://config.acme.com", "token")
            .unwrap();
        assert_eq!(
            scoped.token_for("https://other.example.com/pkg.tar.gz"),
            None
        );
    }

    #[test]
    fn matching_normalizes_host_case_and_default_port() {
        let scoped = ScopedBearerTokens::new()
            .with_prefix("HTTPS://Config.Acme.com:443/team/", "token")
            .unwrap();
        assert_eq!(
            scoped.token_for("https://config.acme.com/team/pkg.tar.gz"),
            Some("token")
        );
        // Paths stay case-sensitive.
        assert_eq!(
            scoped.token_for("https://config.acme.com/Team/pkg.tar.gz"),
            None
        );
    }

    #[test]
    fn non_default_ports_are_distinct_origins() {
        let scoped = ScopedBearerTokens::new()
            .with_prefix("https://config.acme.com:8443", "token")
            .unwrap();
        assert_eq!(
            scoped.token_for("https://config.acme.com:8443/pkg.tar.gz"),
            Some("token")
        );
        assert_eq!(scoped.token_for("https://config.acme.com/pkg.tar.gz"), None);
    }

    #[test]
    fn prefix_validation_rejects_bad_shapes() {
        for prefix in [
            "http://config.acme.com",
            "https://config.acme.com?x",
            "https://config.acme.com#frag",
            "https://",
        ] {
            assert!(
                ScopedBearerTokens::new()
                    .with_prefix(prefix, "token")
                    .is_err(),
                "{prefix} should be rejected"
            );
        }
    }

    #[test]
    fn duplicate_prefixes_are_rejected() {
        let err = ScopedBearerTokens::new()
            .with_prefix("https://config.acme.com/team/", "one")
            .unwrap()
            .with_prefix("https://Config.acme.com/team", "two")
            .unwrap_err();
        assert!(err.to_string().contains("duplicate"), "{err}");
    }

    #[test]
    fn bare_token_binds_to_the_first_origin() {
        let binding = BearerOriginBinding::default();
        binding
            .bind("https://config.acme.com/team-a/pkg.tar.gz")
            .unwrap();
        // Same origin, different path: fine.
        binding
            .bind("https://config.acme.com:443/team-b/pkg.tar.gz")
            .unwrap();
        // A second distinct origin fails the load.
        let err = binding
            .bind("https://other.example.com/pkg.tar.gz")
            .unwrap_err();
        assert!(err.to_string().contains("single archive origin"), "{err}");
        assert!(err.to_string().contains("https://config.acme.com"), "{err}");
        assert!(
            err.to_string().contains("https://other.example.com"),
            "{err}"
        );
    }

    #[test]
    fn clones_share_the_origin_binding() {
        let binding = BearerOriginBinding::default();
        let clone = binding.clone();
        binding.bind("https://config.acme.com/a.tar.gz").unwrap();
        assert!(clone.bind("https://other.example.com/b.tar.gz").is_err());
    }
}
