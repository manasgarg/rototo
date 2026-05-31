use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml_edit::ImDocument;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, DiagnosticRule, DocId, EntityId,
    LintDiagnostic, LintStage, RelatedLocation, RototoRuleId, Severity, SourcePosition,
    SourceRange,
};
use crate::error::{Result, RototoError};
use crate::model::{QualifierLint, VariableLint, WorkspaceLint};
use crate::workspace::workspace_environments;

mod custom;
pub(crate) mod input;
mod nodes;
mod project;
mod source;
mod syntax;

use custom::{RegisteredCustomLint, register_custom_lints, run_registered_custom_lints};
pub(crate) use input::{LintInput, OverlayDocument};
use nodes::*;
use project::{project_external_value, project_manifest, project_qualifier, project_variable};
use source::{DocumentCollection, DocumentKind, SourceStore, workspace_path};
use syntax::{
    ParsedToml, SyntaxIndex, json_parse_diagnostic, read_error_diagnostic,
    toml_de_parse_diagnostic, toml_edit_parse_diagnostic,
};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";
const CUSTOM_LINT_FIELD_SELECTORS: &[&str] = &[
    "context_schema",
    "description",
    "environments",
    "id",
    "json",
    "json.",
    "key",
    "predicates",
    "schema",
    "type",
    "value",
    "value.",
    "values",
];

pub async fn lint_workspace(workspace_root: &Path) -> Result<WorkspaceLint> {
    lint_workspace_with_input(LintInput::new(workspace_root.to_path_buf())).await
}

pub async fn lint_qualifier(workspace_root: &Path, id: &str) -> Result<QualifierLint> {
    let lint = lint_workspace(workspace_root).await?;
    let path = format!("qualifiers/{id}.toml");
    if !lint.documents.iter().any(|document| document.path == path) {
        return Err(RototoError::new(format!(
            "qualifier not found: qualifier://{id}"
        )));
    }

    Ok(QualifierLint {
        root: lint.root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic_belongs_to_qualifier(diagnostic, id, &path))
            .collect(),
    })
}

pub async fn lint_variable(workspace_root: &Path, id: &str) -> Result<VariableLint> {
    let lint = lint_workspace(workspace_root).await?;
    let path = format!("variables/{id}.toml");
    if !lint.documents.iter().any(|document| document.path == path) {
        return Err(RototoError::new(format!(
            "variable not found: variable://{id}"
        )));
    }

    Ok(VariableLint {
        root: lint.root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic_belongs_to_variable(diagnostic, id, &path))
            .collect(),
    })
}

fn diagnostic_belongs_to_qualifier(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    matches!(&diagnostic.entity, EntityId::Qualifier { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Predicate { qualifier, .. } if qualifier == id)
        || diagnostic.primary.path == path
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str, path: &str) -> bool {
    matches!(&diagnostic.entity, EntityId::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.entity, EntityId::EnvironmentBlock { variable, .. } if variable == id)
        || matches!(&diagnostic.entity, EntityId::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == path
}

pub(crate) async fn lint_workspace_with_input(input: LintInput) -> Result<WorkspaceLint> {
    Ok(lint_workspace_snapshot(input).await?.lint)
}

pub(crate) async fn lint_workspace_snapshot(input: LintInput) -> Result<WorkspaceLintSnapshot> {
    LintEngine::new().lint_workspace(input).await
}

pub(crate) struct WorkspaceLintSnapshot {
    pub(crate) lint: WorkspaceLint,
    index: SemanticIndex,
}

impl WorkspaceLintSnapshot {
    pub(crate) fn document_symbols(&self, path: &str) -> Vec<WorkspaceDocumentSymbol> {
        let mut symbols = Vec::new();

        if let Some(manifest) = &self.index.manifest
            && manifest.location.path == path
            && let Some(symbol) = workspace_environments_symbol(&manifest.environments)
        {
            symbols.push(symbol);
        }

        for qualifier in self.index.qualifiers.values() {
            if qualifier.location.path == path {
                symbols.push(qualifier_document_symbol(qualifier));
            }
        }

        for variable in self.index.variables.values() {
            if variable.location.path == path {
                symbols.push(variable_document_symbol(variable));
            }
        }

        for (variable_id, values) in &self.index.external_values {
            for value in values.values() {
                if value.location.path == path {
                    symbols.push(external_value_document_symbol(variable_id, value));
                }
            }
        }

        sort_workspace_document_symbols(&mut symbols);
        symbols
    }

    pub(crate) fn completion_items(&self, path: &str) -> Vec<WorkspaceCompletionItem> {
        let mut items = Vec::new();

        if let Some(manifest) = &self.index.manifest {
            items.extend(workspace_environment_completion_items(
                &manifest.environments,
            ));
        }
        items.extend(qualifier_completion_items(&self.index));
        items.extend(current_variable_value_completion_items(&self.index, path));
        items.extend(predicate_operator_completion_items());
        items.extend(custom_lint_field_selector_completion_items());

        sort_and_deduplicate_workspace_completion_items(&mut items);
        items
    }

    pub(crate) fn hover(&self, path: &str, position: SourcePosition) -> Option<WorkspaceHover> {
        let mut candidates = Vec::new();
        push_diagnostic_hover_candidates(self, path, position, &mut candidates);
        push_manifest_hover_candidates(&self.index, path, position, &mut candidates);
        push_qualifier_hover_candidates(&self.index, path, position, &mut candidates);
        push_variable_hover_candidates(&self.index, path, position, &mut candidates);
        sort_hover_candidates(&mut candidates);
        candidates
            .into_iter()
            .next()
            .map(|candidate| candidate.hover)
            .or_else(|| file_hover(&self.index, path))
    }

    pub(crate) fn definition(
        &self,
        path: &str,
        position: SourcePosition,
    ) -> Option<WorkspaceDefinition> {
        let mut candidates = Vec::new();
        push_manifest_definition_candidates(&self.index, path, position, &mut candidates);
        push_qualifier_definition_candidates(&self.index, path, position, &mut candidates);
        push_variable_definition_candidates(&self.index, path, position, &mut candidates);
        sort_definition_candidates(&mut candidates);
        candidates
            .into_iter()
            .next()
            .and_then(|candidate| self.definition_for_location(candidate.location))
    }

    pub(crate) fn references(
        &self,
        path: &str,
        position: SourcePosition,
        include_declaration: bool,
    ) -> Vec<WorkspaceReference> {
        let Some(target) = reference_target_at_position(&self.index, path, position) else {
            return Vec::new();
        };
        let mut references = reference_locations_for_target(&self.index, &target);
        if include_declaration
            && let Some(declaration) = reference_target_declaration(&self.index, &target)
        {
            references.push(declaration);
        }
        self.references_from_locations(references)
    }

    fn definition_for_location(
        &self,
        mut location: DiagnosticLocation,
    ) -> Option<WorkspaceDefinition> {
        let document = self
            .lint
            .documents
            .iter()
            .find(|document| document.path == location.path)?;
        location.doc = Some(document.id);
        let uri = document.uri.clone();
        Some(WorkspaceDefinition { uri, location })
    }

    fn references_from_locations(
        &self,
        locations: Vec<DiagnosticLocation>,
    ) -> Vec<WorkspaceReference> {
        let mut references = locations
            .into_iter()
            .filter_map(|mut location| {
                let document = self
                    .lint
                    .documents
                    .iter()
                    .find(|document| document.path == location.path)?;
                location.doc = Some(document.id);
                Some(WorkspaceReference {
                    uri: document.uri.clone(),
                    location,
                })
            })
            .collect::<Vec<_>>();
        sort_and_deduplicate_workspace_references(&mut references);
        references
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDocumentSymbol {
    pub(crate) name: String,
    pub(crate) kind: WorkspaceDocumentSymbolKind,
    pub(crate) location: DiagnosticLocation,
    pub(crate) selection_location: DiagnosticLocation,
    pub(crate) children: Vec<WorkspaceDocumentSymbol>,
}

impl WorkspaceDocumentSymbol {
    fn new(
        name: impl Into<String>,
        kind: WorkspaceDocumentSymbolKind,
        location: DiagnosticLocation,
        children: Vec<Self>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            selection_location: location.clone(),
            location,
            children,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceDocumentSymbolKind {
    WorkspaceEnvironments,
    Environment,
    Qualifier,
    Predicate,
    Variable,
    Values,
    Value,
    EnvironmentBlock,
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceCompletionItem {
    pub(crate) label: String,
    pub(crate) kind: WorkspaceCompletionItemKind,
    pub(crate) detail: &'static str,
}

impl WorkspaceCompletionItem {
    fn new(
        label: impl Into<String>,
        kind: WorkspaceCompletionItemKind,
        detail: &'static str,
    ) -> Self {
        Self {
            label: label.into(),
            kind,
            detail,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceCompletionItemKind {
    Environment,
    Qualifier,
    Value,
    PredicateOperator,
    FieldSelector,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceHover {
    pub(crate) contents: String,
    pub(crate) location: DiagnosticLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDefinition {
    pub(crate) uri: String,
    pub(crate) location: DiagnosticLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceReference {
    pub(crate) uri: String,
    pub(crate) location: DiagnosticLocation,
}

struct LintEngine;

impl LintEngine {
    fn new() -> Self {
        Self
    }

    async fn lint_workspace(&self, input: LintInput) -> Result<WorkspaceLintSnapshot> {
        let mut ctx = LintContext::new(input);

        self.run_discover(&mut ctx).await?;
        self.run_parse(&mut ctx);
        self.build_projection(&mut ctx);
        self.run_project(&mut ctx);
        self.run_register(&mut ctx).await;
        self.run_registered_custom_lints(&mut ctx, LintStage::Project)
            .await;
        self.run_reference(&mut ctx);
        self.run_registered_custom_lints(&mut ctx, LintStage::Reference)
            .await;
        self.run_value(&mut ctx);
        self.run_registered_custom_lints(&mut ctx, LintStage::Value)
            .await;
        self.run_graph(&mut ctx);
        self.run_registered_custom_lints(&mut ctx, LintStage::Graph)
            .await;
        self.run_registered_custom_lints(&mut ctx, LintStage::Policy)
            .await;

        Ok(ctx.finish())
    }

    async fn run_discover(&self, ctx: &mut LintContext) -> Result<()> {
        let root = match tokio::fs::canonicalize(&ctx.input.root).await {
            Ok(root) => root,
            Err(err) => {
                ctx.diagnostics.push(LintDiagnostic::rototo(
                    RototoRuleId::WorkspaceNotFound,
                    LintStage::Discover,
                    EntityId::Workspace,
                    DiagnosticLocation::workspace_root(ctx.input.root.display().to_string()),
                    err.to_string(),
                ));
                return Ok(());
            }
        };

        let metadata = match tokio::fs::metadata(&root).await {
            Ok(metadata) => metadata,
            Err(err) => {
                ctx.diagnostics.push(LintDiagnostic::rototo(
                    RototoRuleId::WorkspaceNotFound,
                    LintStage::Discover,
                    EntityId::Workspace,
                    DiagnosticLocation::workspace_root(root.display().to_string()),
                    err.to_string(),
                ));
                return Ok(());
            }
        };

        if !metadata.is_dir() {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::WorkspaceNotFound,
                LintStage::Discover,
                EntityId::Workspace,
                DiagnosticLocation::workspace_root(root.display().to_string()),
                "workspace path is not a directory",
            ));
            return Ok(());
        }

        ctx.source.root = root;
        let manifest_path = PathBuf::from(WORKSPACE_MANIFEST);
        if tokio::fs::metadata(ctx.source.root.join(&manifest_path))
            .await
            .is_ok_and(|metadata| metadata.is_file())
        {
            ctx.source
                .add_disk_document(manifest_path, DocumentKind::Manifest)
                .await;
        } else {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::WorkspaceManifestMissing,
                LintStage::Discover,
                EntityId::Workspace,
                DiagnosticLocation::workspace_root(ctx.source.root.display().to_string()),
                "workspace manifest is missing",
            ));
            return Ok(());
        }

        ctx.source
            .add_named_toml_documents("qualifiers", DocumentCollection::Qualifiers)
            .await?;
        ctx.source
            .add_named_toml_documents("variables", DocumentCollection::Variables)
            .await?;
        ctx.source.add_schema_documents().await?;
        ctx.source.add_custom_lint_documents().await?;

        Ok(())
    }

    fn run_parse(&self, ctx: &mut LintContext) {
        for document in ctx.source.documents.values() {
            if let Some(read_error) = &document.read_error {
                if !matches!(&document.kind, DocumentKind::CustomLint) {
                    ctx.diagnostics
                        .push(read_error_diagnostic(document, read_error));
                }
                continue;
            }

            match &document.kind {
                DocumentKind::Manifest
                | DocumentKind::Qualifier { .. }
                | DocumentKind::Variable { .. }
                | DocumentKind::ExternalValue { .. } => {
                    match ImDocument::parse(document.text.clone()) {
                        Ok(edit) => match document.text.parse::<TomlValue>() {
                            Ok(plain) => {
                                ctx.syntax
                                    .toml
                                    .insert(document.id, ParsedToml { edit, plain });
                            }
                            Err(err) => {
                                ctx.diagnostics
                                    .push(toml_de_parse_diagnostic(document, &err));
                            }
                        },
                        Err(err) => {
                            ctx.diagnostics
                                .push(toml_edit_parse_diagnostic(document, &err));
                        }
                    }
                }
                DocumentKind::Schema => match serde_json::from_str::<JsonValue>(&document.text) {
                    Ok(value) => {
                        ctx.syntax.json.insert(document.id, value);
                    }
                    Err(err) => {
                        ctx.diagnostics.push(json_parse_diagnostic(document, &err));
                    }
                },
                DocumentKind::CustomLint => {}
            }
        }
    }

    fn build_projection(&self, ctx: &mut LintContext) {
        for document in ctx.source.documents.values() {
            let Some(toml) = ctx.syntax.toml.get(&document.id) else {
                continue;
            };

            match &document.kind {
                DocumentKind::Manifest => {
                    ctx.index.manifest = Some(project_manifest(document, toml));
                }
                DocumentKind::Qualifier { id } => {
                    ctx.index
                        .qualifiers
                        .insert(id.clone(), project_qualifier(document, toml, id));
                }
                DocumentKind::Variable { id } => {
                    ctx.index.variables.insert(
                        id.clone(),
                        project_variable(document, toml, id, &ctx.source),
                    );
                }
                DocumentKind::ExternalValue {
                    variable_id,
                    value_key,
                } => {
                    ctx.index
                        .external_values
                        .entry(variable_id.clone())
                        .or_default()
                        .insert(
                            value_key.clone(),
                            project_external_value(document, toml, value_key),
                        );
                }
                DocumentKind::Schema | DocumentKind::CustomLint => {}
            }
        }
    }

    fn run_project(&self, ctx: &mut LintContext) {
        lint_manifest_shape(ctx);
        lint_manifest_custom_rule_shapes(ctx);
        lint_qualifier_shapes(ctx);
        lint_variable_shapes(ctx);
        lint_custom_rule_conflicts(ctx);
    }

    async fn run_register(&self, ctx: &mut LintContext) {
        register_custom_lints(ctx).await;
    }

    fn run_reference(&self, ctx: &mut LintContext) {
        lint_context_schema_reference(ctx);
        lint_qualifier_context_schema_attributes(ctx);
        lint_qualifier_references(ctx);
        lint_variable_references(ctx);
    }

    fn run_value(&self, ctx: &mut LintContext) {
        lint_schema_documents(ctx);
        lint_variable_values(ctx);
    }

    fn run_graph(&self, ctx: &mut LintContext) {
        lint_qualifier_cycles(ctx);
        lint_unreferenced_qualifiers(ctx);
        lint_shadowed_variable_rules(ctx);
        lint_unused_variable_values(ctx);
    }

    async fn run_registered_custom_lints(&self, ctx: &mut LintContext, stage: LintStage) {
        run_registered_custom_lints(ctx, stage).await;
    }
}

struct LintContext {
    input: LintInput,
    source: SourceStore,
    syntax: SyntaxIndex,
    index: SemanticIndex,
    registered_custom_lints: Vec<RegisteredCustomLint>,
    diagnostics: Vec<LintDiagnostic>,
}

impl LintContext {
    fn new(input: LintInput) -> Self {
        let source = SourceStore::new(input.root.clone(), input.overlays.clone());
        Self {
            source,
            input,
            syntax: SyntaxIndex::default(),
            index: SemanticIndex::default(),
            registered_custom_lints: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn finish(mut self) -> WorkspaceLintSnapshot {
        sort_diagnostics(&mut self.diagnostics);
        let documents = self.source.document_summaries();
        let lint = WorkspaceLint {
            root: self.source.root,
            documents,
            diagnostics: self.diagnostics,
        };
        WorkspaceLintSnapshot {
            lint,
            index: self.index,
        }
    }
}

fn lint_manifest_shape(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };
    let Some(parsed) = ctx.syntax.toml.get(&manifest.doc) else {
        return;
    };

    if let Err(err) = workspace_environments(&parsed.plain) {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::WorkspaceManifestSchemaFailed,
            LintStage::Project,
            EntityId::Manifest,
            manifest.location.clone(),
            err.to_string(),
        ));
    }
}

fn lint_qualifier_shapes(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for qualifier in ctx.index.qualifiers.values() {
        if !field_is_integer(&qualifier.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierSchemaVersion,
                EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                qualifier.schema_version.location(),
                "qualifier must declare schema_version = 1",
            );
        }

        match &qualifier.predicates {
            PredicateCollection::Missing { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateMissing,
                EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                location.clone(),
                "qualifier must contain at least one [[predicate]]",
            ),
            PredicateCollection::Invalid { location } => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateShape,
                EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                location.clone(),
                "predicate must use [[predicate]] tables",
            ),
            PredicateCollection::Predicates(predicates) => {
                for predicate in predicates {
                    lint_predicate_shape(diagnostics, qualifier, predicate);
                }
            }
        }
    }
}

fn lint_predicate_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    qualifier: &QualifierNode,
    predicate: &PredicateNode,
) {
    let entity = EntityId::Predicate {
        qualifier: qualifier.id.clone(),
        index: predicate.index,
    };
    if field_is_not_present(&predicate.attribute) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateShape,
            entity.clone(),
            predicate.attribute.location(),
            "predicate must contain attribute",
        );
        return;
    }

    let op = match &predicate.op {
        ProjectField::Present(op) => &op.value,
        ProjectField::Invalid { location } | ProjectField::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateShape,
                entity,
                location.clone(),
                "predicate must contain op",
            );
            return;
        }
    };

    if let PredicateOp::Unknown(op) = op {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateUnknownOp,
            entity.clone(),
            predicate.op.location(),
            format!("predicate has unknown op: {op}"),
        );
    }

    if matches!(op, PredicateOp::Bucket) {
        lint_bucket_predicate(diagnostics, predicate, entity);
    } else {
        lint_comparison_predicate(diagnostics, predicate, op, entity);
    }
}

