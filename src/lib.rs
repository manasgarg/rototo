pub mod catalog;
#[cfg(feature = "console")]
pub mod console;
pub mod diagnostics;
pub mod docs;
pub mod error;
pub mod fixtures;
pub mod inspect;
pub mod lint;
pub mod lsp;
pub mod lua_lint;
pub mod model;
pub mod resolve;
pub mod sdk;
pub mod source;
pub mod testing;
pub mod workspace;

pub use catalog::{catalog, catalog_for_workspace, diagnostic_for_rule};
pub use error::{Result, RototoError};
pub use inspect::inspect_workspace_report;
pub use lint::{diff_workspaces, lint_qualifier, lint_resource, lint_variable, lint_workspace};
pub use resolve::{
    resolve_qualifier, resolve_qualifiers, resolve_variable, resolve_variables,
    trace_qualifier_resolution, trace_qualifier_resolutions, trace_variable_resolution,
    trace_variable_resolutions,
};
pub use sdk::{
    LintMode, LoadOptions, RefreshOptions, RefreshOutcome, RefreshStatus, RefreshingWorkspace,
    ResolveContext, ResolveOptions, Workspace,
};
pub use source::{
    SourceAuth, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe, StagedWorkspace,
    probe_workspace_source, stage_workspace_source,
};
pub use workspace::{
    find_workspace_root, inspect_workspace, list_qualifiers, list_resources, list_variables,
    read_qualifier, read_qualifiers, read_resource, read_resources, read_variable, read_variables,
};
