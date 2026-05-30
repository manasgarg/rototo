use std::path::Path;

use crate::diagnostics::{DiagnosticCatalogEntry, kernel_catalog_entries};
use crate::error::{Result, RototoError};
use crate::model::{DiagnosticCatalog, DiagnosticCatalogScope};
use crate::workspace::inspect_workspace;

pub fn catalog() -> DiagnosticCatalog {
    let mut diagnostics = kernel_catalog_entries();
    diagnostics.sort_by(|left, right| left.code.cmp(&right.code));

    DiagnosticCatalog {
        scope: DiagnosticCatalogScope::Global,
        subject: "global".to_owned(),
        diagnostics,
    }
}

pub async fn catalog_for_workspace(workspace_root: &Path) -> Result<DiagnosticCatalog> {
    let workspace = inspect_workspace(workspace_root).await?;
    let mut catalog = catalog();
    catalog.scope = DiagnosticCatalogScope::Workspace;
    catalog.subject = workspace.root.display().to_string();

    Ok(catalog)
}

pub fn diagnostic_for_code<'a>(
    catalog: &'a DiagnosticCatalog,
    code: &str,
) -> Result<&'a DiagnosticCatalogEntry> {
    catalog
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == code)
        .ok_or_else(|| RototoError::new(format!("diagnostic not found: {code}")))
}
