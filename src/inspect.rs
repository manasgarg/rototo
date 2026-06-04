use std::path::Path;

use crate::error::Result;
use crate::layering::WorkspaceLayers;
use crate::lint::{
    LintInput, compile_runtime_workspace_from_snapshot, inspect_snapshot, lint_workspace_snapshot,
};
use crate::model::{WorkspaceInspectReport, WorkspaceInspectRequest};

pub async fn inspect_workspace_report(
    workspace_root: &Path,
    request: WorkspaceInspectRequest,
) -> Result<WorkspaceInspectReport> {
    inspect_workspace_report_with_layers(workspace_root, request, &WorkspaceLayers::default()).await
}

/// Build an inspection report and attach layer provenance when the workspace was
/// composed from more than one layer.
pub async fn inspect_workspace_report_with_layers(
    workspace_root: &Path,
    request: WorkspaceInspectRequest,
    layers: &WorkspaceLayers,
) -> Result<WorkspaceInspectReport> {
    let snapshot = lint_workspace_snapshot(LintInput::new(workspace_root.to_path_buf())).await?;
    let runtime = compile_runtime_workspace_from_snapshot(&snapshot);
    let (runtime, runtime_error) = match runtime {
        Ok(runtime) => (Some(runtime), None),
        Err(err) => (None, Some(err.to_string())),
    };
    let mut report = inspect_snapshot(&snapshot, runtime.as_ref(), runtime_error, &request).await?;
    if layers.is_layered() {
        report.layers = layers.layers().to_vec();
    }
    Ok(report)
}
