use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RototoRuleId};

use super::super::builtins;
use super::super::engine::LintContext;

pub(super) fn run_builtin(ctx: &mut LintContext) {
    builtins::run_graph(ctx);
}

pub(crate) fn push_graph_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Graph,
        entity,
        primary,
        message,
    ));
}
