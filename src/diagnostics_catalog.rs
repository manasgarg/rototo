use std::path::Path;

use crate::diagnostics::{DiagnosticCatalogEntry, RototoRuleId};
use crate::error::{Result, RototoError};
use crate::lint::{LintInput, lint_package_snapshot};
use crate::model::{DiagnosticCatalog, DiagnosticCatalogScope};
use crate::package::inspect_package;

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

pub async fn diagnostics_catalog_for_package(package_root: &Path) -> Result<DiagnosticCatalog> {
    let package = inspect_package(package_root).await?;
    let snapshot = lint_package_snapshot(LintInput::new(package.root.clone())).await?;
    let mut diagnostics = snapshot.diagnostic_catalog_entries();
    diagnostics.sort_by(|left, right| left.rule.cmp(&right.rule));

    Ok(DiagnosticCatalog {
        scope: DiagnosticCatalogScope::Package,
        subject: package.root.display().to_string(),
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
