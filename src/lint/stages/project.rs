use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RototoRuleId};

use super::super::builtins;
use super::super::engine::LintContext;
use super::super::project::build_semantic_index;

pub(super) fn build_projection(ctx: &mut LintContext) {
    ctx.index = build_semantic_index(&ctx.source, &ctx.syntax);
}

pub(super) fn run_builtin(ctx: &mut LintContext) {
    builtins::run_project(ctx);
}

pub(crate) fn push_project_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Project,
        entity,
        primary,
        message,
    ));
}
