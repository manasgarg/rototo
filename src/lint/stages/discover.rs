use std::path::PathBuf;

use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RototoRuleId, SemanticEntity,
};
use crate::error::Result;

use super::super::PACKAGE_MANIFEST;
use super::super::engine::LintContext;
use super::super::source::{DocumentCollection, DocumentKind};

pub(super) async fn run(ctx: &mut LintContext) -> Result<()> {
    let root = match tokio::fs::canonicalize(&ctx.input.root).await {
        Ok(root) => root,
        Err(err) => {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::PackageNotFound,
                LintStage::Discover,
                SemanticEntity::Package,
                DiagnosticLocation::package_root(ctx.input.root.display().to_string()),
                err.to_string(),
            ));
            return Ok(());
        }
    };

    let metadata = match tokio::fs::metadata(&root).await {
        Ok(metadata) => metadata,
        Err(err) => {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::PackageNotFound,
                LintStage::Discover,
                SemanticEntity::Package,
                DiagnosticLocation::package_root(root.display().to_string()),
                err.to_string(),
            ));
            return Ok(());
        }
    };

    if !metadata.is_dir() {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::PackageNotFound,
            LintStage::Discover,
            SemanticEntity::Package,
            DiagnosticLocation::package_root(root.display().to_string()),
            "package path is not a directory",
        ));
        return Ok(());
    }

    ctx.source.root = root;
    let manifest_path = PathBuf::from(PACKAGE_MANIFEST);
    if tokio::fs::metadata(ctx.source.root.join(&manifest_path))
        .await
        .is_ok_and(|metadata| metadata.is_file())
        || ctx.input.overlays.contains_key(PACKAGE_MANIFEST)
    {
        ctx.source
            .add_disk_document(manifest_path, DocumentKind::Manifest)
            .await;
    } else {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::PackageManifestMissing,
            LintStage::Discover,
            SemanticEntity::Package,
            DiagnosticLocation::package_root(ctx.source.root.display().to_string()),
            "package manifest is missing",
        ));
        return Ok(());
    }

    let governance_path = PathBuf::from("governance.toml");
    if tokio::fs::metadata(ctx.source.root.join(&governance_path))
        .await
        .is_ok_and(|metadata| metadata.is_file())
        || ctx.input.overlays.contains_key("governance.toml")
    {
        ctx.source
            .add_disk_document(governance_path, DocumentKind::Governance)
            .await;
    }

    ctx.source
        .add_named_toml_documents("variables", DocumentCollection::Variables)
        .await?;
    ctx.source.add_list_documents().await?;
    ctx.source.add_layer_documents().await?;
    ctx.source.add_catalog_documents().await?;
    ctx.source.add_evaluation_context_documents().await?;
    ctx.source.add_custom_lint_documents().await?;
    ctx.source.add_overlay_documents().await?;
    report_unrecognized_files(ctx).await;

    Ok(())
}

/// Every file under a rototo-owned directory should map to an entity the
/// package declares; a file nothing claims would otherwise vanish silently
/// (a mistyped suffix, a list members file for an undeclared list, a
/// catalog entry under a catalog with no schema). Warn on each one.
async fn report_unrecognized_files(ctx: &mut LintContext) {
    for owned in ["model", "data", "variables", "lists", "layers", "lint"] {
        let root = ctx.source.root.join(owned);
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&directory).await else {
                continue;
            };
            while let Some(entry) = entries.next_entry().await.ok().flatten() {
                let path = entry.path();
                let Ok(metadata) = tokio::fs::metadata(&path).await else {
                    continue;
                };
                if metadata.is_dir() {
                    pending.push(path);
                    continue;
                }
                let Ok(relative) = path.strip_prefix(&ctx.source.root) else {
                    continue;
                };
                let Some(relative) = relative.to_str() else {
                    continue;
                };
                let relative = relative.replace(std::path::MAIN_SEPARATOR, "/");
                if ctx.source.document_by_path(&relative).is_some() {
                    continue;
                }
                ctx.diagnostics.push(LintDiagnostic::rototo(
                    RototoRuleId::UnrecognizedFile,
                    LintStage::Discover,
                    SemanticEntity::Package,
                    DiagnosticLocation::package_root(relative.clone()),
                    format!("no rototo entity claims this file: {relative}"),
                ));
            }
        }
    }
}
