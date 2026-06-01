use crate::diagnostics::{DiagnosticRule, LintStage, RototoRuleId};

use super::super::engine::LintContext;
use super::super::index::GateEntity;
use super::super::source::{DocumentKind, SourceDocument};
use super::super::syntax::parse_sources;

pub(super) fn run(ctx: &mut LintContext) {
    ctx.syntax = parse_sources(&ctx.source, &mut ctx.diagnostics);
    gate_unparsed_documents(ctx);
}

fn gate_unparsed_documents(ctx: &mut LintContext) {
    for document in ctx.source.documents.values() {
        if !document_needs_parse_gate(ctx, document) {
            continue;
        }
        let Some(entity) = gate_entity_for_document(document) else {
            continue;
        };
        ctx.index.gates.block(
            entity,
            LintStage::Parse,
            Some(DiagnosticRule::Rototo(parse_failed_rule(&document.kind))),
        );
    }
}

fn document_needs_parse_gate(ctx: &LintContext, document: &SourceDocument) -> bool {
    if document.read_error.is_some() && !matches!(&document.kind, DocumentKind::CustomLint) {
        return true;
    }
    match &document.kind {
        DocumentKind::Manifest
        | DocumentKind::Qualifier { .. }
        | DocumentKind::Variable { .. }
        | DocumentKind::Resource { .. }
        | DocumentKind::ResourceObject { .. } => !ctx.syntax.toml.contains_key(&document.id),
        DocumentKind::Schema => !ctx.syntax.json.contains_key(&document.id),
        DocumentKind::CustomLint => false,
    }
}

fn gate_entity_for_document(document: &SourceDocument) -> Option<GateEntity> {
    match &document.kind {
        DocumentKind::Manifest => Some(GateEntity::Manifest),
        DocumentKind::Qualifier { id } => Some(GateEntity::Qualifier(id.clone())),
        DocumentKind::Variable { id } => Some(GateEntity::Variable(id.clone())),
        DocumentKind::Resource { id } => Some(GateEntity::Resource(id.clone())),
        DocumentKind::ResourceObject {
            resource_id,
            object_id,
        } => Some(GateEntity::ResourceObject {
            resource: resource_id.clone(),
            key: object_id.clone(),
        }),
        DocumentKind::Schema => Some(GateEntity::Schema(document.path.clone())),
        DocumentKind::CustomLint => None,
    }
}

fn parse_failed_rule(kind: &DocumentKind) -> RototoRuleId {
    match kind {
        DocumentKind::Manifest => RototoRuleId::WorkspaceManifestParseFailed,
        DocumentKind::Qualifier { .. } => RototoRuleId::QualifierParseFailed,
        DocumentKind::Variable { .. } => RototoRuleId::VariableParseFailed,
        DocumentKind::Resource { .. } => RototoRuleId::ResourceParseFailed,
        DocumentKind::ResourceObject { .. } => RototoRuleId::ResourceObjectParseFailed,
        DocumentKind::Schema => RototoRuleId::SchemaParseFailed,
        DocumentKind::CustomLint => RototoRuleId::CustomLintFailed,
    }
}
