mod ids;
mod nodes;
mod targets;

use std::collections::BTreeMap;

pub(super) use ids::{
    CatalogId, EvaluationContextId, EvaluationContextSampleId, ValueKey, VariableId,
};
pub(super) use nodes::*;
pub(super) use targets::{RegisteredLintAddress, RegisteredLintSelector};

#[derive(Default)]
pub(super) struct SemanticIndex {
    pub(super) manifest: Option<ManifestNode>,
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
