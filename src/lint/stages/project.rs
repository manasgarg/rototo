use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RototoRuleId, SemanticTarget,
};

use super::super::builtins;
use super::super::engine::LintContext;
use super::super::project::build_semantic_index;

pub(super) fn build_projection(ctx: &mut LintContext) {
    let gates = std::mem::take(&mut ctx.index.gates);
    ctx.index = build_semantic_index(&ctx.source, &ctx.syntax);
    ctx.index.gates = gates;
}

pub(super) fn run_builtin(ctx: &mut LintContext) {
    builtins::run_project(ctx);
}

pub(crate) fn push_project_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    target: impl Into<SemanticTarget>,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Project,
        target,
        primary,
        message,
    ));
}
