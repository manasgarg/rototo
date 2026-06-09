use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RototoRuleId, SemanticTarget,
};

use super::super::custom::register_custom_lints;
use super::super::engine::LintContext;

pub(super) async fn run(ctx: &mut LintContext) {
    register_custom_lints(ctx).await;
}

pub(crate) fn push_register_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    target: impl Into<SemanticTarget>,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Register,
        target,
        primary,
        message,
    ));
}
