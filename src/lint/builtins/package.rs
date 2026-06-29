use crate::diagnostics::{LintDiagnostic, LintStage, RototoRuleId};
use crate::package::validate_package_manifest;

use super::super::engine::LintContext;
use super::super::index::ProjectField;
use super::super::stages::push_project_diagnostic;

pub(super) fn lint_manifest_shape(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };
    let Some(parsed) = ctx.syntax.toml.get(&manifest.doc) else {
        return;
    };

    if let Err(err) = validate_package_manifest(&parsed.to_plain_toml()) {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::PackageManifestSchemaFailed,
            LintStage::Project,
            manifest.target(),
            manifest.location.clone(),
            err.to_string(),
        ));
    }
}

/// Validate `[[trace]]` policies: each must declare a `when` that parses as a
/// boolean expression, and the expression may only reference identifiers rototo
/// provides (including `env.resolving.*`, which is unique to trace policies).
pub(super) fn lint_trace_policies(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };
    let diagnostics = &mut ctx.diagnostics;
    for policy in &manifest.trace {
        match &policy.when {
            ProjectField::Present(when) => {
                for issue in &when.value.references().invalid_roots {
                    push_project_diagnostic(
                        diagnostics,
                        RototoRuleId::TraceWhenInvalidReference,
                        manifest.target(),
                        when.location.clone(),
                        issue.describe(),
                    );
                }
            }
            ProjectField::Invalid { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::TraceWhenShape,
                manifest.target(),
                location.clone(),
                format!("trace policy {} when expression is invalid", policy.index),
            ),
            ProjectField::Missing { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::TraceWhenMissing,
                manifest.target(),
                location.clone(),
                format!("trace policy {} must declare when", policy.index),
            ),
        }
    }
}