fn lint_bucket_predicate(
    diagnostics: &mut Vec<LintDiagnostic>,
    predicate: &PredicateNode,
    entity: EntityId,
) {
    if predicate.salt.as_ref().is_none_or(field_is_not_present) {
        let location = predicate
            .salt
            .as_ref()
            .map(ProjectField::location)
            .unwrap_or_else(|| predicate.location.clone());
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            location,
            "bucket predicate must contain salt",
        );
    }

    let Some(range) = &predicate.range else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            predicate.location.clone(),
            "bucket predicate must contain range",
        );
        return;
    };

    if !range.is_array {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            range.location.clone(),
            "bucket range must be a list",
        );
    } else if range.len != 2 {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity.clone(),
            range.location.clone(),
            "bucket range must contain two integers",
        );
    } else {
        match (range.start, range.end) {
            (Some(start), Some(end)) if 0 <= start && start < end && end <= 10_000 => {}
            _ => push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateBucket,
                entity.clone(),
                range.location.clone(),
                "bucket range must satisfy 0 <= start < end <= 10000",
            ),
        }
    }

    if predicate.has_bucket_value {
        let location = predicate
            .value
            .as_ref()
            .map(|value| value.location.clone())
            .unwrap_or_else(|| predicate.location.clone());
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateBucket,
            entity,
            location,
            "bucket predicate must not contain value",
        );
    }
}

fn lint_comparison_predicate(
    diagnostics: &mut Vec<LintDiagnostic>,
    predicate: &PredicateNode,
    op: &PredicateOp,
    entity: EntityId,
) {
    let Some(value) = &predicate.value else {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::QualifierPredicateValue,
            entity,
            predicate.location.clone(),
            "predicate must contain value",
        );
        return;
    };

    match op {
        PredicateOp::In | PredicateOp::NotIn if value.shape != ValueShape::Array => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateValue,
                entity,
                value.location.clone(),
                format!("{} predicate value must be a list", predicate_op_label(op)),
            );
        }
        PredicateOp::Gt | PredicateOp::Gte | PredicateOp::Lt | PredicateOp::Lte
            if !matches!(value.shape, ValueShape::Integer | ValueShape::Float) =>
        {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateValue,
                entity,
                value.location.clone(),
                format!(
                    "{} predicate value must be a number",
                    predicate_op_label(op)
                ),
            );
        }
        _ => {}
    }
}

fn lint_manifest_custom_rule_shapes(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };

    match &manifest.custom_rules {
        CustomRuleCollection::Invalid { location } => push_project_diagnostic(
            &mut ctx.diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            location.clone(),
            "workspace lint rule declarations must use [[lint.rule]] tables",
        ),
        CustomRuleCollection::Rules(rules) => {
            for rule in rules {
                lint_workspace_custom_rule_declaration_shape(&mut ctx.diagnostics, rule);
            }
        }
    }
}

fn lint_workspace_custom_rule_declaration_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: &CustomRuleDeclarationNode,
) {
    if field_is_not_present(&rule.id) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.id.location(),
            "custom lint rule must contain id",
        );
    }
    if field_is_not_present(&rule.title) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.title.location(),
            "custom lint rule must contain title",
        );
    }
    if field_is_not_present(&rule.help) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.help.location(),
            "custom lint rule must contain help",
        );
    }
    if let Some(ProjectField::Invalid { location }) = &rule.severity {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            location.clone(),
            "custom lint rule severity must be error or warning",
        );
    }

    if let ProjectField::Present(id) = &rule.id
        && let Err(err) = CustomRuleId::parse(&id.value)
    {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintInvalidRule,
            EntityId::Manifest,
            id.location.clone(),
            format!("custom lint rule id is invalid: {err}"),
        );
    }
}

fn lint_variable_shapes(ctx: &mut LintContext) {
    let diagnostics = &mut ctx.diagnostics;
    for variable in ctx.index.variables.values() {
        if !field_is_integer(&variable.schema_version, 1) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableSchemaVersion,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                variable.schema_version.location(),
                "variable must declare schema_version = 1",
            );
        }

        lint_type_source(diagnostics, variable);
        lint_values_shape(
            diagnostics,
            variable,
            ctx.index.external_values.get(&variable.id),
        );
        lint_environment_shapes(diagnostics, variable);
    }
}

