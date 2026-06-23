use std::path::PathBuf;

use serde_json::Value as JsonValue;

use crate::diagnostics::{LintDiagnostic, LintStage, RototoRuleId};
use crate::error::Result;
use crate::lua_lint;

use super::super::engine::LintContext;
use super::super::stages::push_stage_diagnostic;
use super::targets::{
    registered_lint_output_anchor, registered_lint_targets, registered_lint_workspace,
};

pub(super) async fn register_pipeline_lint(
    lint_path: PathBuf,
    script: String,
) -> Result<Vec<lua_lint::RawCustomLintRegistration>> {
    lua_lint::register_pipeline_lint(lua_lint::RegisterLintInput { lint_path, script }).await
}

async fn lint_registered_target(
    workspace: JsonValue,
    target: JsonValue,
    lint_path: PathBuf,
    script: String,
    handler: String,
) -> Result<Vec<lua_lint::RegisteredCustomLintOutput>> {
    lua_lint::lint_registered_target(lua_lint::RegisteredLintInput {
        workspace,
        target,
        lint_path,
        script,
        handler,
    })
    .await
}

pub(crate) async fn run_registered_custom_lints(ctx: &mut LintContext, stage: LintStage) {
    let registrations = ctx
        .index
        .custom_lints
        .registrations
        .iter()
        .filter(|registration| registration.stage == stage)
        .cloned()
        .collect::<Vec<_>>();

    for registration in registrations {
        let targets = registered_lint_targets(ctx, &registration.selector);
        let Some(file) = ctx
            .index
            .custom_lints
            .files
            .get(&registration.file_path)
            .cloned()
        else {
            continue;
        };
        let Some(document) = ctx.source.documents.get(&file.doc).cloned() else {
            continue;
        };
        let Some(definition) = ctx
            .index
            .custom_lints
            .rules
            .get(&registration.rule)
            .map(|rule| rule.definition.clone())
        else {
            continue;
        };
        let workspace = registered_lint_workspace(ctx);
        for target in targets {
            match lint_registered_target(
                workspace.clone(),
                target.data.clone(),
                std::path::PathBuf::from(&registration.file_path),
                document.text.clone(),
                registration.handler.clone(),
            )
            .await
            {
                Ok(outputs) => {
                    for output in outputs {
                        let anchor =
                            registered_lint_output_anchor(ctx, &target, output.path.as_deref());
                        ctx.diagnostics.push(LintDiagnostic::custom(
                            &definition,
                            stage,
                            anchor.target,
                            anchor.location,
                            output.message,
                        ));
                    }
                }
                Err(err) => push_stage_diagnostic(
                    &mut ctx.diagnostics,
                    stage,
                    RototoRuleId::CustomLintFailed,
                    target.target.clone(),
                    target.location.clone(),
                    format!(
                        "custom lint handler failed in {}: {err}",
                        registration.file_path
                    ),
                ),
            }
        }
    }
}
