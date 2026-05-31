use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RototoRuleId};

use super::super::builtins;
use super::super::engine::LintContext;
use super::super::references::ReferenceIndex;

pub(super) fn run_builtin(ctx: &mut LintContext) {
    ctx.references = ReferenceIndex::build(&ctx.index, &ctx.source, &ctx.syntax);
    builtins::run_reference(ctx);
}

pub(crate) fn push_reference_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Reference,
        entity,
        primary,
        message,
    ));
}