fn lint_custom_rule_conflicts(ctx: &mut LintContext) {
    let mut declared: BTreeMap<CustomRuleId, CustomRuleDefinition> = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for (definition, location, entity) in custom_rule_definition_entries(ctx) {
        match declared.get(&definition.rule) {
            Some(existing) if !existing.same_metadata(&definition) => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::CustomLintRuleConflict,
                    entity,
                    location,
                    format!("custom lint rule metadata conflicts: {}", definition.rule),
                );
            }
            Some(_) => {}
            None => {
                declared.insert(definition.rule.clone(), definition);
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn custom_rule_definition_entries(
    ctx: &LintContext,
) -> Vec<(CustomRuleDefinition, DiagnosticLocation, EntityId)> {
    let mut definitions = Vec::new();

    if let Some(manifest) = &ctx.index.manifest {
        definitions.extend(
            custom_rule_definitions_from_collection(&manifest.custom_rules)
                .into_iter()
                .map(|(definition, location)| (definition, location, EntityId::Manifest)),
        );
    }

    definitions
}

fn workspace_custom_rule_definitions(
    ctx: &LintContext,
) -> BTreeMap<CustomRuleId, CustomRuleDefinition> {
    let Some(manifest) = &ctx.index.manifest else {
        return BTreeMap::new();
    };
    custom_rule_definitions_from_collection(&manifest.custom_rules)
        .into_iter()
        .map(|(definition, _)| (definition.rule.clone(), definition))
        .collect()
}

fn custom_rule_definitions_from_collection(
    rules: &CustomRuleCollection,
) -> Vec<(CustomRuleDefinition, DiagnosticLocation)> {
    let CustomRuleCollection::Rules(rules) = rules else {
        return Vec::new();
    };
    custom_rule_definitions_from_rules(rules)
}

fn custom_rule_definitions_from_rules(
    rules: &[CustomRuleDeclarationNode],
) -> Vec<(CustomRuleDefinition, DiagnosticLocation)> {
    rules
        .iter()
        .filter_map(|rule| {
            let (
                ProjectField::Present(id),
                ProjectField::Present(title),
                ProjectField::Present(help),
            ) = (&rule.id, &rule.title, &rule.help)
            else {
                return None;
            };
            let Ok(rule_id) = CustomRuleId::parse(&id.value) else {
                return None;
            };
            let severity = match &rule.severity {
                Some(ProjectField::Present(severity)) => severity.value,
                Some(ProjectField::Invalid { .. }) => return None,
                Some(ProjectField::Missing { .. }) | None => Severity::Error,
            };
            Some((
                CustomRuleDefinition::with_severity(
                    rule_id,
                    severity,
                    title.value.clone(),
                    help.value.clone(),
                ),
                rule.location.clone(),
            ))
        })
        .collect()
}

fn lint_type_source(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => {
            if !matches!(
                type_name.value.as_str(),
                "bool" | "int" | "number" | "string" | "list"
            ) {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::VariableUnknownType,
                    EntityId::Variable {
                        id: variable.id.clone(),
                    },
                    type_name.location.clone(),
                    format!("variable declares unknown type: {}", type_name.value),
                );
            }
        }
        TypeSourceNode::Schema(schema) => {
            let _ = &schema.value;
        }
        TypeSourceNode::Missing { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeOrSchema,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "variable must declare exactly one of type or schema",
        ),
        TypeSourceNode::Conflict { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeOrSchema,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "variable must declare exactly one of type or schema",
        ),
        TypeSourceNode::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableTypeOrSchema,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            location.clone(),
            "variable type source must be a string",
        ),
    }
}

fn lint_values_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    external_values: Option<&BTreeMap<String, ValueNode>>,
) {
    if variable.values.invalid_shape {
        if !variable.values.external_keys.is_empty() {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableExternalValuesLoadFailed,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                variable.values.location.clone(),
                "external values cannot be merged because variable values must be a table",
            );
            return;
        }

        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableValuesMissing,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            variable.values.location.clone(),
            "variable values must be a table",
        );
        return;
    }

    if variable.values.inline_keys.is_empty() && variable.values.external_keys.is_empty() {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableValuesMissing,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            variable.values.location.clone(),
            "variable must contain [values] or external values",
        );
    }

    lint_external_value_duplicates(diagnostics, variable, external_values);
}

fn lint_external_value_duplicates(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    external_values: Option<&BTreeMap<String, ValueNode>>,
) {
    let Some(external_values) = external_values else {
        return;
    };

    for (key, value) in external_values {
        if !variable.values.inline_keys.contains(key) {
            continue;
        }

        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableExternalValueDuplicate,
            EntityId::Value {
                variable: variable.id.clone(),
                key: key.clone(),
            },
            value.location.clone(),
            format!("external value duplicates inline value: {key}"),
        );
    }
}

fn lint_environment_shapes(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    let environments = match &variable.environments {
        EnvironmentCollection::Missing { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableEnvMissingDefault,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                location.clone(),
                "variable must contain [env._]",
            );
            return;
        }
        EnvironmentCollection::Invalid { location } => {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::VariableEnvShape,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                location.clone(),
                "env must be a table",
            );
            return;
        }
        EnvironmentCollection::Environments(environments) => environments,
    };

    if !environments.contains_key("_") {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableEnvMissingDefault,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            variable.location.clone(),
            "variable must contain [env._]",
        );
    }

    for block in environments.values() {
        lint_environment_block_shape(diagnostics, variable, block);
    }
}

fn lint_environment_block_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
) {
    let entity = EntityId::EnvironmentBlock {
        variable: variable.id.clone(),
        environment: block.environment.clone(),
    };
    if field_is_not_present(&block.value) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableEnvShape,
            entity,
            block.value.location(),
            "environment block must reference a value",
        );
    }

    match &block.rules {
        RuleCollection::Invalid { location } => push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            EntityId::EnvironmentBlock {
                variable: variable.id.clone(),
                environment: block.environment.clone(),
            },
            location.clone(),
            "rule must use [[env.<id>.rule]] tables or inline rule tables",
        ),
        RuleCollection::Rules(rules) => {
            for rule in rules {
                lint_variable_rule_shape(diagnostics, variable, block, rule);
            }
        }
    }
}

fn lint_variable_rule_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    rule: &VariableRuleNode,
) {
    let entity = EntityId::Rule {
        variable: variable.id.clone(),
        environment: block.environment.clone(),
        index: rule.index,
    };

    if rule.invalid_shape {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            entity,
            rule.location.clone(),
            "rule must be a table",
        );
        return;
    }

    if field_is_not_present(&rule.qualifier) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            entity.clone(),
            rule.qualifier.location(),
            "rule must reference a qualifier",
        );
    }
    if field_is_not_present(&rule.value) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::VariableRuleShape,
            entity,
            rule.value.location(),
            "rule must reference a value",
        );
    }
}

struct ContextSchemaError {
    location: DiagnosticLocation,
    message: String,
}

fn lint_context_schema_reference(ctx: &mut LintContext) {
    let Err(err) = valid_context_schema(ctx) else {
        return;
    };

    push_reference_diagnostic(
        &mut ctx.diagnostics,
        RototoRuleId::WorkspaceContextSchemaRef,
        EntityId::Manifest,
        err.location,
        err.message,
    );
}

fn lint_qualifier_context_schema_attributes(ctx: &mut LintContext) {
    let Ok(Some(schema)) = valid_context_schema(ctx) else {
        return;
    };

    let mut diagnostics = Vec::new();
    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if qualifier_reference(&attribute.value).is_some()
                || context_schema_declares_path(schema, &attribute.value)
            {
                continue;
            }

            push_reference_diagnostic(
                &mut diagnostics,
                RototoRuleId::WorkspaceContextSchemaAttribute,
                EntityId::Predicate {
                    qualifier: qualifier.id.clone(),
                    index: predicate.index,
                },
                attribute.location.clone(),
                format!(
                    "context attribute is not declared by the context schema: {}",
                    attribute.value
                ),
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn valid_context_schema(
    ctx: &LintContext,
) -> std::result::Result<Option<&JsonValue>, ContextSchemaError> {
    let Some(manifest) = &ctx.index.manifest else {
        return Ok(None);
    };
    let Some(context) = &manifest.context_schema else {
        return Ok(None);
    };

    if context.invalid_shape {
        return Err(ContextSchemaError {
            location: context.location.clone(),
            message: "[context] must be a table".to_owned(),
        });
    }

    let ProjectField::Present(schema_ref) = &context.schema else {
        return Err(ContextSchemaError {
            location: context.schema.location(),
            message: "[context] must declare schema".to_owned(),
        });
    };

    let schema_path =
        resolve_workspace_root_path(&schema_ref.value).ok_or_else(|| ContextSchemaError {
            location: schema_ref.location.clone(),
            message: "context schema path must be a relative path inside the workspace".to_owned(),
        })?;
    let schema_document =
        ctx.source
            .document_by_path(&schema_path)
            .ok_or_else(|| ContextSchemaError {
                location: schema_ref.location.clone(),
                message: format!("context schema file not found: {schema_path}"),
            })?;
    if !matches!(&schema_document.kind, DocumentKind::Schema) {
        return Err(ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema path is not a schema document: {schema_path}"),
        });
    }

    let schema = ctx
        .syntax
        .json
        .get(&schema_document.id)
        .ok_or_else(|| ContextSchemaError {
            location: schema_ref.location.clone(),
            message: format!("context schema file could not be parsed: {schema_path}"),
        })?;
    jsonschema::validator_for(schema).map_err(|err| ContextSchemaError {
        location: schema_ref.location.clone(),
        message: format!("context schema is invalid: {err}"),
    })?;

    Ok(Some(schema))
}

fn context_schema_declares_path(schema: &JsonValue, attribute: &str) -> bool {
    if attribute.is_empty() {
        return false;
    }

    let mut current = schema;
    for segment in attribute.split('.') {
        let Some(properties) = current.get("properties").and_then(JsonValue::as_object) else {
            return false;
        };
        let Some(next) = properties.get(segment) else {
            return false;
        };
        current = next;
    }
    true
}

fn lint_qualifier_references(ctx: &mut LintContext) {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let diagnostics = &mut ctx.diagnostics;

    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            let Some(referenced_qualifier) = qualifier_reference(&attribute.value) else {
                continue;
            };

            if known_qualifiers.contains(referenced_qualifier) {
                continue;
            }

            push_reference_diagnostic(
                diagnostics,
                RototoRuleId::QualifierPredicateUnknownQualifier,
                EntityId::Predicate {
                    qualifier: qualifier.id.clone(),
                    index: predicate.index,
                },
                attribute.location.clone(),
                format!(
                    "predicate references unknown qualifier: {}",
                    reference_label(referenced_qualifier)
                ),
            );
        }
    }
}

fn lint_variable_references(ctx: &mut LintContext) {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let declared_environments = declared_workspace_environments(ctx);
    let diagnostics = &mut ctx.diagnostics;

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };

        for block in environments.values() {
            lint_environment_reference(
                diagnostics,
                variable,
                block,
                declared_environments.as_ref(),
            );
            lint_environment_value_reference(diagnostics, variable, block);
            lint_rule_references(diagnostics, variable, block, &known_qualifiers);
        }
    }
}

fn lint_environment_reference(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    declared_environments: Option<&BTreeSet<String>>,
) {
    let Some(declared_environments) = declared_environments else {
        return;
    };

    if block.environment == "_" || declared_environments.contains(&block.environment) {
        return;
    }

    push_reference_diagnostic(
        diagnostics,
        RototoRuleId::VariableUnknownEnvironment,
        EntityId::EnvironmentBlock {
            variable: variable.id.clone(),
            environment: block.environment.clone(),
        },
        block.value.location(),
        format!(
            "variable references undeclared environment: {}",
            block.environment
        ),
    );
}

fn lint_environment_value_reference(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
) {
    let ProjectField::Present(value) = &block.value else {
        return;
    };

    if !variable_has_values(variable) || variable_has_value(variable, &value.value) {
        return;
    }

    push_reference_diagnostic(
        diagnostics,
        RototoRuleId::VariableUnknownValue,
        EntityId::EnvironmentBlock {
            variable: variable.id.clone(),
            environment: block.environment.clone(),
        },
        value.location.clone(),
        format!("environment references unknown value: {}", value.value),
    );
}

