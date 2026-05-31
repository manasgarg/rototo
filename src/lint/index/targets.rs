#[derive(Clone)]
pub(in crate::lint) struct RegisteredLintSelector {
    pub(in crate::lint) entity: RegisteredLintEntity,
    pub(in crate::lint) field: Option<RegisteredLintField>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::lint) enum RegisteredLintEntity {
    Workspace,
    Qualifier,
    Variable,
    Value,
    Schema,
}

#[derive(Clone)]
pub(in crate::lint) enum RegisteredLintField {
    Workspace(WorkspaceLintField),
    Qualifier(QualifierLintField),
    Variable(VariableLintField),
    Value(ValueLintField),
    Schema(SchemaLintField),
}

#[derive(Clone)]
pub(in crate::lint) enum WorkspaceLintField {
    Environments,
    ContextSchema,
}

#[derive(Clone)]
pub(in crate::lint) enum QualifierLintField {
    Id,
    Description,
    Predicates,
}

#[derive(Clone)]
pub(in crate::lint) enum VariableLintField {
    Id,
    Description,
    Type,
    Schema,
    Values,
    Environments,
}

#[derive(Clone)]
pub(in crate::lint) enum ValueLintField {
    Key,
    Value,
    JsonPath(Vec<String>),
}

#[derive(Clone)]
pub(in crate::lint) enum SchemaLintField {
    Json,
    JsonPath(Vec<String>),
}
