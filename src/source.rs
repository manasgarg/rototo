mod archive;
mod auth;
mod git;
mod governance;
mod layer;
mod load;
mod local;
mod path;
mod pin;
mod types;
mod uri;

pub use self::auth::{ScopedBearerTokens, SourceAuth, source_auth_from_package_token_entries};
pub(crate) use self::layer::read_resolve_provenance;
pub use self::load::{
    load_package_source, load_package_source_snapshot, probe_package_source, stage_package_source,
};
pub use self::pin::PinStore;
pub use self::types::{
    LoadedPackageSource, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe, StagedPackage,
};

const PACKAGE_MANIFEST: &str = "rototo-package.toml";