fn lint_rule_references(
    diagnostics: &mut Vec<LintDiagnostic>,
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    known_qualifiers: &BTreeSet<String>,
) {
    let RuleCollection::Rules(rules) = &block.rules else {
        return;
    };

    for rule in rules {
        if rule.invalid_shape {
            continue;
        }

        let entity = EntityId::Rule {
            variable: variable.id.clone(),
            environment: block.environment.clone(),
            index: rule.index,
        };

        if let ProjectField::Present(qualifier) = &rule.qualifier
            && !known_qualifiers.contains(&qualifier.value)
        {
            push_reference_diagnostic(
                diagnostics,
                RototoRuleId::VariableRuleUnknownQualifier,
                entity.clone(),
                qualifier.location.clone(),
                format!("rule references unknown qualifier: {}", qualifier.value),
            );
        }

        if let ProjectField::Present(value) = &rule.value
            && variable_has_values(variable)
            && !variable_has_value(variable, &value.value)
        {
            push_reference_diagnostic(
                diagnostics,
                RototoRuleId::VariableUnknownValue,
                entity,
                value.location.clone(),
                format!("rule references unknown value: {}", value.value),
            );
        }
    }
}

fn declared_workspace_environments(ctx: &LintContext) -> Option<BTreeSet<String>> {
    let manifest = ctx.index.manifest.as_ref()?;
    let parsed = ctx.syntax.toml.get(&manifest.doc)?;
    workspace_environments(&parsed.plain)
        .ok()
        .map(|environments| environments.into_iter().collect())
}

fn qualifier_reference(attribute: &str) -> Option<&str> {
    attribute.strip_prefix("qualifier.")
}

fn reference_label(reference: &str) -> &str {
    if reference.is_empty() {
        "<empty>"
    } else {
        reference
    }
}

fn variable_has_values(variable: &VariableNode) -> bool {
    !variable.values.inline_keys.is_empty() || !variable.values.external_keys.is_empty()
}

fn variable_has_value(variable: &VariableNode, value: &str) -> bool {
    variable.values.inline_keys.contains(value) || variable.values.external_keys.contains(value)
}

#[derive(Clone)]
struct QualifierReferenceEdge {
    from: String,
    to: String,
    location: DiagnosticLocation,
}

fn lint_qualifier_cycles(ctx: &mut LintContext) {
    let graph = qualifier_reference_graph(ctx);
    let components = strongly_connected_qualifiers(&graph);
    let mut diagnostics = Vec::new();

    for component in components {
        let component_set: BTreeSet<_> = component.iter().cloned().collect();
        let cycle_edges = component
            .iter()
            .flat_map(|qualifier_id| graph.get(qualifier_id).into_iter().flatten())
            .filter(|edge| component_set.contains(&edge.to))
            .cloned()
            .collect::<Vec<_>>();
        let is_cycle = component.len() > 1
            || cycle_edges
                .iter()
                .any(|edge| edge.from == edge.to && component_set.contains(&edge.from));
        if !is_cycle {
            continue;
        }

        for qualifier_id in &component {
            let Some(qualifier) = ctx.index.qualifiers.get(qualifier_id) else {
                continue;
            };
            let primary_edge = cycle_edges.iter().find(|edge| edge.from == *qualifier_id);
            let primary = primary_edge
                .map(|edge| edge.location.clone())
                .unwrap_or_else(|| qualifier.location.clone());
            let mut diagnostic = LintDiagnostic::rototo(
                RototoRuleId::QualifierCycle,
                LintStage::Graph,
                EntityId::Qualifier {
                    id: qualifier_id.clone(),
                },
                primary.clone(),
                qualifier_cycle_message(qualifier_id, &component),
            );
            diagnostic.related = cycle_edges
                .iter()
                .filter(|edge| edge.from != *qualifier_id || edge.location != primary)
                .map(|edge| RelatedLocation {
                    location: edge.location.clone(),
                    message: format!("cycle reference: {} -> {}", edge.from, edge.to),
                })
                .collect();
            diagnostics.push(diagnostic);
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_cycle_message(qualifier_id: &str, component: &[String]) -> String {
    if component.len() == 1 {
        format!("qualifier references itself: {qualifier_id}")
    } else {
        format!(
            "qualifier participates in a reference cycle: {}",
            component.join(" -> ")
        )
    }
}

fn lint_unreferenced_qualifiers(ctx: &mut LintContext) {
    let referenced = referenced_qualifier_ids(ctx);
    let mut diagnostics = Vec::new();

    for qualifier in ctx.index.qualifiers.values() {
        if referenced.contains(&qualifier.id) {
            continue;
        }

        push_graph_diagnostic(
            &mut diagnostics,
            RototoRuleId::QualifierUnreferenced,
            EntityId::Qualifier {
                id: qualifier.id.clone(),
            },
            qualifier.location.clone(),
            format!("qualifier is not referenced: {}", qualifier.id),
        );
    }

    ctx.diagnostics.extend(diagnostics);
}

fn lint_shadowed_variable_rules(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };

        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            let mut seen_qualifiers: BTreeMap<String, DiagnosticLocation> = BTreeMap::new();

            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                let ProjectField::Present(qualifier) = &rule.qualifier else {
                    continue;
                };

                if let Some(first_location) = seen_qualifiers.get(&qualifier.value) {
                    let mut diagnostic = LintDiagnostic::rototo(
                        RototoRuleId::VariableRuleShadowed,
                        LintStage::Graph,
                        EntityId::Rule {
                            variable: variable.id.clone(),
                            environment: block.environment.clone(),
                            index: rule.index,
                        },
                        qualifier.location.clone(),
                        format!(
                            "rule is shadowed by an earlier rule with qualifier: {}",
                            qualifier.value
                        ),
                    );
                    diagnostic.related.push(RelatedLocation {
                        location: first_location.clone(),
                        message: format!("first rule using qualifier: {}", qualifier.value),
                    });
                    diagnostics.push(diagnostic);
                } else {
                    seen_qualifiers.insert(qualifier.value.clone(), qualifier.location.clone());
                }
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn lint_unused_variable_values(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let referenced = referenced_variable_value_keys(variable);
        for value in variable_values(ctx, variable) {
            if referenced.contains(&value.key) {
                continue;
            }

            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableValueUnused,
                EntityId::Value {
                    variable: variable.id.clone(),
                    key: value.key.clone(),
                },
                value.location.clone(),
                format!("variable value is not referenced: {}", value.key),
            );
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_reference_graph(ctx: &LintContext) -> BTreeMap<String, Vec<QualifierReferenceEdge>> {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let mut graph = known_qualifiers
        .iter()
        .map(|qualifier_id| (qualifier_id.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();

    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            let Some(referenced_qualifier) = qualifier_reference(&attribute.value) else {
                continue;
            };
            if !known_qualifiers.contains(referenced_qualifier) {
                continue;
            }

            graph
                .entry(qualifier.id.clone())
                .or_default()
                .push(QualifierReferenceEdge {
                    from: qualifier.id.clone(),
                    to: referenced_qualifier.to_owned(),
                    location: attribute.location.clone(),
                });
        }
    }

    graph
}

#[derive(Default)]
struct TarjanState {
    next_index: usize,
    stack: Vec<String>,
    indices: BTreeMap<String, usize>,
    lowlinks: BTreeMap<String, usize>,
    on_stack: BTreeSet<String>,
    components: Vec<Vec<String>>,
}

fn strongly_connected_qualifiers(
    graph: &BTreeMap<String, Vec<QualifierReferenceEdge>>,
) -> Vec<Vec<String>> {
    let mut state = TarjanState::default();

    for qualifier_id in graph.keys() {
        if !state.indices.contains_key(qualifier_id) {
            strong_connect_qualifier(qualifier_id, graph, &mut state);
        }
    }

    state.components
}

fn strong_connect_qualifier(
    qualifier_id: &str,
    graph: &BTreeMap<String, Vec<QualifierReferenceEdge>>,
    state: &mut TarjanState,
) {
    state
        .indices
        .insert(qualifier_id.to_owned(), state.next_index);
    state
        .lowlinks
        .insert(qualifier_id.to_owned(), state.next_index);
    state.next_index += 1;
    state.stack.push(qualifier_id.to_owned());
    state.on_stack.insert(qualifier_id.to_owned());

    if let Some(edges) = graph.get(qualifier_id) {
        for edge in edges {
            if !state.indices.contains_key(&edge.to) {
                strong_connect_qualifier(&edge.to, graph, state);
                let target_lowlink = state.lowlinks[&edge.to];
                let lowlink = state.lowlinks.get_mut(qualifier_id).unwrap();
                *lowlink = (*lowlink).min(target_lowlink);
            } else if state.on_stack.contains(&edge.to) {
                let target_index = state.indices[&edge.to];
                let lowlink = state.lowlinks.get_mut(qualifier_id).unwrap();
                *lowlink = (*lowlink).min(target_index);
            }
        }
    }

    if state.lowlinks[qualifier_id] != state.indices[qualifier_id] {
        return;
    }

    let mut component = Vec::new();
    while let Some(member) = state.stack.pop() {
        state.on_stack.remove(&member);
        let is_root = member == qualifier_id;
        component.push(member);
        if is_root {
            break;
        }
    }
    component.sort();
    state.components.push(component);
}

fn referenced_qualifier_ids(ctx: &LintContext) -> BTreeSet<String> {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let mut referenced = BTreeSet::new();

    for edges in qualifier_reference_graph(ctx).values() {
        for edge in edges {
            if edge.from != edge.to {
                referenced.insert(edge.to.clone());
            }
        }
    }

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };
        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                let ProjectField::Present(qualifier) = &rule.qualifier else {
                    continue;
                };
                if known_qualifiers.contains(&qualifier.value) {
                    referenced.insert(qualifier.value.clone());
                }
            }
        }
    }

    referenced
}

fn referenced_variable_value_keys(variable: &VariableNode) -> BTreeSet<String> {
    let mut referenced = BTreeSet::new();
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return referenced;
    };

    for block in environments.values() {
        if let ProjectField::Present(value) = &block.value {
            referenced.insert(value.value.clone());
        }
        let RuleCollection::Rules(rules) = &block.rules else {
            continue;
        };
        for rule in rules {
            if rule.invalid_shape {
                continue;
            }
            if let ProjectField::Present(value) = &rule.value {
                referenced.insert(value.value.clone());
            }
        }
    }

    referenced
}

