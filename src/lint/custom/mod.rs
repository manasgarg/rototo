mod marshal;
mod registry;
mod runner;
mod targets;

use crate::diagnostics::{CustomRuleDefinition, LintStage};

pub(super) use super::index::{
    QualifierLintField, RegisteredLintEntity, RegisteredLintField, RegisteredLintSelector,
    SchemaLintField, ValueLintField, VariableLintField, WorkspaceLintField,
};
pub(super) use registry::register_custom_lints;
pub(super) use runner::run_registered_custom_lints;

#[derive(Clone)]
pub(super) struct RegisteredCustomLint {
    pub(super) file_path: String,
    pub(super) script: String,
    pub(super) stage: LintStage,
    pub(super) selector: RegisteredLintSelector,
    pub(super) definition: CustomRuleDefinition,
    pub(super) handler: String,
}
