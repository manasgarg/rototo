use crate::diagnostics::{LintDiagnostic, LintStage, RototoRuleId};
use crate::workspace::validate_workspace_manifest;

use super::super::engine::LintContext;

pub(super) fn lint_manifest_shape(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };
    let Some(parsed) = ctx.syntax.toml.get(&manifest.doc) else {
        return;
    };

    if let Err(err) = validate_workspace_manifest(&parsed.to_plain_toml()) {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::WorkspaceManifestSchemaFailed,
            LintStage::Project,
            manifest.target(),
            manifest.location.clone(),
            err.to_string(),
        ));
    }
}
