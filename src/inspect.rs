use std::path::Path;

use crate::error::Result;
use crate::lint::{
    LintInput, compile_runtime_package_from_snapshot, inspect_snapshot, lint_package_snapshot,
};
use crate::model::{PackageInspectReport, PackageInspectRequest};

pub async fn inspect_package_report(
    package_root: &Path,
    request: PackageInspectRequest,
) -> Result<PackageInspectReport> {
    let snapshot = lint_package_snapshot(LintInput::new(package_root.to_path_buf())).await?;
    let runtime = compile_runtime_package_from_snapshot(&snapshot);
    let (runtime, runtime_error) = match runtime {
        Ok(runtime) => (Some(runtime), None),
        Err(err) => (None, Some(err.to_string())),
    };
    inspect_snapshot(&snapshot, runtime.as_ref(), runtime_error, &request).await
}
