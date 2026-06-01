use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RototoRuleId};

use super::super::custom::register_custom_lints;
use super::super::engine::LintContext;

pub(super) async fn run(ctx: &mut LintContext) {
    register_custom_lints(ctx).await;
}

pub(crate) fn push_register_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Register,
        entity,
        primary,
        message,
    ));
}
