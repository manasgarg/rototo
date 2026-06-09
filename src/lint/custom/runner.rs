use std::path::PathBuf;

use serde_json::Value as JsonValue;

use crate::diagnostics::{LintDiagnostic, LintStage, RototoRuleId};
use crate::error::Result;
use crate::lua_lint;

use super::super::engine::LintContext;
use super::super::stages::push_stage_diagnostic;
use super::marshal::{lint_stage_label, registered_lint_entity_label};
use super::registry::parse_registered_lint_output_field;
use super::targets::{
    registered_lint_output_location, registered_lint_output_target, registered_lint_targets,
};

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
        for target in targets {
            match lint_registered_target(
                stage,
                registered_lint_entity_label(registration.selector.entity).to_owned(),
                target.data.clone(),
                ctx.source.root.join(&registration.file_path),
                document.text.clone(),
                registration.handler.clone(),
            )
            .await
            {
                Ok(outputs) => {
                    for output in outputs {
                        let output_field = output.field.as_deref().and_then(|field| {
                            parse_registered_lint_output_field(registration.selector.entity, field)
                        });
                        let location =
                            registered_lint_output_location(ctx, &target, output_field.as_ref());
                        ctx.diagnostics.push(LintDiagnostic::custom(
                            &definition,
                            stage,
                            registered_lint_output_target(&target, output_field.as_ref()),
                            location,
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
