use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RototoRuleId, SemanticTarget,
};

use super::super::builtins;
use super::super::engine::LintContext;

pub(super) fn run_builtin(ctx: &mut LintContext) {
    builtins::run_graph(ctx);
}

pub(crate) fn push_graph_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    target: impl Into<SemanticTarget>,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Graph,
        target,
        primary,
        message,
    ));
}
