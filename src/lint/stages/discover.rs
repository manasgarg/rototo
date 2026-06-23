use std::path::PathBuf;

use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RototoRuleId, SemanticEntity,
};
use crate::error::Result;

use super::super::WORKSPACE_MANIFEST;
use super::super::engine::LintContext;
use super::super::source::{DocumentCollection, DocumentKind};

pub(super) async fn run(ctx: &mut LintContext) -> Result<()> {
    let root = match tokio::fs::canonicalize(&ctx.input.root).await {
        Ok(root) => root,
        Err(err) => {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::WorkspaceNotFound,
                LintStage::Discover,
                SemanticEntity::Workspace,
                DiagnosticLocation::workspace_root(ctx.input.root.display().to_string()),
                err.to_string(),
            ));
            return Ok(());
        }
    };

    let metadata = match tokio::fs::metadata(&root).await {
        Ok(metadata) => metadata,
        Err(err) => {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::WorkspaceNotFound,
                LintStage::Discover,
                SemanticEntity::Workspace,
                DiagnosticLocation::workspace_root(root.display().to_string()),
                err.to_string(),
            ));
            return Ok(());
        }
    };

    if !metadata.is_dir() {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::WorkspaceNotFound,
            LintStage::Discover,
            SemanticEntity::Workspace,
            DiagnosticLocation::workspace_root(root.display().to_string()),
            "workspace path is not a directory",
        ));
        return Ok(());
    }

    ctx.source.root = root;
    let manifest_path = PathBuf::from(WORKSPACE_MANIFEST);
    if tokio::fs::metadata(ctx.source.root.join(&manifest_path))
        .await
        .is_ok_and(|metadata| metadata.is_file())
        || ctx.input.overlays.contains_key(WORKSPACE_MANIFEST)
    {
        ctx.source
            .add_disk_document(manifest_path, DocumentKind::Manifest)
            .await;
    } else {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::WorkspaceManifestMissing,
            LintStage::Discover,
            SemanticEntity::Workspace,
            DiagnosticLocation::workspace_root(ctx.source.root.display().to_string()),
            "workspace manifest is missing",
        ));
        return Ok(());
    }

    ctx.source
        .add_named_toml_documents("qualifiers", DocumentCollection::Qualifiers)
        .await?;
    ctx.source
        .add_named_toml_documents("variables", DocumentCollection::Variables)
        .await?;
    ctx.source.add_catalog_documents().await?;
    ctx.source.add_request_context_documents().await?;
    ctx.source.add_custom_lint_documents().await?;
    ctx.source.add_overlay_documents().await?;

    Ok(())
}
