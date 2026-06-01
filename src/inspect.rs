use std::path::Path;

use crate::error::Result;
use crate::lint::{
    LintInput, compile_runtime_workspace_from_snapshot, inspect_snapshot, lint_workspace_snapshot,
};
use crate::model::{WorkspaceInspectReport, WorkspaceInspectRequest};

pub async fn inspect_workspace_report(
    workspace_root: &Path,
    request: WorkspaceInspectRequest,
) -> Result<WorkspaceInspectReport> {
    let snapshot = lint_workspace_snapshot(LintInput::new(workspace_root.to_path_buf())).await?;
    let runtime = compile_runtime_workspace_from_snapshot(&snapshot);
    let (runtime, runtime_error) = match runtime {
        Ok(runtime) => (Some(runtime), None),
        Err(err) => (None, Some(err.to_string())),
    };
    inspect_snapshot(&snapshot, runtime.as_ref(), runtime_error, &request).await
}
