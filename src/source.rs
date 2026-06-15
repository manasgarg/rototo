mod archive;
mod git;
mod layer;
mod load;
mod local;
mod path;
mod types;
mod uri;

pub use self::load::{
    load_workspace_source, load_workspace_source_snapshot, probe_workspace_source,
    stage_workspace_source,
};
pub use self::types::{
    LoadedWorkspaceSource, SourceAuth, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe,
    StagedWorkspace,
};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
