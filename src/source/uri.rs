use crate::error::{Result, RototoError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceUri {
    pub(super) scheme: String,
    pub(super) base: String,
    pub(super) ref_: Option<String>,
    pub(super) subdir: Option<String>,
}

impl SourceUri {
    pub(super) fn parse(source: &str) -> Result<Option<Self>> {
        let Some((scheme, rest)) = source.split_once("://") else {
            return Ok(None);
        };
        if scheme.is_empty() || rest.is_empty() {
            return Err(RototoError::new(format!(
                "package source URI is invalid: {source}"
            )));
        }
        let (base, fragment) = match rest.split_once('#') {
            Some((base, fragment)) => (base, Some(fragment)),
            None => (rest, None),
        };
        if base.is_empty() {
            return Err(RototoError::new(format!(
                "package source URI is invalid: {source}"
            )));
        }
        let (ref_, subdir) = match fragment {
            Some(fragment) => match fragment.split_once(':') {
                Some((ref_, subdir)) => (
                    (!ref_.is_empty()).then(|| ref_.to_owned()),
                    (!subdir.is_empty()).then(|| subdir.to_owned()),
                ),
                None => ((!fragment.is_empty()).then(|| fragment.to_owned()), None),
            },
            None => (None, None),
        };
        Ok(Some(Self {
            scheme: scheme.to_ascii_lowercase(),
            base: base.to_owned(),
            ref_,
            subdir,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_uri_rejects_malformed_uris() {
        assert!(SourceUri::parse("examples/basic").unwrap().is_none());
        assert!(SourceUri::parse("://example.com/package.tar.gz").is_err());
        assert!(SourceUri::parse("https://").is_err());
        assert!(SourceUri::parse("https://#main").is_err());
    }

    #[test]
    fn source_uri_accepts_supported_source_forms() {
        for source in [
            "file:///tmp/package",
            "git+file:///tmp/package.git#main:rototo",
            "git+https://github.com/example/config.git#main:rototo",
            "git+ssh://git@github.com/example/config.git#main:rototo",
            "https://example.com/package.tar.gz#:rototo",
        ] {
            assert!(
                SourceUri::parse(source).unwrap().is_some(),
                "source should parse: {source}"
            );
        }
    }
}
