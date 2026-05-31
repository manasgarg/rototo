mod marshal;
mod registry;
mod runner;
mod targets;

use crate::diagnostics::{CustomRuleDefinition, LintStage};

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

#[derive(Clone)]
pub(super) struct RegisteredLintSelector {
    pub(super) entity: RegisteredLintEntity,
    pub(super) field: Option<RegisteredLintField>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RegisteredLintEntity {
    Workspace,
    Qualifier,
    Variable,
    Value,
    Schema,
}

#[derive(Clone)]
pub(super) enum RegisteredLintField {
    Workspace(WorkspaceLintField),
    Qualifier(QualifierLintField),
    Variable(VariableLintField),
    Value(ValueLintField),
    Schema(SchemaLintField),
}

#[derive(Clone)]
pub(super) enum WorkspaceLintField {
    Environments,
    ContextSchema,
}

#[derive(Clone)]
pub(super) enum QualifierLintField {
    Id,
    Description,
    Predicates,
}

#[derive(Clone)]
pub(super) enum VariableLintField {
    Id,
    Description,
    Type,
    Schema,
    Values,
    Environments,
}

#[derive(Clone)]
pub(super) enum ValueLintField {
    Key,
    Value,
    JsonPath(Vec<String>),
}

#[derive(Clone)]
pub(super) enum SchemaLintField {
    Json,
    JsonPath(Vec<String>),
}
