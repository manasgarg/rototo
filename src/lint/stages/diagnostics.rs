use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RototoRuleId};

pub(crate) fn push_stage_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    stage: LintStage,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule, stage, entity, primary, message,
    ));
}