fn lint_schema_documents(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for document in ctx.source.documents.values() {
        if !matches!(&document.kind, DocumentKind::Schema) {
            continue;
        }
        let Some(schema) = ctx.syntax.json.get(&document.id) else {
            continue;
        };

        if let Err(err) = jsonschema::validator_for(schema) {
            push_value_diagnostic(
                &mut diagnostics,
                RototoRuleId::SchemaInvalid,
                EntityId::Schema {
                    path: document.path.clone(),
                },
                document.document_location(),
                format!("schema is invalid: {err}"),
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_variable_values(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for variable in ctx.index.variables.values() {
        match &variable.type_source {
            TypeSourceNode::Primitive(type_name) => {
                let Some(primitive) = PrimitiveType::from_str(&type_name.value) else {
                    continue;
                };
                lint_primitive_variable_values(&mut diagnostics, ctx, variable, primitive);
            }
            TypeSourceNode::Schema(schema_ref) => {
                lint_schema_backed_variable_values(&mut diagnostics, ctx, variable, schema_ref);
            }
            TypeSourceNode::Missing { .. }
            | TypeSourceNode::Conflict { .. }
            | TypeSourceNode::Invalid { .. } => {}
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_primitive_variable_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    variable: &VariableNode,
    primitive: PrimitiveType,
) {
    for value in variable_values(ctx, variable) {
        if primitive.matches(&value.value) {
            continue;
        }

        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableValueTypeMismatch,
            EntityId::Value {
                variable: variable.id.clone(),
                key: value.key.clone(),
            },
            value.location.clone(),
            format!(
                "value {} does not match type {}",
                value.key,
                primitive.as_str()
            ),
        );
    }
}

fn lint_schema_backed_variable_values(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    variable: &VariableNode,
    schema_ref: &Spanned<String>,
) {
    let Some(schema_path) =
        resolve_workspace_relative_path(&variable.location.path, &schema_ref.value)
    else {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableSchemaRef,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            schema_ref.location.clone(),
            format!(
                "variable schema reference is invalid: {} is not a relative path inside the workspace",
                schema_ref.value
            ),
        );
        return;
    };

    let Some(document) = ctx.source.document_by_path(&schema_path) else {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableSchemaRef,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            schema_ref.location.clone(),
            format!("variable schema reference is invalid: schema file not found: {schema_path}"),
        );
        return;
    };

    if !matches!(&document.kind, DocumentKind::Schema) {
        push_value_diagnostic(
            diagnostics,
            RototoRuleId::VariableSchemaRef,
            EntityId::Variable {
                id: variable.id.clone(),
            },
            schema_ref.location.clone(),
            format!(
                "variable schema reference is invalid: path is not a schema document: {schema_path}"
            ),
        );
        return;
    }

    let Some(schema) = ctx.syntax.json.get(&document.id) else {
        return;
    };

    let validator = match jsonschema::validator_for(schema) {
        Ok(validator) => validator,
        Err(_) => {
            return;
        }
    };

    for value in variable_values(ctx, variable) {
        if let Err(err) = validator.validate(&value.value) {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::VariableValueSchemaMismatch,
                EntityId::Value {
                    variable: variable.id.clone(),
                    key: value.key.clone(),
                },
                value.location.clone(),
                format!("value {} does not match schema: {err}", value.key),
            );
        }
    }
}

fn variable_values<'a>(
    ctx: &'a LintContext,
    variable: &'a VariableNode,
) -> impl Iterator<Item = &'a ValueNode> {
    variable.values.inline_values.values().chain(
        ctx.index
            .external_values
            .get(&variable.id)
            .into_iter()
            .flat_map(|values| values.values()),
    )
}

fn workspace_environments_symbol(
    environments: &WorkspaceEnvironmentCollection,
) -> Option<WorkspaceDocumentSymbol> {
    match environments {
        WorkspaceEnvironmentCollection::Missing => None,
        WorkspaceEnvironmentCollection::Invalid { location } => Some(WorkspaceDocumentSymbol::new(
            "environments",
            WorkspaceDocumentSymbolKind::WorkspaceEnvironments,
            location.clone(),
            Vec::new(),
        )),
        WorkspaceEnvironmentCollection::Environments { location, values } => {
            Some(WorkspaceDocumentSymbol::new(
                "environments",
                WorkspaceDocumentSymbolKind::WorkspaceEnvironments,
                location.clone(),
                values
                    .iter()
                    .map(|environment| {
                        WorkspaceDocumentSymbol::new(
                            environment.name.clone(),
                            WorkspaceDocumentSymbolKind::Environment,
                            environment.location.clone(),
                            Vec::new(),
                        )
                    })
                    .collect(),
            ))
        }
    }
}

fn qualifier_document_symbol(qualifier: &QualifierNode) -> WorkspaceDocumentSymbol {
    let children = match &qualifier.predicates {
        PredicateCollection::Predicates(predicates) => predicates
            .iter()
            .map(predicate_document_symbol)
            .collect::<Vec<_>>(),
        PredicateCollection::Missing { .. } | PredicateCollection::Invalid { .. } => Vec::new(),
    };
    WorkspaceDocumentSymbol::new(
        qualifier.id.clone(),
        WorkspaceDocumentSymbolKind::Qualifier,
        qualifier.location.clone(),
        children,
    )
}

fn predicate_document_symbol(predicate: &PredicateNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        predicate_symbol_name(predicate),
        WorkspaceDocumentSymbolKind::Predicate,
        predicate.location.clone(),
        Vec::new(),
    )
}

fn variable_document_symbol(variable: &VariableNode) -> WorkspaceDocumentSymbol {
    let mut children = Vec::new();
    if let Some(values) = variable_values_document_symbol(variable) {
        children.push(values);
    }
    children.extend(variable_environment_document_symbols(variable));

    WorkspaceDocumentSymbol::new(
        variable.id.clone(),
        WorkspaceDocumentSymbolKind::Variable,
        variable.location.clone(),
        children,
    )
}

fn variable_values_document_symbol(variable: &VariableNode) -> Option<WorkspaceDocumentSymbol> {
    if variable.values.inline_values.is_empty() && !variable.values.invalid_shape {
        return None;
    }

    Some(WorkspaceDocumentSymbol::new(
        "values",
        WorkspaceDocumentSymbolKind::Values,
        variable.values.location.clone(),
        variable
            .values
            .inline_values
            .values()
            .map(value_document_symbol)
            .collect(),
    ))
}

fn variable_environment_document_symbols(variable: &VariableNode) -> Vec<WorkspaceDocumentSymbol> {
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return Vec::new();
    };

    environments
        .values()
        .map(|block| {
            let children = match &block.rules {
                RuleCollection::Rules(rules) => rules
                    .iter()
                    .map(variable_rule_document_symbol)
                    .collect::<Vec<_>>(),
                RuleCollection::Invalid { .. } => Vec::new(),
            };
            WorkspaceDocumentSymbol::new(
                format!("env.{}", block.environment),
                WorkspaceDocumentSymbolKind::EnvironmentBlock,
                block.location.clone(),
                children,
            )
        })
        .collect()
}

fn variable_rule_document_symbol(rule: &VariableRuleNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        variable_rule_symbol_name(rule),
        WorkspaceDocumentSymbolKind::Rule,
        rule.location.clone(),
        Vec::new(),
    )
}

fn external_value_document_symbol(variable_id: &str, value: &ValueNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        format!("{}.{}", variable_id, value.key),
        WorkspaceDocumentSymbolKind::Value,
        value.location.clone(),
        Vec::new(),
    )
}

fn value_document_symbol(value: &ValueNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        value.key.clone(),
        WorkspaceDocumentSymbolKind::Value,
        value.location.clone(),
        Vec::new(),
    )
}

fn predicate_symbol_name(predicate: &PredicateNode) -> String {
    let index = predicate.index + 1;
    let Some(attribute) = string_project_field_value(&predicate.attribute) else {
        return format!("predicate {index}");
    };
    let Some(op) = predicate_op_project_field_value(&predicate.op) else {
        return format!("predicate {index}: {attribute}");
    };
    format!("predicate {index}: {attribute} {op}")
}

fn variable_rule_symbol_name(rule: &VariableRuleNode) -> String {
    let index = rule.index + 1;
    match (
        string_project_field_value(&rule.qualifier),
        string_project_field_value(&rule.value),
    ) {
        (Some(qualifier), Some(value)) => format!("rule {index}: {qualifier} -> {value}"),
        (Some(qualifier), None) => format!("rule {index}: {qualifier}"),
        (None, Some(value)) => format!("rule {index}: {value}"),
        (None, None) => format!("rule {index}"),
    }
}

fn string_project_field_value(field: &ProjectField<String>) -> Option<&str> {
    match field {
        ProjectField::Present(value) => Some(&value.value),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn predicate_op_project_field_value(field: &ProjectField<PredicateOp>) -> Option<&str> {
    match field {
        ProjectField::Present(value) => Some(value.value.as_str()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn sort_workspace_document_symbols(symbols: &mut [WorkspaceDocumentSymbol]) {
    for symbol in symbols.iter_mut() {
        sort_workspace_document_symbols(&mut symbol.children);
    }
    symbols.sort_by(|left, right| {
        symbol_position(left)
            .cmp(&symbol_position(right))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn symbol_position(symbol: &WorkspaceDocumentSymbol) -> (usize, usize) {
    symbol
        .location
        .range
        .map(|range| (range.start.line, range.start.character))
        .unwrap_or((0, 0))
}

fn workspace_environment_completion_items(
    environments: &WorkspaceEnvironmentCollection,
) -> Vec<WorkspaceCompletionItem> {
    let WorkspaceEnvironmentCollection::Environments { values, .. } = environments else {
        return Vec::new();
    };

    values
        .iter()
        .map(|environment| {
            WorkspaceCompletionItem::new(
                environment.name.clone(),
                WorkspaceCompletionItemKind::Environment,
                "workspace environment",
            )
        })
        .collect()
}

fn qualifier_completion_items(index: &SemanticIndex) -> Vec<WorkspaceCompletionItem> {
    index
        .qualifiers
        .keys()
        .map(|qualifier| {
            WorkspaceCompletionItem::new(
                qualifier.clone(),
                WorkspaceCompletionItemKind::Qualifier,
                "qualifier",
            )
        })
        .collect()
}

fn current_variable_value_completion_items(
    index: &SemanticIndex,
    path: &str,
) -> Vec<WorkspaceCompletionItem> {
    let Some(variable) = current_variable_for_path(index, path) else {
        return Vec::new();
    };

    variable
        .values
        .inline_keys
        .iter()
        .chain(variable.values.external_keys.iter())
        .map(|value| {
            WorkspaceCompletionItem::new(
                value.clone(),
                WorkspaceCompletionItemKind::Value,
                "variable value",
            )
        })
        .collect()
}

fn current_variable_for_path<'a>(index: &'a SemanticIndex, path: &str) -> Option<&'a VariableNode> {
    index
        .variables
        .values()
        .find(|variable| variable.location.path == path)
        .or_else(|| current_variable_for_external_value_path(index, path))
}

fn current_variable_for_external_value_path<'a>(
    index: &'a SemanticIndex,
    path: &str,
) -> Option<&'a VariableNode> {
    let variable_id = index
        .external_values
        .iter()
        .find_map(|(variable_id, values)| {
            values
                .values()
                .any(|value| value.location.path == path)
                .then_some(variable_id)
        })?;
    index.variables.get(variable_id)
}

fn predicate_operator_completion_items() -> Vec<WorkspaceCompletionItem> {
    PredicateOp::COMPLETION_LABELS
        .iter()
        .copied()
        .map(|op| {
            WorkspaceCompletionItem::new(
                op,
                WorkspaceCompletionItemKind::PredicateOperator,
                "predicate operator",
            )
        })
        .collect()
}

fn custom_lint_field_selector_completion_items() -> Vec<WorkspaceCompletionItem> {
    CUSTOM_LINT_FIELD_SELECTORS
        .iter()
        .copied()
        .map(|field| {
            WorkspaceCompletionItem::new(
                field,
                WorkspaceCompletionItemKind::FieldSelector,
                "custom lint field selector",
            )
        })
        .collect()
}

fn sort_and_deduplicate_workspace_completion_items(items: &mut Vec<WorkspaceCompletionItem>) {
    items.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then_with(|| {
                completion_item_kind_rank(left.kind).cmp(&completion_item_kind_rank(right.kind))
            })
            .then_with(|| left.detail.cmp(right.detail))
    });
    items.dedup_by(|left, right| {
        left.label == right.label && left.kind == right.kind && left.detail == right.detail
    });
}

fn completion_item_kind_rank(kind: WorkspaceCompletionItemKind) -> u8 {
    match kind {
        WorkspaceCompletionItemKind::Environment => 0,
        WorkspaceCompletionItemKind::Qualifier => 1,
        WorkspaceCompletionItemKind::Value => 2,
        WorkspaceCompletionItemKind::PredicateOperator => 3,
        WorkspaceCompletionItemKind::FieldSelector => 4,
    }
}

struct HoverCandidate {
    priority: u8,
    span_size: usize,
    hover: WorkspaceHover,
}

fn push_diagnostic_hover_candidates(
    snapshot: &WorkspaceLintSnapshot,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    for diagnostic in &snapshot.lint.diagnostics {
        let contents = diagnostic_hover_contents(&snapshot.index, diagnostic);
        push_hover_candidate(candidates, path, position, &diagnostic.primary, 0, contents);
    }
}

fn push_manifest_hover_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    let Some(manifest) = &index.manifest else {
        return;
    };
    let CustomRuleCollection::Rules(rules) = &manifest.custom_rules else {
        return;
    };

    for rule in rules {
        let Some(definition) = custom_rule_definition_from_declaration(rule) else {
            continue;
        };
        push_hover_candidate(
            candidates,
            path,
            position,
            &rule.location,
            1,
            custom_rule_hover_contents(&definition),
        );
        for location in [
            Some(rule.id.location()),
            Some(rule.title.location()),
            Some(rule.help.location()),
            rule.severity.as_ref().map(ProjectField::location),
        ]
        .into_iter()
        .flatten()
        {
            push_hover_candidate(
                candidates,
                path,
                position,
                &location,
                1,
                custom_rule_hover_contents(&definition),
            );
        }
    }
}

fn push_qualifier_hover_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    for qualifier in index.qualifiers.values() {
        if qualifier.location.path != path {
            continue;
        }

        if let Some(ProjectField::Present(description)) = &qualifier.description {
            push_hover_candidate(
                candidates,
                path,
                position,
                &description.location,
                2,
                qualifier_hover_contents(qualifier),
            );
        }

        if let PredicateCollection::Predicates(predicates) = &qualifier.predicates {
            for predicate in predicates {
                push_hover_candidate(
                    candidates,
                    path,
                    position,
                    &predicate.location,
                    3,
                    predicate_hover_contents(qualifier, predicate),
                );
                for location in [
                    Some(predicate.attribute.location()),
                    Some(predicate.op.location()),
                    predicate.value.as_ref().map(|value| value.location.clone()),
                    predicate.salt.as_ref().map(ProjectField::location),
                    predicate.range.as_ref().map(|range| range.location.clone()),
                ]
                .into_iter()
                .flatten()
                {
                    push_hover_candidate(
                        candidates,
                        path,
                        position,
                        &location,
                        2,
                        predicate_hover_contents(qualifier, predicate),
                    );
                }
            }
        }
    }
}

