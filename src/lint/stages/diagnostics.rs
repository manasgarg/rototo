use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RototoRuleId, SemanticTarget,
};

pub(crate) fn push_stage_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    stage: LintStage,
    rule: RototoRuleId,
    target: impl Into<SemanticTarget>,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule, stage, target, primary, message,
    ));
}
