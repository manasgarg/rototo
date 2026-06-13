#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::diagnostics::{DiagnosticRule, LintStage};

use super::ids::{CatalogId, QualifierId, ValueKey, VariableId, WorkspacePath};

#[derive(Default)]
pub(in crate::lint) struct GateIndex {
    pub(in crate::lint) entity_state: BTreeMap<GateEntity, GateState>,
}

pub(in crate::lint) struct GateState {
    pub(in crate::lint) blocked_at: LintStage,
    pub(in crate::lint) diagnostic: Option<DiagnosticRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::lint) enum GateEntity {
    Manifest,
    Qualifier(QualifierId),
    Variable(VariableId),
    Catalog(CatalogId),
    CatalogEntry { catalog: CatalogId, key: ValueKey },
    Schema(WorkspacePath),
    CustomLintFile(WorkspacePath),
}

impl GateIndex {
    pub(in crate::lint) fn block(
        &mut self,
        entity: GateEntity,
        stage: LintStage,
        diagnostic: Option<DiagnosticRule>,
    ) {
        self.entity_state
            .entry(entity)
            .or_insert_with(|| GateState {
                blocked_at: stage,
                diagnostic,
            });
    }

    pub(in crate::lint) fn is_blocked(&self, entity: &GateEntity) -> bool {
        self.entity_state.contains_key(entity)
    }
}
