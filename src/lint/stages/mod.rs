mod diagnostics;
mod discover;
mod graph;
mod parse;
mod policy;
mod project;
mod reference;
mod register;
mod value;

use crate::diagnostics::LintStage;
use crate::error::Result;

use super::custom::run_registered_custom_lints;
use super::engine::LintContext;

pub(super) use diagnostics::push_stage_diagnostic;
pub(super) use graph::push_graph_diagnostic;
pub(super) use project::push_project_diagnostic;
pub(super) use reference::push_reference_diagnostic;
pub(super) use register::push_register_diagnostic;
pub(super) use value::push_value_diagnostic;

const CHECKED_STAGES: [LintStage; 5] = [
    LintStage::Project,
    LintStage::Reference,
    LintStage::Value,
    LintStage::Graph,
    LintStage::Policy,
];

pub(super) async fn run_pipeline(ctx: &mut LintContext) -> Result<()> {
    run_until(ctx, LintStage::Policy).await
}

pub(super) async fn run_until(ctx: &mut LintContext, stop_after: LintStage) -> Result<()> {
    discover::run(ctx).await?;
    if stop_after == LintStage::Discover {
        return Ok(());
    }

    parse::run(ctx);
    if stop_after == LintStage::Parse {
        return Ok(());
    }

    project::build_projection(ctx);
    register::run(ctx).await;
    if stop_after == LintStage::Register {
        return Ok(());
    }

    for stage in CHECKED_STAGES {
        run_builtin_stage(ctx, stage);
        run_custom_stage(ctx, stage).await;
        if stop_after == stage {
            return Ok(());
        }
    }

    Ok(())
}

fn run_builtin_stage(ctx: &mut LintContext, stage: LintStage) {
    match stage {
        LintStage::Project => project::run_builtin(ctx),
        LintStage::Reference => reference::run_builtin(ctx),
        LintStage::Value => value::run_builtin(ctx),
        LintStage::Graph => graph::run_builtin(ctx),
        LintStage::Policy => policy::run_builtin(ctx),
        LintStage::Discover | LintStage::Parse | LintStage::Register => {}
    }
}

async fn run_custom_stage(ctx: &mut LintContext, stage: LintStage) {
    match stage {
        LintStage::Project
        | LintStage::Reference
        | LintStage::Value
        | LintStage::Graph
        | LintStage::Policy => run_registered_custom_lints(ctx, stage).await,
        LintStage::Discover | LintStage::Parse | LintStage::Register => {}
    }
}
