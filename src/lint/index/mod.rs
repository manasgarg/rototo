mod gates;
mod ids;
mod nodes;
mod targets;

use std::collections::BTreeMap;

pub(super) use gates::{GateEntity, GateIndex};
pub(super) use ids::{CatalogId, QualifierId, ValueKey, VariableId, WorkspacePath};
pub(super) use nodes::*;
pub(super) use targets::{
    QualifierLintField, RegisteredLintEntity, RegisteredLintField, RegisteredLintSelector,
    SchemaLintField, ValueLintField, VariableLintField, WorkspaceLintField,
};

#[derive(Default)]
pub(super) struct SemanticIndex {
    pub(super) manifest: Option<ManifestNode>,
    pub(super) qualifiers: BTreeMap<QualifierId, QualifierNode>,
    pub(super) variables: BTreeMap<VariableId, VariableNode>,
    pub(super) catalogs: BTreeMap<CatalogId, CatalogNode>,
    pub(super) catalog_entries: BTreeMap<CatalogId, BTreeMap<ValueKey, CatalogEntryNode>>,
    pub(super) schemas: BTreeMap<WorkspacePath, SchemaNode>,
    pub(super) custom_lints: CustomLintRegistry,
    #[allow(dead_code)]
    pub(super) gates: GateIndex,
}