fn push_variable_hover_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<HoverCandidate>,
) {
    for variable in index.variables.values() {
        if variable.location.path != path {
            continue;
        }

        if let Some(ProjectField::Present(description)) = &variable.description {
            push_hover_candidate(
                candidates,
                path,
                position,
                &description.location,
                2,
                variable_hover_contents(variable),
            );
        }

        push_hover_candidate(
            candidates,
            path,
            position,
            &variable.type_source.location(),
            2,
            variable_type_hover_contents(variable),
        );

        push_hover_candidate(
            candidates,
            path,
            position,
            &variable.values.location,
            4,
            variable_values_hover_contents(variable),
        );
        for value in variable.values.inline_values.values() {
            push_hover_candidate(
                candidates,
                path,
                position,
                &value.location,
                2,
                value_hover_contents(&variable.id, value),
            );
        }

        if let EnvironmentCollection::Environments(environments) = &variable.environments {
            for block in environments.values() {
                push_hover_candidate(
                    candidates,
                    path,
                    position,
                    &block.value.location(),
                    3,
                    environment_block_hover_contents(variable, block),
                );
                push_hover_candidate(
                    candidates,
                    path,
                    position,
                    &block.location,
                    4,
                    environment_block_hover_contents(variable, block),
                );
                if let RuleCollection::Rules(rules) = &block.rules {
                    for rule in rules {
                        push_hover_candidate(
                            candidates,
                            path,
                            position,
                            &rule.location,
                            3,
                            variable_rule_hover_contents(variable, block, rule),
                        );
                        for location in [rule.qualifier.location(), rule.value.location()] {
                            push_hover_candidate(
                                candidates,
                                path,
                                position,
                                &location,
                                2,
                                variable_rule_hover_contents(variable, block, rule),
                            );
                        }
                    }
                }
            }
        }
    }

    for (variable_id, values) in &index.external_values {
        for value in values.values() {
            push_hover_candidate(
                candidates,
                path,
                position,
                &value.location,
                2,
                value_hover_contents(variable_id, value),
            );
        }
    }
}

fn push_hover_candidate(
    candidates: &mut Vec<HoverCandidate>,
    path: &str,
    position: SourcePosition,
    location: &DiagnosticLocation,
    priority: u8,
    contents: String,
) {
    if !location_contains_position(location, path, position) {
        return;
    }
    candidates.push(HoverCandidate {
        priority,
        span_size: location.range.map(source_range_size).unwrap_or(usize::MAX),
        hover: WorkspaceHover {
            contents,
            location: location.clone(),
        },
    });
}

fn sort_hover_candidates(candidates: &mut [HoverCandidate]) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.span_size.cmp(&right.span_size))
            .then_with(|| left.hover.contents.cmp(&right.hover.contents))
    });
}

fn location_contains_position(
    location: &DiagnosticLocation,
    path: &str,
    position: SourcePosition,
) -> bool {
    location.path == path
        && location
            .range
            .is_some_and(|range| source_range_contains_position(range, position))
}

fn source_range_contains_position(range: SourceRange, position: SourcePosition) -> bool {
    source_position_le(range.start, position) && source_position_lt(position, range.end)
}

fn source_position_le(left: SourcePosition, right: SourcePosition) -> bool {
    (left.line, left.character) <= (right.line, right.character)
}

fn source_position_lt(left: SourcePosition, right: SourcePosition) -> bool {
    (left.line, left.character) < (right.line, right.character)
}

fn source_range_size(range: SourceRange) -> usize {
    range
        .end
        .line
        .saturating_sub(range.start.line)
        .saturating_mul(10_000)
        .saturating_add(range.end.character.saturating_sub(range.start.character))
}

fn file_hover(index: &SemanticIndex, path: &str) -> Option<WorkspaceHover> {
    index
        .variables
        .values()
        .find(|variable| variable.location.path == path)
        .map(|variable| WorkspaceHover {
            contents: variable_hover_contents(variable),
            location: variable.location.clone(),
        })
        .or_else(|| {
            index
                .qualifiers
                .values()
                .find(|qualifier| qualifier.location.path == path)
                .map(|qualifier| WorkspaceHover {
                    contents: qualifier_hover_contents(qualifier),
                    location: qualifier.location.clone(),
                })
        })
        .or_else(|| {
            index
                .external_values
                .iter()
                .find_map(|(variable_id, values)| {
                    values
                        .values()
                        .find(|value| value.location.path == path)
                        .map(|value| WorkspaceHover {
                            contents: value_hover_contents(variable_id, value),
                            location: value.location.clone(),
                        })
                })
        })
}

fn diagnostic_hover_contents(index: &SemanticIndex, diagnostic: &LintDiagnostic) -> String {
    let (title, help) = diagnostic_rule_title_help(index, &diagnostic.rule);
    format!(
        "### {title}\n\n`{}`\n\n{}\n\n{}",
        diagnostic.rule.as_string(),
        diagnostic.message,
        help
    )
}

fn diagnostic_rule_title_help(index: &SemanticIndex, rule: &DiagnosticRule) -> (String, String) {
    match rule {
        DiagnosticRule::Rototo(rule) => {
            let meta = rule.meta();
            (meta.title.to_owned(), meta.help.to_owned())
        }
        DiagnosticRule::Custom(rule) => custom_rule_definition(index, rule)
            .map(|definition| (definition.title, definition.help))
            .unwrap_or_else(|| {
                (
                    rule.as_str().to_owned(),
                    "Workspace custom lint.".to_owned(),
                )
            }),
    }
}

fn custom_rule_definition(
    index: &SemanticIndex,
    rule: &CustomRuleId,
) -> Option<CustomRuleDefinition> {
    let manifest = index.manifest.as_ref()?;
    custom_rule_definitions_from_collection(&manifest.custom_rules)
        .into_iter()
        .map(|(definition, _)| definition)
        .find(|definition| &definition.rule == rule)
}

fn custom_rule_definition_from_declaration(
    rule: &CustomRuleDeclarationNode,
) -> Option<CustomRuleDefinition> {
    let (ProjectField::Present(id), ProjectField::Present(title), ProjectField::Present(help)) =
        (&rule.id, &rule.title, &rule.help)
    else {
        return None;
    };
    let rule_id = CustomRuleId::parse(&id.value).ok()?;
    let severity = match &rule.severity {
        Some(ProjectField::Present(severity)) => severity.value,
        Some(ProjectField::Invalid { .. }) => return None,
        Some(ProjectField::Missing { .. }) | None => Severity::Error,
    };
    Some(CustomRuleDefinition::with_severity(
        rule_id,
        severity,
        title.value.clone(),
        help.value.clone(),
    ))
}

fn custom_rule_hover_contents(definition: &CustomRuleDefinition) -> String {
    format!(
        "### Custom rule `{}`\n\n{}\n\n{}",
        definition.rule, definition.title, definition.help
    )
}

fn qualifier_hover_contents(qualifier: &QualifierNode) -> String {
    let mut contents = format!("### Qualifier `{}`", qualifier.id);
    if let Some(description) = project_field_string(&qualifier.description) {
        contents.push_str("\n\n");
        contents.push_str(description);
    }
    contents
}

fn predicate_hover_contents(qualifier: &QualifierNode, predicate: &PredicateNode) -> String {
    let mut contents = format!(
        "### Predicate {} for `{}`\n\n{}",
        predicate.index + 1,
        qualifier.id,
        predicate_summary(predicate)
    );
    if let Some(value) = &predicate.value {
        contents.push_str("\n\nValue shape: `");
        contents.push_str(value.shape.as_str());
        contents.push('`');
    }
    contents
}

fn predicate_summary(predicate: &PredicateNode) -> String {
    match (
        string_project_field_value(&predicate.attribute),
        predicate_op_project_field_value(&predicate.op),
    ) {
        (Some(attribute), Some(op)) => format!("`{attribute}` `{op}`"),
        (Some(attribute), None) => format!("`{attribute}`"),
        (None, Some(op)) => format!("operator `{op}`"),
        (None, None) => "Incomplete predicate".to_owned(),
    }
}

fn variable_hover_contents(variable: &VariableNode) -> String {
    let mut contents = format!(
        "### Variable `{}`\n\n{}",
        variable.id,
        type_source_summary(variable)
    );
    if let Some(description) = project_field_string(&variable.description) {
        contents.push_str("\n\n");
        contents.push_str(description);
    }
    let values = variable_value_keys(variable);
    if !values.is_empty() {
        contents.push_str("\n\nValues: ");
        contents.push_str(&values.join(", "));
    }
    contents
}

fn variable_type_hover_contents(variable: &VariableNode) -> String {
    format!(
        "### Variable `{}`\n\n{}",
        variable.id,
        type_source_summary(variable)
    )
}

fn variable_values_hover_contents(variable: &VariableNode) -> String {
    let values = variable_value_keys(variable);
    if values.is_empty() {
        return format!("### Values for `{}`\n\nNo values declared.", variable.id);
    }
    format!("### Values for `{}`\n\n{}", variable.id, values.join(", "))
}

fn value_hover_contents(variable_id: &str, value: &ValueNode) -> String {
    format!(
        "### Value `{}`\n\nVariable: `{}`\n\nJSON shape: `{}`",
        value.key,
        variable_id,
        json_shape_label(&value.value)
    )
}

fn environment_block_hover_contents(
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
) -> String {
    match string_project_field_value(&block.value) {
        Some(value) => format!(
            "### Environment `{}`\n\nVariable: `{}`\n\nDefault value: `{}`",
            block.environment, variable.id, value
        ),
        None => format!(
            "### Environment `{}`\n\nVariable: `{}`",
            block.environment, variable.id
        ),
    }
}

