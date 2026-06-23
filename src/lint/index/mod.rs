mod ids;
mod nodes;
mod targets;

use std::collections::BTreeMap;

pub(super) use ids::{
    CatalogId, QualifierId, RequestContextEntryId, RequestContextId, ValueKey, VariableId,
};
pub(super) use nodes::*;
pub(super) use targets::{RegisteredLintAddress, RegisteredLintSelector};

#[derive(Default)]
pub(super) struct SemanticIndex {
    pub(super) manifest: Option<ManifestNode>,
    pub(super) qualifiers: BTreeMap<QualifierId, QualifierNode>,
    pub(super) variables: BTreeMap<VariableId, VariableNode>,
    pub(super) catalogs: BTreeMap<CatalogId, CatalogNode>,
    pub(super) catalog_entries: BTreeMap<CatalogId, BTreeMap<ValueKey, CatalogEntryNode>>,
    pub(super) request_contexts: BTreeMap<RequestContextId, RequestContextNode>,
    pub(super) request_context_entries:
        BTreeMap<RequestContextId, BTreeMap<RequestContextEntryId, RequestContextEntryNode>>,
    pub(super) custom_lints: CustomLintRegistry,
}
