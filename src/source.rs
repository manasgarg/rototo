mod archive;
mod auth;
mod git;
mod governance;
mod layer;
mod load;
mod local;
mod path;
mod types;
mod uri;

pub use self::auth::{ScopedBearerTokens, SourceAuth, source_auth_from_package_token_entries};
pub(crate) use self::layer::read_resolve_provenance;
#[cfg(feature = "console")]
pub(crate) use self::load::stage_source_tree;
pub use self::load::{
    load_package_source, load_package_source_snapshot, probe_package_source, stage_package_source,
};
#[cfg(feature = "console")]
pub(crate) use self::types::StagedSourceTree;
pub use self::types::{
    LoadedPackageSource, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe, StagedPackage,
};

const PACKAGE_MANIFEST: &str = "rototo-package.toml";
