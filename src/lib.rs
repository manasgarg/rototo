pub(crate) mod address;
#[cfg(feature = "console")]
pub mod console;
pub mod diagnostics;
pub mod diagnostics_catalog;
pub mod docs;
pub mod error;
mod expression;
pub mod fixtures;
pub mod inspect;
pub mod lint;
pub mod lsp;
pub mod lua_lint;
pub mod model;
pub mod pack;
pub mod package;
mod predicate;
pub mod resolve;
pub mod sdk;
pub mod source;

pub use diagnostics_catalog::{
    diagnostic_for_rule, diagnostics_catalog, diagnostics_catalog_for_package,
};
pub use error::{Result, RototoError};
pub use inspect::inspect_package_report;
pub use lint::{diff_packages, lint_catalog, lint_package, lint_variable};
pub use pack::{PackagedArchive, pack_package, project_package};
pub use package::{
    find_package_root, inspect_package, list_catalogs, list_variables, read_catalog, read_catalogs,
    read_variable, read_variables,
};
pub use resolve::{
    resolve_variable, resolve_variables, trace_variable_resolution, trace_variable_resolutions,
};
pub use sdk::{
    EvaluationContext, LintMode, LoadOptions, Package, PackageIdentity, PackageLayerIdentity,
    RedactedPackageSource, RefreshEvent, RefreshEventSummary, RefreshEventType, RefreshOptions,
    RefreshOutcome, RefreshSnapshot, RefreshStatus, RefreshingPackage, ResolveOptions, SdkIdentity,
    TraceDetail, TraceEvent, TraceProvenance, TraceStreamItem, TraceSubscription, TraceTarget,
    ValueRef, source_fingerprint_to_json,
};
pub use source::{
    ScopedBearerTokens, SourceAuth, SourceFingerprint, SourceLayer, SourceOptions, SourceProbe,
    StagedPackage, probe_package_source, source_auth_from_package_token_entries,
    stage_package_source,
};
