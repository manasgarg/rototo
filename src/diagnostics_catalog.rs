use std::path::Path;

use crate::diagnostics::{DiagnosticCatalogEntry, RototoRuleId};
use crate::error::{Result, RototoError};
use crate::lint::{LintInput, lint_workspace_snapshot};
use crate::model::{DiagnosticCatalog, DiagnosticCatalogScope};
use crate::workspace::inspect_workspace;

pub fn diagnostics_catalog() -> DiagnosticCatalog {
    let mut diagnostics: Vec<_> = RototoRuleId::iter()
        .map(DiagnosticCatalogEntry::from_rototo)
        .collect();
    diagnostics.sort_by(|left, right| left.rule.cmp(&right.rule));

    DiagnosticCatalog {
        scope: DiagnosticCatalogScope::Global,
        subject: "global".to_owned(),
        diagnostics,
    }
}

pub async fn diagnostics_catalog_for_workspace(workspace_root: &Path) -> Result<DiagnosticCatalog> {
    let workspace = inspect_workspace(workspace_root).await?;
    let snapshot = lint_workspace_snapshot(LintInput::new(workspace.root.clone())).await?;
    let mut diagnostics = snapshot.diagnostic_catalog_entries();
    diagnostics.sort_by(|left, right| left.rule.cmp(&right.rule));

    Ok(DiagnosticCatalog {
        scope: DiagnosticCatalogScope::Workspace,
        subject: workspace.root.display().to_string(),
        diagnostics,
    })
}

pub fn diagnostic_for_rule<'a>(
    catalog: &'a DiagnosticCatalog,
    rule: &str,
) -> Result<&'a DiagnosticCatalogEntry> {
    catalog
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule == rule)
        .ok_or_else(|| RototoError::new(format!("diagnostic not found: {rule}")))
}
