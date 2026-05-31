use std::path::PathBuf;

use serde_json::Value as JsonValue;

use crate::diagnostics::{LintDiagnostic, LintStage, RototoRuleId};
use crate::error::Result;
use crate::lua_lint;

use super::super::engine::LintContext;
use super::super::stages::push_stage_diagnostic;
use super::marshal::{lint_stage_label, registered_lint_entity_label};
use super::targets::registered_lint_targets;

pub(super) async fn register_pipeline_lint(
    lint_path: PathBuf,
    script: String,
) -> Result<Vec<lua_lint::RawCustomLintRegistration>> {
    lua_lint::register_pipeline_lint(lua_lint::RegisterLintInput { lint_path, script }).await
}

async fn lint_registered_target(
    stage: LintStage,
    entity: String,
    data: JsonValue,
    lint_path: PathBuf,
    script: String,
    handler: String,
) -> Result<Vec<lua_lint::RegisteredCustomLintOutput>> {
    lua_lint::lint_registered_target(lua_lint::RegisteredLintInput {
        stage: lint_stage_label(stage).to_owned(),
        target: lua_lint::RegisteredLintTarget { entity, data },
        lint_path,
        script,
        handler,
    })
    .await
}

pub(crate) async fn run_registered_custom_lints(ctx: &mut LintContext, stage: LintStage) {
    let registrations = ctx
        .registered_custom_lints
        .iter()
        .filter(|registration| registration.stage == stage)
        .cloned()
        .collect::<Vec<_>>();

    for registration in registrations {
        let targets = registered_lint_targets(ctx, &registration.selector);
        for target in targets {
            match lint_registered_target(
                stage,
                registered_lint_entity_label(registration.selector.entity).to_owned(),
                target.data,
                ctx.source.root.join(&registration.file_path),
                registration.script.clone(),
                registration.handler.clone(),
            )
            .await
            {
                Ok(outputs) => {
                    for output in outputs {
                        ctx.diagnostics.push(LintDiagnostic::custom(
                            &registration.definition,
                            stage,
                            target.entity.clone(),
                            target.location.clone(),
                            output.message,
                        ));
                    }
                }
                Err(err) => push_stage_diagnostic(
                    &mut ctx.diagnostics,
                    stage,
                    RototoRuleId::CustomLintFailed,
                    target.entity.clone(),
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