fn variable_rule_hover_contents(
    variable: &VariableNode,
    block: &EnvironmentBlockNode,
    rule: &VariableRuleNode,
) -> String {
    format!(
        "### Rule {} for `{}` in `{}`\n\n{}",
        rule.index + 1,
        variable.id,
        block.environment,
        variable_rule_summary(rule)
    )
}

fn variable_rule_summary(rule: &VariableRuleNode) -> String {
    match (
        string_project_field_value(&rule.qualifier),
        string_project_field_value(&rule.value),
    ) {
        (Some(qualifier), Some(value)) => {
            format!("Qualifier `{qualifier}` selects value `{value}`.")
        }
        (Some(qualifier), None) => format!("Qualifier `{qualifier}`."),
        (None, Some(value)) => format!("Selects value `{value}`."),
        (None, None) => "Incomplete rule.".to_owned(),
    }
}

fn type_source_summary(variable: &VariableNode) -> String {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => format!("Type: `{}`", type_name.value),
        TypeSourceNode::Schema(schema) => format!("Schema: `{}`", schema.value),
        TypeSourceNode::Missing { .. } => "Type/schema: missing".to_owned(),
        TypeSourceNode::Conflict { .. } => "Type/schema: both declared".to_owned(),
        TypeSourceNode::Invalid { .. } => "Type/schema: invalid".to_owned(),
    }
}

fn variable_value_keys(variable: &VariableNode) -> Vec<String> {
    variable
        .values
        .inline_keys
        .iter()
        .chain(variable.values.external_keys.iter())
        .map(|value| format!("`{value}`"))
        .collect()
}

fn project_field_string(field: &Option<ProjectField<String>>) -> Option<&str> {
    let Some(ProjectField::Present(value)) = field else {
        return None;
    };
    Some(&value.value)
}

fn json_shape_label(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(number) if number.is_i64() || number.is_u64() => "int",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "list",
        JsonValue::Object(_) => "object",
    }
}

struct DefinitionCandidate {
    priority: u8,
    span_size: usize,
    location: DiagnosticLocation,
}

fn push_manifest_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    let Some(manifest) = &index.manifest else {
        return;
    };
    let Some(context) = &manifest.context_schema else {
        return;
    };
    let ProjectField::Present(schema) = &context.schema else {
        return;
    };
    if !location_contains_position(&schema.location, path, position) {
        return;
    }
    let Some(schema_path) = resolve_workspace_root_path(&schema.value) else {
        return;
    };
    candidates.push(DefinitionCandidate {
        priority: 2,
        span_size: schema
            .location
            .range
            .map(source_range_size)
            .unwrap_or(usize::MAX),
        location: DiagnosticLocation::document(DocId(0), schema_path),
    });
}

fn push_qualifier_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    for qualifier in index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if !location_contains_position(&attribute.location, path, position) {
                continue;
            }
            let Some(target_id) = qualifier_reference(&attribute.value) else {
                continue;
            };
            let Some(target) = index.qualifiers.get(target_id) else {
                continue;
            };
            candidates.push(DefinitionCandidate {
                priority: 0,
                span_size: attribute
                    .location
                    .range
                    .map(source_range_size)
                    .unwrap_or(usize::MAX),
                location: target.location.clone(),
            });
        }
    }
}

fn push_variable_definition_candidates(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<DefinitionCandidate>,
) {
    for variable in index.variables.values() {
        if let TypeSourceNode::Schema(schema) = &variable.type_source
            && location_contains_position(&schema.location, path, position)
            && let Some(schema_path) =
                resolve_workspace_relative_path(&variable.location.path, &schema.value)
        {
            candidates.push(DefinitionCandidate {
                priority: 1,
                span_size: schema
                    .location
                    .range
                    .map(source_range_size)
                    .unwrap_or(usize::MAX),
                location: DiagnosticLocation::document(DocId(0), schema_path),
            });
        }

        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };

        for block in environments.values() {
            if let ProjectField::Present(value) = &block.value
                && location_contains_position(&value.location, path, position)
                && let Some(target) =
                    variable_value_definition_location(index, variable, &value.value)
            {
                candidates.push(DefinitionCandidate {
                    priority: 0,
                    span_size: value
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    location: target,
                });
            }

            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };

            for rule in rules {
                if let ProjectField::Present(qualifier) = &rule.qualifier
                    && location_contains_position(&qualifier.location, path, position)
                    && let Some(target) = index.qualifiers.get(&qualifier.value)
                {
                    candidates.push(DefinitionCandidate {
                        priority: 0,
                        span_size: qualifier
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        location: target.location.clone(),
                    });
                }

                if let ProjectField::Present(value) = &rule.value
                    && location_contains_position(&value.location, path, position)
                    && let Some(target) =
                        variable_value_definition_location(index, variable, &value.value)
                {
                    candidates.push(DefinitionCandidate {
                        priority: 0,
                        span_size: value
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        location: target,
                    });
                }
            }
        }
    }
}

fn variable_value_definition_location(
    index: &SemanticIndex,
    variable: &VariableNode,
    value: &str,
) -> Option<DiagnosticLocation> {
    variable
        .values
        .inline_values
        .get(value)
        .or_else(|| {
            index
                .external_values
                .get(&variable.id)
                .and_then(|values| values.get(value))
        })
        .map(|value| value.location.clone())
}

fn sort_definition_candidates(candidates: &mut [DefinitionCandidate]) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.span_size.cmp(&right.span_size))
            .then_with(|| left.location.path.cmp(&right.location.path))
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReferenceTarget {
    Qualifier(String),
    VariableValue { variable: String, value: String },
    Schema(String),
    ContextAttribute(String),
}

struct ReferenceTargetCandidate {
    priority: u8,
    span_size: usize,
    target: ReferenceTarget,
}

fn reference_target_at_position(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
) -> Option<ReferenceTarget> {
    let mut candidates = Vec::new();
    push_reference_targets_from_manifest(index, path, position, &mut candidates);
    push_reference_targets_from_qualifiers(index, path, position, &mut candidates);
    push_reference_targets_from_variables(index, path, position, &mut candidates);
    push_reference_targets_from_schema_documents(index, path, &mut candidates);
    sort_reference_target_candidates(&mut candidates);
    candidates
        .into_iter()
        .next()
        .map(|candidate| candidate.target)
}

fn push_reference_targets_from_manifest(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    let Some(manifest) = &index.manifest else {
        return;
    };
    let Some(context) = &manifest.context_schema else {
        return;
    };
    let ProjectField::Present(schema) = &context.schema else {
        return;
    };
    if location_contains_position(&schema.location, path, position)
        && let Some(schema_path) = resolve_workspace_root_path(&schema.value)
    {
        candidates.push(ReferenceTargetCandidate {
            priority: 0,
            span_size: schema
                .location
                .range
                .map(source_range_size)
                .unwrap_or(usize::MAX),
            target: ReferenceTarget::Schema(schema_path),
        });
    }
}

fn push_reference_targets_from_qualifiers(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    for qualifier in index.qualifiers.values() {
        if qualifier.location.path == path {
            candidates.push(ReferenceTargetCandidate {
                priority: 5,
                span_size: usize::MAX,
                target: ReferenceTarget::Qualifier(qualifier.id.clone()),
            });
        }

        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };
        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if !location_contains_position(&attribute.location, path, position) {
                continue;
            }
            match qualifier_reference(&attribute.value) {
                Some(qualifier_id) => candidates.push(ReferenceTargetCandidate {
                    priority: 0,
                    span_size: attribute
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::Qualifier(qualifier_id.to_owned()),
                }),
                None => candidates.push(ReferenceTargetCandidate {
                    priority: 0,
                    span_size: attribute
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::ContextAttribute(attribute.value.clone()),
                }),
            }
        }
    }
}

fn push_reference_targets_from_variables(
    index: &SemanticIndex,
    path: &str,
    position: SourcePosition,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    for variable in index.variables.values() {
        if let TypeSourceNode::Schema(schema) = &variable.type_source
            && location_contains_position(&schema.location, path, position)
            && let Some(schema_path) =
                resolve_workspace_relative_path(&variable.location.path, &schema.value)
        {
            candidates.push(ReferenceTargetCandidate {
                priority: 0,
                span_size: schema
                    .location
                    .range
                    .map(source_range_size)
                    .unwrap_or(usize::MAX),
                target: ReferenceTarget::Schema(schema_path),
            });
        }

        for value in variable.values.inline_values.values() {
            if location_contains_position(&value.location, path, position) {
                candidates.push(ReferenceTargetCandidate {
                    priority: 1,
                    span_size: value
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::VariableValue {
                        variable: variable.id.clone(),
                        value: value.key.clone(),
                    },
                });
            }
        }

        if let Some(values) = index.external_values.get(&variable.id) {
            for value in values.values() {
                if location_contains_position(&value.location, path, position) {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 1,
                        span_size: value
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        target: ReferenceTarget::VariableValue {
                            variable: variable.id.clone(),
                            value: value.key.clone(),
                        },
                    });
                }
            }
        }

        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };
        for block in environments.values() {
            if let ProjectField::Present(value) = &block.value
                && location_contains_position(&value.location, path, position)
            {
                candidates.push(ReferenceTargetCandidate {
                    priority: 0,
                    span_size: value
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: ReferenceTarget::VariableValue {
                        variable: variable.id.clone(),
                        value: value.value.clone(),
                    },
                });
            }

            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            for rule in rules {
                if let ProjectField::Present(qualifier) = &rule.qualifier
                    && location_contains_position(&qualifier.location, path, position)
                {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 0,
                        span_size: qualifier
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        target: ReferenceTarget::Qualifier(qualifier.value.clone()),
                    });
                }

                if let ProjectField::Present(value) = &rule.value
                    && location_contains_position(&value.location, path, position)
                {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 0,
                        span_size: value
                            .location
                            .range
                            .map(source_range_size)
                            .unwrap_or(usize::MAX),
                        target: ReferenceTarget::VariableValue {
                            variable: variable.id.clone(),
                            value: value.value.clone(),
                        },
                    });
                }
            }
        }
    }
}

fn push_reference_targets_from_schema_documents(
    index: &SemanticIndex,
    path: &str,
    candidates: &mut Vec<ReferenceTargetCandidate>,
) {
    if schema_path_is_referenced(index, path) {
        candidates.push(ReferenceTargetCandidate {
            priority: 5,
            span_size: usize::MAX,
            target: ReferenceTarget::Schema(path.to_owned()),
        });
    }
}

fn schema_path_is_referenced(index: &SemanticIndex, path: &str) -> bool {
    context_schema_reference_path(index).as_deref() == Some(path)
        || index.variables.values().any(|variable| {
            matches!(
                &variable.type_source,
                TypeSourceNode::Schema(schema)
                    if resolve_workspace_relative_path(&variable.location.path, &schema.value)
                        .as_deref()
                        == Some(path)
            )
        })
}

fn context_schema_reference_path(index: &SemanticIndex) -> Option<String> {
    let manifest = index.manifest.as_ref()?;
    let context = manifest.context_schema.as_ref()?;
    let ProjectField::Present(schema) = &context.schema else {
        return None;
    };
    resolve_workspace_root_path(&schema.value)
}

fn sort_reference_target_candidates(candidates: &mut [ReferenceTargetCandidate]) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.span_size.cmp(&right.span_size))
    });
}

fn reference_locations_for_target(
    index: &SemanticIndex,
    target: &ReferenceTarget,
) -> Vec<DiagnosticLocation> {
    match target {
        ReferenceTarget::Qualifier(qualifier) => qualifier_reference_locations(index, qualifier),
        ReferenceTarget::VariableValue { variable, value } => {
            variable_value_reference_locations(index, variable, value)
        }
        ReferenceTarget::Schema(schema_path) => schema_reference_locations(index, schema_path),
        ReferenceTarget::ContextAttribute(attribute) => {
            context_attribute_reference_locations(index, attribute)
        }
    }
}

