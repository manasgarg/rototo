use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RototoRuleId};

use super::super::builtins;
use super::super::engine::LintContext;

pub(super) fn run_builtin(ctx: &mut LintContext) {
    builtins::run_value(ctx);
}

pub(crate) fn push_value_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Value,
        entity,
        primary,
        message,
    ));
}
