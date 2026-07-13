mod ids;
mod nodes;
mod targets;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

pub(super) use ids::{
    CatalogId, EvaluationContextId, EvaluationContextSampleId, LayerId, ListId, ValueKey,
    VariableId,
};
pub(crate) use nodes::parse_arm_buckets;
pub(super) use nodes::*;
pub(super) use targets::RegisteredLintSelector;

#[derive(Default)]
pub(super) struct SemanticIndex {
    pub(super) manifest: Option<ManifestNode>,
    pub(super) lists: BTreeMap<ListId, ListNode>,
    pub(super) layers: BTreeMap<LayerId, LayerNode>,
    pub(super) governance: Option<GovernanceNode>,
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
