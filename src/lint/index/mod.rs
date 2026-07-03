mod ids;
mod nodes;
mod targets;

use std::collections::BTreeMap;

pub(super) use ids::{
    CatalogId, EnumId, EvaluationContextId, EvaluationContextSampleId, LayerId, ValueKey,
    VariableId,
};
pub(super) use nodes::*;
pub(super) use targets::{RegisteredLintAddress, RegisteredLintSelector};

#[derive(Default)]
pub(super) struct SemanticIndex {
    pub(super) manifest: Option<ManifestNode>,
    pub(super) enums: BTreeMap<EnumId, EnumNode>,
    pub(super) enum_members: BTreeMap<EnumId, EnumMembersNode>,
    pub(super) layers: BTreeMap<LayerId, LayerNode>,
    pub(super) variables: BTreeMap<VariableId, VariableNode>,
    pub(super) catalogs: BTreeMap<CatalogId, CatalogNode>,
    pub(super) catalog_entries: BTreeMap<CatalogId, BTreeMap<ValueKey, CatalogEntryNode>>,
    pub(super) evaluation_contexts: BTreeMap<EvaluationContextId, EvaluationContextNode>,
    pub(super) evaluation_context_samples: BTreeMap<
        EvaluationContextId,
        BTreeMap<EvaluationContextSampleId, EvaluationContextSampleNode>,
    >,
    pub(super) custom_lints: CustomLintRegistry,
}