fn reference_target_declaration(
    index: &SemanticIndex,
    target: &ReferenceTarget,
) -> Option<DiagnosticLocation> {
    match target {
        ReferenceTarget::Qualifier(qualifier) => index
            .qualifiers
            .get(qualifier)
            .map(|qualifier| qualifier.location.clone()),
        ReferenceTarget::VariableValue { variable, value } => index
            .variables
            .get(variable)
            .and_then(|variable| variable_value_definition_location(index, variable, value)),
        ReferenceTarget::Schema(schema_path) => {
            Some(DiagnosticLocation::document(DocId(0), schema_path.clone()))
        }
        ReferenceTarget::ContextAttribute(_) => None,
    }
}

fn qualifier_reference_locations(
    index: &SemanticIndex,
    qualifier_id: &str,
) -> Vec<DiagnosticLocation> {
    let mut locations = Vec::new();
    for qualifier in index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };
        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            if qualifier_reference(&attribute.value) == Some(qualifier_id) {
                locations.push(attribute.location.clone());
            }
        }
    }

    for variable in index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };
        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            for rule in rules {
                if let ProjectField::Present(qualifier) = &rule.qualifier
                    && qualifier.value == qualifier_id
                {
                    locations.push(qualifier.location.clone());
                }
            }
        }
    }
    locations
}

fn variable_value_reference_locations(
    index: &SemanticIndex,
    variable_id: &str,
    value_key: &str,
) -> Vec<DiagnosticLocation> {
    let Some(variable) = index.variables.get(variable_id) else {
        return Vec::new();
    };
    let mut locations = Vec::new();
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return locations;
    };
    for block in environments.values() {
        if let ProjectField::Present(value) = &block.value
            && value.value == value_key
        {
            locations.push(value.location.clone());
        }

        let RuleCollection::Rules(rules) = &block.rules else {
            continue;
        };
        for rule in rules {
            if let ProjectField::Present(value) = &rule.value
                && value.value == value_key
            {
                locations.push(value.location.clone());
            }
        }
    }
    locations
}

fn schema_reference_locations(index: &SemanticIndex, schema_path: &str) -> Vec<DiagnosticLocation> {
    let mut locations = Vec::new();
    if context_schema_reference_path(index).as_deref() == Some(schema_path)
        && let Some(manifest) = &index.manifest
        && let Some(context) = &manifest.context_schema
        && let ProjectField::Present(schema) = &context.schema
    {
        locations.push(schema.location.clone());
    }

    for variable in index.variables.values() {
        if let TypeSourceNode::Schema(schema) = &variable.type_source
            && resolve_workspace_relative_path(&variable.location.path, &schema.value).as_deref()
                == Some(schema_path)
        {
            locations.push(schema.location.clone());
        }
    }
    locations
}

fn context_attribute_reference_locations(
    index: &SemanticIndex,
    attribute: &str,
) -> Vec<DiagnosticLocation> {
    let mut locations = Vec::new();
    for qualifier in index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };
        for predicate in predicates {
            let ProjectField::Present(predicate_attribute) = &predicate.attribute else {
                continue;
            };
            if qualifier_reference(&predicate_attribute.value).is_none()
                && predicate_attribute.value == attribute
            {
                locations.push(predicate_attribute.location.clone());
            }
        }
    }
    locations
}

fn sort_and_deduplicate_workspace_references(references: &mut Vec<WorkspaceReference>) {
    references.sort_by(|left, right| {
        left.uri.cmp(&right.uri).then_with(|| {
            source_location_sort_key(&left.location).cmp(&source_location_sort_key(&right.location))
        })
    });
    references.dedup_by(|left, right| {
        left.uri == right.uri
            && source_location_sort_key(&left.location) == source_location_sort_key(&right.location)
    });
}

fn source_location_sort_key(location: &DiagnosticLocation) -> (usize, usize, usize, usize) {
    location
        .range
        .map(|range| {
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
            )
        })
        .unwrap_or((0, 0, 0, 0))
}

fn resolve_workspace_relative_path(document_path: &str, reference: &str) -> Option<String> {
    let reference = Path::new(reference);
    if reference.as_os_str().is_empty() || reference.is_absolute() {
        return None;
    }

    let base = Path::new(document_path).parent().unwrap_or(Path::new(""));
    let mut normalized = PathBuf::new();
    for component in base.join(reference).components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(workspace_path(&normalized))
    }
}

fn resolve_workspace_root_path(reference: &str) -> Option<String> {
    let reference = Path::new(reference);
    if reference.as_os_str().is_empty() || reference.is_absolute() {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in reference.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(workspace_path(&normalized))
    }
}

#[derive(Clone, Copy)]
enum PrimitiveType {
    Bool,
    Int,
    Number,
    String,
    List,
}

impl PrimitiveType {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "number" => Some(Self::Number),
            "string" => Some(Self::String),
            "list" => Some(Self::List),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Number => "number",
            Self::String => "string",
            Self::List => "list",
        }
    }

    fn matches(self, value: &JsonValue) -> bool {
        match self {
            Self::Bool => value.is_boolean(),
            Self::Int => value.as_i64().is_some() || value.as_u64().is_some(),
            Self::Number => value.is_number(),
            Self::String => value.is_string(),
            Self::List => value.is_array(),
        }
    }
}

fn field_is_not_present<T>(field: &ProjectField<T>) -> bool {
    !matches!(field, ProjectField::Present(_))
}

fn field_is_integer(field: &ProjectField<i64>, expected: i64) -> bool {
    matches!(field, ProjectField::Present(value) if value.value == expected)
}

fn predicate_op_label(op: &PredicateOp) -> &'static str {
    match op {
        PredicateOp::Eq => "eq",
        PredicateOp::Neq => "neq",
        PredicateOp::In => "in",
        PredicateOp::NotIn => "not_in",
        PredicateOp::Gt => "gt",
        PredicateOp::Gte => "gte",
        PredicateOp::Lt => "lt",
        PredicateOp::Lte => "lte",
        PredicateOp::Bucket => "bucket",
        PredicateOp::Unknown(_) => "unknown",
    }
}

fn push_project_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Project,
        entity,
        primary,
        message,
    ));
}

fn push_reference_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Reference,
        entity,
        primary,
        message,
    ));
}

fn push_graph_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Graph,
        entity,
        primary,
        message,
    ));
}

fn push_register_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Register,
        entity,
        primary,
        message,
    ));
}

fn push_stage_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    stage: LintStage,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule, stage, entity, primary, message,
    ));
}

fn push_value_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: RototoRuleId,
    entity: EntityId,
    primary: DiagnosticLocation,
    message: impl Into<String>,
) {
    diagnostics.push(LintDiagnostic::rototo(
        rule,
        LintStage::Value,
        entity,
        primary,
        message,
    ));
}

fn sort_diagnostics(diagnostics: &mut [LintDiagnostic]) {
    diagnostics.sort_by(|left, right| diagnostic_sort_key(left).cmp(&diagnostic_sort_key(right)));
}

fn diagnostic_sort_key(diagnostic: &LintDiagnostic) -> (u8, &str, usize, usize, String, &str) {
    let location_rank = match diagnostic.primary.kind {
        crate::diagnostics::DiagnosticLocationKind::WorkspaceRoot => 0,
        crate::diagnostics::DiagnosticLocationKind::Document
        | crate::diagnostics::DiagnosticLocationKind::Span => 1,
    };
    let (line, character) = diagnostic
        .primary
        .range
        .map(|range| (range.start.line, range.start.character))
        .unwrap_or((0, 0));
    (
        location_rank,
        diagnostic.primary.path.as_str(),
        line,
        character,
        diagnostic.rule.as_string(),
        diagnostic.message.as_str(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn snapshot_lints_overlay_without_writing_to_disk_and_groups_empty_documents() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join(WORKSPACE_MANIFEST),
            r#"schema_version = 1

[environments]
values = ["prod"]
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[env._]
value = "control"
"#;
        tokio::fs::write(root.join("variables/message.toml"), disk_variable)
            .await
            .unwrap();

        let invalid_overlay = r#"schema_version = 1
type = "mystery"

[values]
control = "hello"

[env._]
value = "control"
"#;
        let mut input = LintInput::new(root.to_path_buf());
        input.overlays.insert(
            "variables/message.toml".to_owned(),
            OverlayDocument {
                text: invalid_overlay.to_owned(),
                version: Some(42),
            },
        );
        let snapshot = lint_workspace_snapshot(input).await.unwrap();
        let lint = &snapshot.lint;

        let diagnostic = lint
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule.as_string() == "rototo/variable-unknown-type")
            .unwrap();
        assert_eq!(diagnostic.primary.path, "variables/message.toml");
        assert_eq!(diagnostic.primary.range.unwrap().start.line, 1);

        let variable_document = lint
            .documents
            .iter()
            .find(|document| document.path == "variables/message.toml")
            .unwrap();
        assert_eq!(variable_document.version, Some(42));

        let grouped = lint.diagnostics_by_document();
        assert!(grouped.iter().any(|group| {
            group.document.path == "rototo-workspace.toml" && group.diagnostics.is_empty()
        }));
        assert!(grouped.iter().any(|group| {
            group.document.path == "variables/message.toml" && !group.diagnostics.is_empty()
        }));
        let disk_after_overlay = tokio::fs::read_to_string(root.join("variables/message.toml"))
            .await
            .unwrap();
        assert_eq!(disk_after_overlay, disk_variable);

        let symbols = snapshot.document_symbols("variables/message.toml");
        assert_eq!(symbols[0].name, "message");
        assert!(symbols[0].children.iter().any(|symbol| {
            symbol.name == "values" && symbol.children.iter().any(|child| child.name == "control")
        }));

        let mut cleared_input = LintInput::new(root.to_path_buf());
        cleared_input.overlays.insert(
            "variables/message.toml".to_owned(),
            OverlayDocument {
                text: disk_variable.to_owned(),
                version: Some(43),
            },
        );
        let cleared = lint_workspace_snapshot(cleared_input).await.unwrap();

        assert!(cleared.lint.diagnostics.is_empty());
        let cleared_groups = cleared.lint.diagnostics_by_document();
        let variable_group = cleared_groups
            .iter()
            .find(|group| group.document.path == "variables/message.toml")
            .unwrap();
        assert_eq!(variable_group.document.version, Some(43));
        assert!(variable_group.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn snapshot_diagnostic_ranges_cover_references_and_external_values() {
        let reference_snapshot = lint_workspace_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/workspaces/rules/reference/variable-rule-unknown-qualifier",
        )))
        .await
        .unwrap();
        let reference = diagnostic_by_rule(
            &reference_snapshot.lint,
            "rototo/variable-rule-unknown-qualifier",
        );
        assert_eq!(reference.primary.path, "variables/checkout-redesign.toml");
        assert_eq!(reference.primary.range.unwrap().start.line, 14);
        assert_eq!(reference.primary.range.unwrap().start.character, 12);
        assert_eq!(reference.primary.range.unwrap().end.line, 14);
        assert_eq!(reference.primary.range.unwrap().end.character, 27);

        let external_value_snapshot = lint_workspace_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/workspaces/rules/project/variable-external-value-duplicate",
        )))
        .await
        .unwrap();
        let external_value = diagnostic_by_rule(
            &external_value_snapshot.lint,
            "rototo/variable-external-value-duplicate",
        );
        assert_eq!(
            external_value.primary.path,
            "variables/external-message-values/default.toml"
        );
        assert_eq!(external_value.primary.range.unwrap().start.line, 0);
        assert_eq!(external_value.primary.range.unwrap().start.character, 8);
        assert_eq!(external_value.primary.range.unwrap().end.line, 0);
        assert_eq!(external_value.primary.range.unwrap().end.character, 18);
    }

    fn diagnostic_by_rule<'a>(lint: &'a WorkspaceLint, rule: &str) -> &'a LintDiagnostic {
        lint.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule.as_string() == rule)
            .unwrap_or_else(|| panic!("diagnostic not found: {rule}"))
    }
}
