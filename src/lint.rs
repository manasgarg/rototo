use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml_edit::{ImDocument, Item, Table, TableLike, Value as EditValue};

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, DiagnosticRule, DocId, EntityId,
    LintDiagnostic, LintStage, RelatedLocation, RototoRuleId, SourcePosition, SourceRange,
};
use crate::error::{Result, RototoError};
use crate::lua_lint;
use crate::model::{QualifierLint, SourceDocumentSummary, SourceKind, VariableLint, WorkspaceLint};
use crate::workspace::workspace_environments;

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

#[derive(Clone)]
pub(crate) struct LintInput {
    root: PathBuf,
    pub(crate) overlays: BTreeMap<String, OverlayDocument>,
}

impl LintInput {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            overlays: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct OverlayDocument {
    pub(crate) text: String,
    pub(crate) version: Option<i32>,
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
                ctx.diagnostics
                    .push(read_error_diagnostic(document, read_error));
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

#[derive(Default)]
struct SyntaxIndex {
    toml: BTreeMap<DocId, ParsedToml>,
    json: BTreeMap<DocId, JsonValue>,
}

struct ParsedToml {
    edit: ImDocument<String>,
    plain: TomlValue,
}

#[derive(Default)]
struct SemanticIndex {
    manifest: Option<ManifestNode>,
    qualifiers: BTreeMap<String, QualifierNode>,
    variables: BTreeMap<String, VariableNode>,
    external_values: BTreeMap<String, BTreeMap<String, ValueNode>>,
}

struct ManifestNode {
    doc: DocId,
    location: DiagnosticLocation,
    environments: WorkspaceEnvironmentCollection,
    context_schema: Option<ContextSchemaNode>,
    custom_rules: CustomRuleCollection,
}

struct WorkspaceEnvironmentNode {
    name: String,
    location: DiagnosticLocation,
}

enum WorkspaceEnvironmentCollection {
    Missing,
    Invalid {
        location: DiagnosticLocation,
    },
    Environments {
        location: DiagnosticLocation,
        values: Vec<WorkspaceEnvironmentNode>,
    },
}

struct ContextSchemaNode {
    location: DiagnosticLocation,
    schema: ProjectField<String>,
    invalid_shape: bool,
}

struct QualifierNode {
    doc: DocId,
    id: String,
    location: DiagnosticLocation,
    schema_version: ProjectField<i64>,
    description: Option<ProjectField<String>>,
    predicates: PredicateCollection,
}

struct PredicateNode {
    index: usize,
    location: DiagnosticLocation,
    attribute: ProjectField<String>,
    op: ProjectField<PredicateOp>,
    value: Option<ValueShapeNode>,
    salt: Option<ProjectField<String>>,
    range: Option<BucketRangeNode>,
    has_bucket_value: bool,
}

enum PredicateCollection {
    Missing { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
    Predicates(Vec<PredicateNode>),
}

#[derive(Clone)]
enum PredicateOp {
    Eq,
    Neq,
    In,
    NotIn,
    Gt,
    Gte,
    Lt,
    Lte,
    Bucket,
    Unknown(String),
}

struct BucketRangeNode {
    location: DiagnosticLocation,
    is_array: bool,
    len: usize,
    start: Option<i64>,
    end: Option<i64>,
}

struct VariableNode {
    doc: DocId,
    id: String,
    location: DiagnosticLocation,
    schema_version: ProjectField<i64>,
    description: Option<ProjectField<String>>,
    type_source: TypeSourceNode,
    values: ValuesNode,
    environments: EnvironmentCollection,
}

enum TypeSourceNode {
    Primitive(Spanned<String>),
    Schema(Spanned<String>),
    Missing { location: DiagnosticLocation },
    Conflict { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
}

struct ValuesNode {
    location: DiagnosticLocation,
    inline_keys: BTreeSet<String>,
    inline_values: BTreeMap<String, ValueNode>,
    external_keys: BTreeSet<String>,
    invalid_shape: bool,
}

struct ValueNode {
    key: String,
    location: DiagnosticLocation,
    value: JsonValue,
}

enum CustomRuleCollection {
    Rules(Vec<CustomRuleDeclarationNode>),
    Invalid { location: DiagnosticLocation },
}

struct CustomRuleDeclarationNode {
    location: DiagnosticLocation,
    id: ProjectField<String>,
    title: ProjectField<String>,
    help: ProjectField<String>,
}

enum EnvironmentCollection {
    Missing { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
    Environments(BTreeMap<String, EnvironmentBlockNode>),
}

struct EnvironmentBlockNode {
    environment: String,
    location: DiagnosticLocation,
    value: ProjectField<String>,
    rules: RuleCollection,
}

enum RuleCollection {
    Rules(Vec<VariableRuleNode>),
    Invalid { location: DiagnosticLocation },
}

struct VariableRuleNode {
    index: usize,
    location: DiagnosticLocation,
    qualifier: ProjectField<String>,
    value: ProjectField<String>,
    invalid_shape: bool,
}

#[derive(Clone)]
struct Spanned<T> {
    value: T,
    location: DiagnosticLocation,
}

enum ProjectField<T> {
    Present(Spanned<T>),
    Invalid { location: DiagnosticLocation },
    Missing { location: DiagnosticLocation },
}

impl<T> ProjectField<T> {
    fn location(&self) -> DiagnosticLocation {
        match self {
            Self::Present(value) => value.location.clone(),
            Self::Invalid { location } | Self::Missing { location } => location.clone(),
        }
    }
}

struct ValueShapeNode {
    location: DiagnosticLocation,
    shape: ValueShape,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueShape {
    String,
    Integer,
    Float,
    Boolean,
    Array,
    Table,
}

impl ValueShape {
    fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "int",
            Self::Float => "number",
            Self::Boolean => "bool",
            Self::Array => "list",
            Self::Table => "table",
        }
    }
}

fn project_manifest(document: &SourceDocument, toml: &ParsedToml) -> ManifestNode {
    let root = toml.edit.as_table();
    ManifestNode {
        doc: document.id,
        location: document.document_location(),
        environments: project_workspace_environments(document, root),
        context_schema: project_context_schema(document, root),
        custom_rules: project_workspace_custom_rules(document, root),
    }
}

fn project_workspace_environments(
    document: &SourceDocument,
    root: &Table,
) -> WorkspaceEnvironmentCollection {
    let Some(item) = root.get("environments") else {
        return WorkspaceEnvironmentCollection::Missing;
    };
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return WorkspaceEnvironmentCollection::Invalid { location };
    };
    let Some(values_item) = table.get("values") else {
        return WorkspaceEnvironmentCollection::Missing;
    };
    let location = item_location(document, values_item);
    let Some(values) = values_item.as_array() else {
        return WorkspaceEnvironmentCollection::Invalid { location };
    };

    WorkspaceEnvironmentCollection::Environments {
        location,
        values: values
            .iter()
            .filter_map(|value| {
                Some(WorkspaceEnvironmentNode {
                    name: value.as_str()?.to_owned(),
                    location: value_location(document, value),
                })
            })
            .collect(),
    }
}

fn project_context_schema(document: &SourceDocument, root: &Table) -> Option<ContextSchemaNode> {
    let item = root.get("context")?;
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return Some(ContextSchemaNode {
            location: location.clone(),
            schema: ProjectField::Invalid {
                location: location.clone(),
            },
            invalid_shape: true,
        });
    };

    Some(ContextSchemaNode {
        location: location.clone(),
        schema: string_field(document, table, "schema", location),
        invalid_shape: false,
    })
}

fn project_workspace_custom_rules(document: &SourceDocument, root: &Table) -> CustomRuleCollection {
    let Some(item) = root.get("lint") else {
        return CustomRuleCollection::Rules(Vec::new());
    };
    let Some(table) = item.as_table() else {
        return CustomRuleCollection::Invalid {
            location: item_location(document, item),
        };
    };
    project_custom_rule_declarations(document, table)
}

fn project_qualifier(document: &SourceDocument, toml: &ParsedToml, id: &str) -> QualifierNode {
    let root = toml.edit.as_table();
    let location = document.document_location();
    let schema_version = integer_field(document, root, "schema_version", location.clone());
    let description = optional_string_field(document, root, "description");
    let predicates = project_predicates(document, root);

    QualifierNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        predicates,
    }
}

fn project_predicates(document: &SourceDocument, root: &Table) -> PredicateCollection {
    let Some(item) = root.get("predicate") else {
        return PredicateCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(predicates) = item.as_array_of_tables() else {
        return PredicateCollection::Invalid {
            location: item_location(document, item),
        };
    };

    PredicateCollection::Predicates(
        predicates
            .iter()
            .enumerate()
            .map(|(index, table)| project_predicate(document, index, table))
            .collect(),
    )
}

fn project_predicate(document: &SourceDocument, index: usize, table: &Table) -> PredicateNode {
    let location = table_location(document, table);
    let attribute = string_field(document, table, "attribute", location.clone());
    let op = predicate_op_field(document, table, location.clone());
    let value = table
        .get("value")
        .map(|item| project_value_shape(document, item));
    let salt = table
        .get("salt")
        .map(|_| string_field(document, table, "salt", location.clone()));
    let range = table
        .get("range")
        .map(|item| project_bucket_range(document, item));
    let has_bucket_value = table.contains_key("value");

    PredicateNode {
        index,
        location,
        attribute,
        op,
        value,
        salt,
        range,
        has_bucket_value,
    }
}

fn project_variable(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
    source: &SourceStore,
) -> VariableNode {
    let root = toml.edit.as_table();
    let location = document.document_location();
    let schema_version = integer_field(document, root, "schema_version", location.clone());
    let description = optional_string_field(document, root, "description");
    let type_source = project_type_source(document, root, location.clone());
    let values = project_values(document, toml, root, id, source);
    let environments = project_environments(document, root, id);

    VariableNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        type_source,
        values,
        environments,
    }
}

fn project_type_source(
    document: &SourceDocument,
    root: &Table,
    location: DiagnosticLocation,
) -> TypeSourceNode {
    let type_item = root.get("type");
    let schema_item = root.get("schema");
    match (type_item, schema_item) {
        (None, None) => TypeSourceNode::Missing { location },
        (Some(_type_item), Some(schema_item)) => TypeSourceNode::Conflict {
            location: item_location(document, schema_item),
        },
        (Some(item), None) => match item.as_str() {
            Some(type_name) => TypeSourceNode::Primitive(Spanned {
                value: type_name.to_owned(),
                location: item_location(document, item),
            }),
            None => TypeSourceNode::Invalid {
                location: item_location(document, item),
            },
        },
        (None, Some(item)) => match item.as_str() {
            Some(schema) => TypeSourceNode::Schema(Spanned {
                value: schema.to_owned(),
                location: item_location(document, item),
            }),
            None => TypeSourceNode::Invalid {
                location: item_location(document, item),
            },
        },
    }
}

fn project_custom_rule_declarations(
    document: &SourceDocument,
    lint_table: &Table,
) -> CustomRuleCollection {
    let Some(item) = lint_table.get("rule") else {
        return CustomRuleCollection::Rules(Vec::new());
    };
    let Some(rules) = item.as_array_of_tables() else {
        return CustomRuleCollection::Invalid {
            location: item_location(document, item),
        };
    };

    CustomRuleCollection::Rules(
        rules
            .iter()
            .map(|table| project_custom_rule_declaration(document, table))
            .collect(),
    )
}

fn project_custom_rule_declaration(
    document: &SourceDocument,
    table: &Table,
) -> CustomRuleDeclarationNode {
    let location = table_location(document, table);
    CustomRuleDeclarationNode {
        location: location.clone(),
        id: string_field(document, table, "id", location.clone()),
        title: string_field(document, table, "title", location.clone()),
        help: string_field(document, table, "help", location),
    }
}

fn project_values(
    document: &SourceDocument,
    toml: &ParsedToml,
    root: &Table,
    id: &str,
    source: &SourceStore,
) -> ValuesNode {
    let external_keys = source.external_value_keys(id);
    let Some(item) = root.get("values") else {
        return ValuesNode {
            location: document.document_location(),
            inline_keys: BTreeSet::new(),
            inline_values: BTreeMap::new(),
            external_keys,
            invalid_shape: false,
        };
    };
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return ValuesNode {
            location,
            inline_keys: BTreeSet::new(),
            inline_values: BTreeMap::new(),
            external_keys,
            invalid_shape: true,
        };
    };

    let inline_values = project_inline_values(document, toml, table);
    ValuesNode {
        location,
        inline_keys: inline_values.keys().cloned().collect(),
        inline_values,
        external_keys,
        invalid_shape: false,
    }
}

fn project_inline_values(
    document: &SourceDocument,
    toml: &ParsedToml,
    table: &Table,
) -> BTreeMap<String, ValueNode> {
    let plain_values = toml.plain.get("values").and_then(TomlValue::as_table);
    table
        .iter()
        .filter_map(|(key, item)| {
            let value = plain_values?.get(key)?;
            Some((
                key.to_owned(),
                ValueNode {
                    key: key.to_owned(),
                    location: item_location(document, item),
                    value: json_from_toml_value(value),
                },
            ))
        })
        .collect()
}

fn project_external_value(document: &SourceDocument, toml: &ParsedToml, key: &str) -> ValueNode {
    let root = toml.edit.as_table();
    let wrapped_value = toml
        .plain
        .as_table()
        .filter(|table| table.len() == 1)
        .and_then(|table| table.get("value"));

    match wrapped_value {
        Some(value) => ValueNode {
            key: key.to_owned(),
            location: root
                .get("value")
                .map(|item| item_location(document, item))
                .unwrap_or_else(|| document.document_location()),
            value: json_from_toml_value(value),
        },
        None => ValueNode {
            key: key.to_owned(),
            location: document.document_location(),
            value: json_from_toml_value(&toml.plain),
        },
    }
}

fn project_environments(
    document: &SourceDocument,
    root: &Table,
    variable_id: &str,
) -> EnvironmentCollection {
    let Some(item) = root.get("env") else {
        return EnvironmentCollection::Missing {
            location: document.document_location(),
        };
    };
    let Some(table) = item.as_table() else {
        return EnvironmentCollection::Invalid {
            location: item_location(document, item),
        };
    };

    EnvironmentCollection::Environments(
        table
            .iter()
            .map(|(environment, item)| {
                (
                    environment.to_owned(),
                    project_environment_block(document, variable_id, environment, item),
                )
            })
            .collect(),
    )
}

fn project_environment_block(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    item: &Item,
) -> EnvironmentBlockNode {
    let location = item_location(document, item);
    let Some(table) = item.as_table() else {
        return EnvironmentBlockNode {
            environment: environment.to_owned(),
            location: location.clone(),
            value: ProjectField::Invalid {
                location: location.clone(),
            },
            rules: RuleCollection::Rules(Vec::new()),
        };
    };

    EnvironmentBlockNode {
        environment: environment.to_owned(),
        location: location.clone(),
        value: string_field(document, table, "value", location.clone()),
        rules: project_rules(document, variable_id, environment, table),
    }
}

fn project_rules(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    table: &Table,
) -> RuleCollection {
    let Some(item) = table.get("rule") else {
        return RuleCollection::Rules(Vec::new());
    };

    if let Some(rules) = item.as_array_of_tables() {
        return RuleCollection::Rules(
            rules
                .iter()
                .enumerate()
                .map(|(index, table)| {
                    project_rule_from_table(document, variable_id, environment, index, table)
                })
                .collect(),
        );
    }

    if let Some(array) = item.as_array() {
        return RuleCollection::Rules(
            array
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    project_rule_from_value(document, variable_id, environment, index, value)
                })
                .collect(),
        );
    }

    RuleCollection::Invalid {
        location: item_location(document, item),
    }
}

fn project_rule_from_table(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    index: usize,
    table: &Table,
) -> VariableRuleNode {
    let location = table_location(document, table);
    project_rule_from_table_like(
        document,
        variable_id,
        environment,
        index,
        table,
        location,
        false,
    )
}

fn project_rule_from_value(
    document: &SourceDocument,
    variable_id: &str,
    environment: &str,
    index: usize,
    value: &EditValue,
) -> VariableRuleNode {
    let location = value_location(document, value);
    let Some(table) = value.as_inline_table() else {
        return VariableRuleNode {
            index,
            location: location.clone(),
            qualifier: ProjectField::Invalid {
                location: location.clone(),
            },
            value: ProjectField::Invalid { location },
            invalid_shape: true,
        };
    };
    project_rule_from_table_like(
        document,
        variable_id,
        environment,
        index,
        table,
        location,
        false,
    )
}

fn project_rule_from_table_like(
    document: &SourceDocument,
    _variable_id: &str,
    _environment: &str,
    index: usize,
    table: &dyn TableLike,
    location: DiagnosticLocation,
    invalid_shape: bool,
) -> VariableRuleNode {
    VariableRuleNode {
        index,
        location: location.clone(),
        qualifier: string_field(document, table, "qualifier", location.clone()),
        value: string_field(document, table, "value", location.clone()),
        invalid_shape,
    }
}

fn integer_field(
    document: &SourceDocument,
    table: &dyn TableLike,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<i64> {
    match table.get(key) {
        Some(item) => match item.as_integer() {
            Some(value) => ProjectField::Present(Spanned {
                value,
                location: item_location(document, item),
            }),
            None => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Missing {
            location: missing_location,
        },
    }
}

fn string_field(
    document: &SourceDocument,
    table: &dyn TableLike,
    key: &str,
    missing_location: DiagnosticLocation,
) -> ProjectField<String> {
    match table.get(key) {
        Some(item) => match item.as_str() {
            Some(value) => ProjectField::Present(Spanned {
                value: value.to_owned(),
                location: item_location(document, item),
            }),
            None => ProjectField::Invalid {
                location: item_location(document, item),
            },
        },
        None => ProjectField::Missing {
            location: missing_location,
        },
    }
}

fn optional_string_field(
    document: &SourceDocument,
    table: &dyn TableLike,
    key: &str,
) -> Option<ProjectField<String>> {
    let item = table.get(key)?;
    Some(match item.as_str() {
        Some(value) => ProjectField::Present(Spanned {
            value: value.to_owned(),
            location: item_location(document, item),
        }),
        None => ProjectField::Invalid {
            location: item_location(document, item),
        },
    })
}

fn predicate_op_field(
    document: &SourceDocument,
    table: &Table,
    missing_location: DiagnosticLocation,
) -> ProjectField<PredicateOp> {
    match string_field(document, table, "op", missing_location) {
        ProjectField::Present(op) => ProjectField::Present(Spanned {
            value: PredicateOp::from_str(&op.value),
            location: op.location,
        }),
        ProjectField::Invalid { location } => ProjectField::Invalid { location },
        ProjectField::Missing { location } => ProjectField::Missing { location },
    }
}

impl PredicateOp {
    const COMPLETION_LABELS: &'static [&'static str] = &[
        "eq", "neq", "in", "not_in", "gt", "gte", "lt", "lte", "bucket",
    ];

    fn from_str(op: &str) -> Self {
        match op {
            "eq" => Self::Eq,
            "neq" => Self::Neq,
            "in" => Self::In,
            "not_in" => Self::NotIn,
            "gt" => Self::Gt,
            "gte" => Self::Gte,
            "lt" => Self::Lt,
            "lte" => Self::Lte,
            "bucket" => Self::Bucket,
            op => Self::Unknown(op.to_owned()),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Eq => "eq",
            Self::Neq => "neq",
            Self::In => "in",
            Self::NotIn => "not_in",
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::Bucket => "bucket",
            Self::Unknown(op) => op,
        }
    }
}

fn project_value_shape(document: &SourceDocument, item: &Item) -> ValueShapeNode {
    ValueShapeNode {
        location: item_location(document, item),
        shape: value_shape(item),
    }
}

fn project_bucket_range(document: &SourceDocument, item: &Item) -> BucketRangeNode {
    let location = item_location(document, item);
    let Some(array) = item.as_array() else {
        return BucketRangeNode {
            location,
            is_array: false,
            len: 0,
            start: None,
            end: None,
        };
    };
    let values: Vec<_> = array.iter().collect();
    BucketRangeNode {
        location,
        is_array: true,
        len: values.len(),
        start: values.first().and_then(|value| value.as_integer()),
        end: values.get(1).and_then(|value| value.as_integer()),
    }
}

fn value_shape(item: &Item) -> ValueShape {
    if item.as_str().is_some() {
        ValueShape::String
    } else if item.as_integer().is_some() {
        ValueShape::Integer
    } else if item.as_float().is_some() {
        ValueShape::Float
    } else if item.as_bool().is_some() {
        ValueShape::Boolean
    } else if item.as_array().is_some() {
        ValueShape::Array
    } else {
        ValueShape::Table
    }
}

fn json_from_toml_value(value: &TomlValue) -> JsonValue {
    serde_json::to_value(value).unwrap_or(JsonValue::Null)
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
            Some((
                CustomRuleDefinition::new(rule_id, title.value.clone(), help.value.clone()),
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

#[derive(Clone)]
struct RegisteredCustomLint {
    file_path: String,
    script: String,
    stage: LintStage,
    selector: RegisteredLintSelector,
    definition: CustomRuleDefinition,
    handler: String,
}

#[derive(Clone)]
struct RegisteredLintSelector {
    entity: RegisteredLintEntity,
    field: Option<RegisteredLintField>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegisteredLintEntity {
    Workspace,
    Qualifier,
    Variable,
    Value,
    Schema,
}

#[derive(Clone)]
enum RegisteredLintField {
    Workspace(WorkspaceLintField),
    Qualifier(QualifierLintField),
    Variable(VariableLintField),
    Value(ValueLintField),
    Schema(SchemaLintField),
}

#[derive(Clone)]
enum WorkspaceLintField {
    Environments,
    ContextSchema,
}

#[derive(Clone)]
enum QualifierLintField {
    Id,
    Description,
    Predicates,
}

#[derive(Clone)]
enum VariableLintField {
    Id,
    Description,
    Type,
    Schema,
    Values,
    Environments,
}

#[derive(Clone)]
enum ValueLintField {
    Key,
    Value,
    JsonPath(Vec<String>),
}

#[derive(Clone)]
enum SchemaLintField {
    Json,
    JsonPath(Vec<String>),
}

async fn register_custom_lints(ctx: &mut LintContext) {
    let workspace_rules = workspace_custom_rule_definitions(ctx);
    let documents = ctx
        .source
        .documents
        .values()
        .filter(|document| matches!(&document.kind, DocumentKind::CustomLint))
        .cloned()
        .collect::<Vec<_>>();

    for document in documents {
        if let Some(read_error) = &document.read_error {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintFailed,
                EntityId::CustomLint {
                    path: document.path.clone(),
                },
                document.document_location(),
                format!("failed to read custom lint {}: {read_error}", document.path),
            );
            continue;
        }

        let input = lua_lint::RegisterLintInput {
            lint_path: ctx.source.root.join(&document.path),
            script: document.text.clone(),
        };
        let registrations = match lua_lint::register_pipeline_lint(input).await {
            Ok(registrations) => registrations,
            Err(err) => {
                push_register_diagnostic(
                    &mut ctx.diagnostics,
                    RototoRuleId::CustomLintFailed,
                    EntityId::CustomLint {
                        path: document.path.clone(),
                    },
                    document.document_location(),
                    err.to_string(),
                );
                continue;
            }
        };

        for registration in registrations {
            match validate_custom_registration(&workspace_rules, &registration) {
                Ok((stage, selector, definition)) => {
                    ctx.registered_custom_lints.push(RegisteredCustomLint {
                        file_path: document.path.clone(),
                        script: document.text.clone(),
                        stage,
                        selector,
                        definition,
                        handler: registration.handler,
                    });
                }
                Err((rule, message)) => push_register_diagnostic(
                    &mut ctx.diagnostics,
                    rule,
                    EntityId::CustomLint {
                        path: document.path.clone(),
                    },
                    document.document_location(),
                    message,
                ),
            }
        }
    }
}

fn validate_custom_registration(
    workspace_rules: &BTreeMap<CustomRuleId, CustomRuleDefinition>,
    registration: &lua_lint::RawCustomLintRegistration,
) -> std::result::Result<
    (LintStage, RegisteredLintSelector, CustomRuleDefinition),
    (RototoRuleId, String),
> {
    let stage = parse_registered_lint_stage(&registration.stage)?;
    let selector =
        parse_registered_lint_selector(&registration.entity, registration.field.as_deref())?;
    if !registration.handler_exists {
        return Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!(
                "custom lint registration handler is not callable: {}",
                registration.handler
            ),
        ));
    }

    let rule = CustomRuleId::parse(&registration.rule).map_err(|err| {
        (
            RototoRuleId::CustomLintRegistrationInvalid,
            format!(
                "custom lint registration rule id is invalid: {}: {err}",
                registration.rule
            ),
        )
    })?;
    let definition = workspace_rules.get(&rule).cloned().ok_or_else(|| {
        (
            RototoRuleId::CustomLintUnknownRule,
            format!("custom lint registration references undeclared rule: {rule}"),
        )
    })?;

    Ok((stage, selector, definition))
}

fn parse_registered_lint_stage(
    stage: &str,
) -> std::result::Result<LintStage, (RototoRuleId, String)> {
    match stage {
        "project" => Ok(LintStage::Project),
        "reference" => Ok(LintStage::Reference),
        "value" => Ok(LintStage::Value),
        "graph" => Ok(LintStage::Graph),
        "policy" => Ok(LintStage::Policy),
        _ => Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!("custom lint registration has unsupported stage: {stage}"),
        )),
    }
}

fn parse_registered_lint_selector(
    entity: &str,
    field: Option<&str>,
) -> std::result::Result<RegisteredLintSelector, (RototoRuleId, String)> {
    match entity {
        "workspace" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Workspace,
            field: parse_workspace_lint_field(field)?,
        }),
        "qualifier" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Qualifier,
            field: parse_qualifier_lint_field(field)?,
        }),
        "variable" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Variable,
            field: parse_variable_lint_field(field)?,
        }),
        "value" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Value,
            field: parse_value_lint_field(field)?,
        }),
        "schema" => Ok(RegisteredLintSelector {
            entity: RegisteredLintEntity::Schema,
            field: parse_schema_lint_field(field)?,
        }),
        _ => Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!("custom lint registration has unsupported entity: {entity}"),
        )),
    }
}

fn parse_workspace_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("environments") => Ok(Some(RegisteredLintField::Workspace(
            WorkspaceLintField::Environments,
        ))),
        Some("context_schema") => Ok(Some(RegisteredLintField::Workspace(
            WorkspaceLintField::ContextSchema,
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_qualifier_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("id") => Ok(Some(RegisteredLintField::Qualifier(QualifierLintField::Id))),
        Some("description") => Ok(Some(RegisteredLintField::Qualifier(
            QualifierLintField::Description,
        ))),
        Some("predicates") => Ok(Some(RegisteredLintField::Qualifier(
            QualifierLintField::Predicates,
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_variable_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("id") => Ok(Some(RegisteredLintField::Variable(VariableLintField::Id))),
        Some("description") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Description,
        ))),
        Some("type") => Ok(Some(RegisteredLintField::Variable(VariableLintField::Type))),
        Some("schema") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Schema,
        ))),
        Some("values") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Values,
        ))),
        Some("environments") => Ok(Some(RegisteredLintField::Variable(
            VariableLintField::Environments,
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_value_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("key") => Ok(Some(RegisteredLintField::Value(ValueLintField::Key))),
        Some("value") => Ok(Some(RegisteredLintField::Value(ValueLintField::Value))),
        Some(field) if field.starts_with("value.") => Ok(Some(RegisteredLintField::Value(
            ValueLintField::JsonPath(parse_json_path_selector(field, "value.")?),
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_schema_lint_field(
    field: Option<&str>,
) -> std::result::Result<Option<RegisteredLintField>, (RototoRuleId, String)> {
    match field {
        None => Ok(None),
        Some("json") => Ok(Some(RegisteredLintField::Schema(SchemaLintField::Json))),
        Some(field) if field.starts_with("json.") => Ok(Some(RegisteredLintField::Schema(
            SchemaLintField::JsonPath(parse_json_path_selector(field, "json.")?),
        ))),
        Some(field) => unsupported_registration_field(field),
    }
}

fn parse_json_path_selector(
    field: &str,
    prefix: &str,
) -> std::result::Result<Vec<String>, (RototoRuleId, String)> {
    let path = field.strip_prefix(prefix).unwrap_or_default();
    let segments = path.split('.').map(str::to_owned).collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| !valid_json_path_segment(segment))
    {
        return Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!("custom lint registration has unsupported field: {field}"),
        ));
    }
    Ok(segments)
}

fn valid_json_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn unsupported_registration_field<T>(
    field: &str,
) -> std::result::Result<Option<T>, (RototoRuleId, String)> {
    Err((
        RototoRuleId::CustomLintRegistrationInvalid,
        format!("custom lint registration has unsupported field: {field}"),
    ))
}

fn expanded_variable_toml_json(ctx: &LintContext, variable: &VariableNode) -> JsonValue {
    let mut toml = ctx
        .syntax
        .toml
        .get(&variable.doc)
        .map(|parsed| json_from_toml_value(&parsed.plain))
        .unwrap_or_else(|| JsonValue::Object(serde_json::Map::new()));
    let mut values = serde_json::Map::new();
    for value in variable_values(ctx, variable) {
        values.insert(value.key.clone(), value.value.clone());
    }

    if let JsonValue::Object(object) = &mut toml {
        object.insert("values".to_owned(), JsonValue::Object(values));
    }
    toml
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

struct RegisteredLintTargetInstance {
    entity: EntityId,
    location: DiagnosticLocation,
    data: JsonValue,
}

async fn run_registered_custom_lints(ctx: &mut LintContext, stage: LintStage) {
    let registrations = ctx
        .registered_custom_lints
        .iter()
        .filter(|registration| registration.stage == stage)
        .cloned()
        .collect::<Vec<_>>();

    for registration in registrations {
        let targets = registered_lint_targets(ctx, &registration.selector);
        for target in targets {
            let input = lua_lint::RegisteredLintInput {
                stage: lint_stage_label(stage).to_owned(),
                target: lua_lint::RegisteredLintTarget {
                    entity: registered_lint_entity_label(registration.selector.entity).to_owned(),
                    data: target.data,
                },
                lint_path: ctx.source.root.join(&registration.file_path),
                script: registration.script.clone(),
                handler: registration.handler.clone(),
            };

            match lua_lint::lint_registered_target(input).await {
                Ok(outputs) => {
                    for output in outputs {
                        ctx.diagnostics.push(LintDiagnostic::custom(
                            &registration.definition,
                            stage,
                            target.entity.clone(),
                            target.location.clone(),
                            output.message,
                        ));
                    }
                }
                Err(err) => push_stage_diagnostic(
                    &mut ctx.diagnostics,
                    stage,
                    RototoRuleId::CustomLintFailed,
                    target.entity.clone(),
                    target.location.clone(),
                    format!(
                        "custom lint handler failed in {}: {err}",
                        registration.file_path
                    ),
                ),
            }
        }
    }
}

fn registered_lint_targets(
    ctx: &LintContext,
    selector: &RegisteredLintSelector,
) -> Vec<RegisteredLintTargetInstance> {
    match selector.entity {
        RegisteredLintEntity::Workspace => {
            registered_workspace_targets(ctx, selector.field.as_ref())
        }
        RegisteredLintEntity::Qualifier => {
            registered_qualifier_targets(ctx, selector.field.as_ref())
        }
        RegisteredLintEntity::Variable => registered_variable_targets(ctx, selector.field.as_ref()),
        RegisteredLintEntity::Value => registered_value_targets(ctx, selector.field.as_ref()),
        RegisteredLintEntity::Schema => registered_schema_targets(ctx, selector.field.as_ref()),
    }
}

fn registered_workspace_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    let Some(manifest) = &ctx.index.manifest else {
        return Vec::new();
    };
    let Some(document) = ctx.source.documents.get(&manifest.doc) else {
        return Vec::new();
    };

    let environments = declared_workspace_environments(ctx)
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
    let context_schema =
        manifest
            .context_schema
            .as_ref()
            .and_then(|context| match &context.schema {
                ProjectField::Present(schema) => Some(schema.value.clone()),
                _ => None,
            });

    vec![RegisteredLintTargetInstance {
        entity: EntityId::Workspace,
        location: registered_workspace_location(ctx, manifest, field),
        data: serde_json::json!({
            "kind": "workspace",
            "root": ctx.source.root.display().to_string(),
            "manifest": {
                "uri": document.uri,
                "path": document.path,
                "toml": parsed_toml_json(ctx, manifest.doc),
            },
            "environments": environments,
            "context_schema": context_schema,
        }),
    }]
}

fn registered_qualifier_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    ctx.index
        .qualifiers
        .values()
        .filter_map(|qualifier| {
            let document = ctx.source.documents.get(&qualifier.doc)?;
            Some(RegisteredLintTargetInstance {
                entity: EntityId::Qualifier {
                    id: qualifier.id.clone(),
                },
                location: registered_qualifier_location(ctx, qualifier, field),
                data: serde_json::json!({
                    "kind": "qualifier",
                    "id": qualifier.id,
                    "uri": document.uri,
                    "path": document.path,
                    "toml": parsed_toml_json(ctx, qualifier.doc),
                }),
            })
        })
        .collect()
}

fn registered_variable_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    ctx.index
        .variables
        .values()
        .filter_map(|variable| {
            let document = ctx.source.documents.get(&variable.doc)?;
            Some(RegisteredLintTargetInstance {
                entity: EntityId::Variable {
                    id: variable.id.clone(),
                },
                location: registered_variable_location(ctx, variable, field),
                data: serde_json::json!({
                    "kind": "variable",
                    "id": variable.id,
                    "uri": document.uri,
                    "path": document.path,
                    "toml": expanded_variable_toml_json(ctx, variable),
                }),
            })
        })
        .collect()
}

fn registered_value_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    let mut targets = Vec::new();
    for variable in ctx.index.variables.values() {
        let Some(variable_document) = ctx.source.documents.get(&variable.doc) else {
            continue;
        };
        for value in variable_values(ctx, variable) {
            targets.push(RegisteredLintTargetInstance {
                entity: EntityId::Value {
                    variable: variable.id.clone(),
                    key: value.key.clone(),
                },
                location: registered_value_location(value, field),
                data: serde_json::json!({
                    "kind": "value",
                    "name": value.key,
                    "value": value.value,
                    "selected": selected_value_field(&value.value, field),
                    "variable": {
                        "id": variable.id,
                        "uri": variable_document.uri,
                        "path": variable_document.path,
                    },
                }),
            });
        }
    }
    targets
}

fn registered_schema_targets(
    ctx: &LintContext,
    field: Option<&RegisteredLintField>,
) -> Vec<RegisteredLintTargetInstance> {
    ctx.source
        .documents
        .values()
        .filter(|document| matches!(&document.kind, DocumentKind::Schema))
        .filter_map(|document| {
            let schema = ctx.syntax.json.get(&document.id)?;
            Some(RegisteredLintTargetInstance {
                entity: EntityId::Schema {
                    path: document.path.clone(),
                },
                location: registered_schema_location(document, field),
                data: serde_json::json!({
                    "kind": "schema",
                    "uri": document.uri,
                    "path": document.path,
                    "json": schema,
                    "selected": selected_schema_field(schema, field),
                }),
            })
        })
        .collect()
}

fn registered_workspace_location(
    ctx: &LintContext,
    manifest: &ManifestNode,
    field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    match field {
        Some(RegisteredLintField::Workspace(WorkspaceLintField::Environments)) => {
            toml_root_item_location(ctx, manifest.doc, "environments")
                .unwrap_or_else(|| manifest.location.clone())
        }
        Some(RegisteredLintField::Workspace(WorkspaceLintField::ContextSchema)) => manifest
            .context_schema
            .as_ref()
            .map(|context| context.location.clone())
            .unwrap_or_else(|| manifest.location.clone()),
        _ => manifest.location.clone(),
    }
}

fn registered_qualifier_location(
    ctx: &LintContext,
    qualifier: &QualifierNode,
    field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    match field {
        Some(RegisteredLintField::Qualifier(QualifierLintField::Description)) => {
            toml_root_item_location(ctx, qualifier.doc, "description")
                .unwrap_or_else(|| qualifier.location.clone())
        }
        Some(RegisteredLintField::Qualifier(QualifierLintField::Predicates)) => {
            toml_root_item_location(ctx, qualifier.doc, "predicate")
                .unwrap_or_else(|| qualifier.location.clone())
        }
        _ => qualifier.location.clone(),
    }
}

fn registered_variable_location(
    ctx: &LintContext,
    variable: &VariableNode,
    field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    match field {
        Some(RegisteredLintField::Variable(VariableLintField::Description)) => {
            toml_root_item_location(ctx, variable.doc, "description")
                .unwrap_or_else(|| variable.location.clone())
        }
        Some(RegisteredLintField::Variable(VariableLintField::Type))
            if matches!(&variable.type_source, TypeSourceNode::Primitive(_)) =>
        {
            type_source_location(&variable.type_source)
        }
        Some(RegisteredLintField::Variable(VariableLintField::Schema))
            if matches!(&variable.type_source, TypeSourceNode::Schema(_)) =>
        {
            type_source_location(&variable.type_source)
        }
        Some(RegisteredLintField::Variable(VariableLintField::Values)) => {
            variable.values.location.clone()
        }
        Some(RegisteredLintField::Variable(VariableLintField::Environments)) => {
            toml_root_item_location(ctx, variable.doc, "env").unwrap_or_else(|| {
                environment_collection_location(&variable.environments, variable.location.clone())
            })
        }
        _ => variable.location.clone(),
    }
}

fn registered_value_location(
    value: &ValueNode,
    _field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    value.location.clone()
}

fn registered_schema_location(
    document: &SourceDocument,
    _field: Option<&RegisteredLintField>,
) -> DiagnosticLocation {
    document.document_location()
}

fn toml_root_item_location(ctx: &LintContext, doc: DocId, key: &str) -> Option<DiagnosticLocation> {
    let document = ctx.source.documents.get(&doc)?;
    let parsed = ctx.syntax.toml.get(&doc)?;
    parsed
        .edit
        .as_table()
        .get(key)
        .map(|item| item_location(document, item))
}

fn parsed_toml_json(ctx: &LintContext, doc: DocId) -> JsonValue {
    ctx.syntax
        .toml
        .get(&doc)
        .map(|parsed| json_from_toml_value(&parsed.plain))
        .unwrap_or(JsonValue::Null)
}

fn type_source_location(type_source: &TypeSourceNode) -> DiagnosticLocation {
    match type_source {
        TypeSourceNode::Primitive(type_name) => type_name.location.clone(),
        TypeSourceNode::Schema(schema) => schema.location.clone(),
        TypeSourceNode::Missing { location }
        | TypeSourceNode::Conflict { location }
        | TypeSourceNode::Invalid { location } => location.clone(),
    }
}

fn environment_collection_location(
    environments: &EnvironmentCollection,
    fallback: DiagnosticLocation,
) -> DiagnosticLocation {
    match environments {
        EnvironmentCollection::Missing { location }
        | EnvironmentCollection::Invalid { location } => location.clone(),
        EnvironmentCollection::Environments(_) => fallback,
    }
}

fn selected_value_field(value: &JsonValue, field: Option<&RegisteredLintField>) -> JsonValue {
    match field {
        Some(RegisteredLintField::Value(ValueLintField::JsonPath(path))) => {
            json_value_at_path(value, path)
                .cloned()
                .unwrap_or(JsonValue::Null)
        }
        _ => value.clone(),
    }
}

fn selected_schema_field(schema: &JsonValue, field: Option<&RegisteredLintField>) -> JsonValue {
    match field {
        Some(RegisteredLintField::Schema(SchemaLintField::JsonPath(path))) => {
            json_value_at_path(schema, path)
                .cloned()
                .unwrap_or(JsonValue::Null)
        }
        _ => schema.clone(),
    }
}

fn json_value_at_path<'a>(value: &'a JsonValue, path: &[String]) -> Option<&'a JsonValue> {
    let mut current = value;
    for segment in path {
        current = current.get(segment)?;
    }
    Some(current)
}

fn lint_stage_label(stage: LintStage) -> &'static str {
    match stage {
        LintStage::Discover => "discover",
        LintStage::Parse => "parse",
        LintStage::Project => "project",
        LintStage::Register => "register",
        LintStage::Reference => "reference",
        LintStage::Value => "value",
        LintStage::Graph => "graph",
        LintStage::Policy => "policy",
    }
}

fn registered_lint_entity_label(entity: RegisteredLintEntity) -> &'static str {
    match entity {
        RegisteredLintEntity::Workspace => "workspace",
        RegisteredLintEntity::Qualifier => "qualifier",
        RegisteredLintEntity::Variable => "variable",
        RegisteredLintEntity::Value => "value",
        RegisteredLintEntity::Schema => "schema",
    }
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
    let schema = match variable_schema(ctx, variable, schema_ref) {
        Ok(schema) => schema,
        Err(err) => {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::VariableSchemaRef,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                schema_ref.location.clone(),
                format!("variable schema reference is invalid: {err}"),
            );
            return;
        }
    };

    let validator = match jsonschema::validator_for(schema) {
        Ok(validator) => validator,
        Err(err) => {
            push_value_diagnostic(
                diagnostics,
                RototoRuleId::VariableSchemaRef,
                EntityId::Variable {
                    id: variable.id.clone(),
                },
                schema_ref.location.clone(),
                format!("variable schema reference is invalid: {err}"),
            );
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

fn variable_schema<'a>(
    ctx: &'a LintContext,
    variable: &VariableNode,
    schema_ref: &Spanned<String>,
) -> std::result::Result<&'a JsonValue, String> {
    let Some(schema_path) =
        resolve_workspace_relative_path(&variable.location.path, &schema_ref.value)
    else {
        return Err(format!(
            "{} is not a relative path inside the workspace",
            schema_ref.value
        ));
    };
    let document = ctx
        .source
        .document_by_path(&schema_path)
        .ok_or_else(|| format!("schema file not found: {schema_path}"))?;
    if !matches!(&document.kind, DocumentKind::Schema) {
        return Err(format!("path is not a schema document: {schema_path}"));
    }
    ctx.syntax
        .json
        .get(&document.id)
        .ok_or_else(|| format!("schema file could not be parsed: {schema_path}"))
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
            rule.id.location(),
            rule.title.location(),
            rule.help.location(),
        ] {
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
            &type_source_location(&variable.type_source),
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
    Some(CustomRuleDefinition::new(
        rule_id,
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

struct SourceStore {
    root: PathBuf,
    overlays: BTreeMap<String, OverlayDocument>,
    documents: BTreeMap<DocId, SourceDocument>,
    by_path: BTreeMap<String, DocId>,
}

impl SourceStore {
    fn new(root: PathBuf, overlays: BTreeMap<String, OverlayDocument>) -> Self {
        Self {
            root,
            overlays,
            documents: BTreeMap::new(),
            by_path: BTreeMap::new(),
        }
    }

    async fn add_named_toml_documents(
        &mut self,
        directory: &str,
        collection: DocumentCollection,
    ) -> Result<()> {
        let directory_path = self.root.join(directory);
        let entries = match sorted_directory_entries(&directory_path).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    directory_path.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let relative_path =
                PathBuf::from(directory).join(path.file_name().expect("entry has filename"));
            let kind = match collection {
                DocumentCollection::Qualifiers => DocumentKind::Qualifier {
                    id: stem.to_owned(),
                },
                DocumentCollection::Variables => DocumentKind::Variable {
                    id: stem.to_owned(),
                },
            };
            self.add_disk_document(relative_path, kind).await;
            if matches!(collection, DocumentCollection::Variables) {
                self.add_external_value_documents(stem).await?;
            }
        }

        Ok(())
    }

    async fn add_external_value_documents(&mut self, variable_id: &str) -> Result<()> {
        let values_dir = self
            .root
            .join("variables")
            .join(format!("{variable_id}-values"));
        let entries = match sorted_directory_entries(&values_dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    values_dir.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }
            let Some(value_key) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let relative_path = PathBuf::from("variables")
                .join(format!("{variable_id}-values"))
                .join(path.file_name().expect("entry has filename"));
            self.add_disk_document(
                relative_path,
                DocumentKind::ExternalValue {
                    variable_id: variable_id.to_owned(),
                    value_key: value_key.to_owned(),
                },
            )
            .await;
        }

        Ok(())
    }

    async fn add_schema_documents(&mut self) -> Result<()> {
        let schemas = self.root.join("schemas");
        let entries = match sorted_directory_entries(&schemas).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    schemas.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let relative_path =
                PathBuf::from("schemas").join(path.file_name().expect("entry has filename"));
            self.add_disk_document(relative_path, DocumentKind::Schema)
                .await;
        }

        Ok(())
    }

    async fn add_custom_lint_documents(&mut self) -> Result<()> {
        let lint = self.root.join("lint");
        let entries = match sorted_directory_entries(&lint).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(RototoError::new(format!(
                    "failed to read {}: {err}",
                    lint.display()
                )));
            }
        };

        for path in entries {
            if path.extension().and_then(|extension| extension.to_str()) != Some("lua") {
                continue;
            }
            let relative_path =
                PathBuf::from("lint").join(path.file_name().expect("entry has filename"));
            self.add_disk_document(relative_path, DocumentKind::CustomLint)
                .await;
        }

        Ok(())
    }

    async fn add_disk_document(&mut self, relative_path: PathBuf, kind: DocumentKind) -> DocId {
        let path = workspace_path(&relative_path);
        if let Some(doc) = self.by_path.get(&path).copied() {
            return doc;
        }

        let id = DocId(self.documents.len() as u32);
        let absolute_path = self.root.join(&relative_path);
        let (text, version, read_error) = match self.overlays.get(&path) {
            Some(overlay) => (overlay.text.clone(), overlay.version, None),
            None => match tokio::fs::read_to_string(&absolute_path).await {
                Ok(text) => (text, None, None),
                Err(err) => (String::new(), None, Some(err.to_string())),
            },
        };
        let document = SourceDocument {
            id,
            path: path.clone(),
            uri: file_uri(&absolute_path),
            version,
            kind,
            line_index: LineIndex::new(&text),
            text,
            read_error,
        };

        self.documents.insert(id, document);
        self.by_path.insert(path, id);
        id
    }

    fn external_value_keys(&self, variable_id: &str) -> BTreeSet<String> {
        self.documents
            .values()
            .filter_map(|document| match &document.kind {
                DocumentKind::ExternalValue {
                    variable_id: document_variable_id,
                    value_key,
                } if document_variable_id == variable_id => Some(value_key.clone()),
                _ => None,
            })
            .collect()
    }

    fn document_by_path(&self, path: &str) -> Option<&SourceDocument> {
        self.by_path
            .get(path)
            .and_then(|document_id| self.documents.get(document_id))
    }

    fn document_summaries(&self) -> Vec<SourceDocumentSummary> {
        self.documents
            .values()
            .map(|document| SourceDocumentSummary {
                id: document.id,
                path: document.path.clone(),
                uri: document.uri.clone(),
                version: document.version,
                kind: document.kind.summary_kind(),
            })
            .collect()
    }
}

#[derive(Clone)]
enum DocumentKind {
    Manifest,
    Qualifier {
        id: String,
    },
    Variable {
        id: String,
    },
    ExternalValue {
        variable_id: String,
        value_key: String,
    },
    Schema,
    CustomLint,
}

impl DocumentKind {
    fn summary_kind(&self) -> SourceKind {
        match self {
            Self::Manifest => SourceKind::Manifest,
            Self::Qualifier { .. } => SourceKind::Qualifier,
            Self::Variable { .. } => SourceKind::Variable,
            Self::ExternalValue { .. } => SourceKind::ExternalValue,
            Self::Schema => SourceKind::Schema,
            Self::CustomLint => SourceKind::CustomLint,
        }
    }
}

#[derive(Clone)]
struct SourceDocument {
    id: DocId,
    path: String,
    uri: String,
    version: Option<i32>,
    kind: DocumentKind,
    text: String,
    line_index: LineIndex,
    read_error: Option<String>,
}

impl SourceDocument {
    fn document_location(&self) -> DiagnosticLocation {
        DiagnosticLocation::document(self.id, self.path.clone())
    }

    fn span_location(&self, range: Range<usize>) -> DiagnosticLocation {
        DiagnosticLocation::span(self.id, self.path.clone(), self.line_index.range(range))
    }
}

#[derive(Clone)]
struct LineIndex {
    line_starts: Vec<usize>,
    text_len: usize,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self {
            line_starts,
            text_len: text.len(),
        }
    }

    fn range(&self, range: Range<usize>) -> SourceRange {
        let start = range.start.min(self.text_len);
        let end = range.end.min(self.text_len).max(start);
        SourceRange {
            start: self.position(start),
            end: self.position(end),
        }
    }

    fn position(&self, offset: usize) -> SourcePosition {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next_line) => next_line.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        SourcePosition {
            line,
            character: offset.saturating_sub(line_start),
        }
    }

    fn offset_for_line_character(&self, line: usize, character: usize) -> usize {
        let line_start = self.line_starts.get(line).copied().unwrap_or(self.text_len);
        line_start.saturating_add(character).min(self.text_len)
    }
}

#[derive(Clone, Copy)]
enum DocumentCollection {
    Qualifiers,
    Variables,
}

fn item_location(document: &SourceDocument, item: &Item) -> DiagnosticLocation {
    item.span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

fn table_location(document: &SourceDocument, table: &Table) -> DiagnosticLocation {
    table
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

fn value_location(document: &SourceDocument, value: &EditValue) -> DiagnosticLocation {
    value
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location())
}

fn read_error_diagnostic(document: &SourceDocument, read_error: &str) -> LintDiagnostic {
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        document.document_location(),
        format!("failed to read {}: {read_error}", document.path),
    )
}

fn toml_edit_parse_diagnostic(
    document: &SourceDocument,
    err: &toml_edit::TomlError,
) -> LintDiagnostic {
    let location = err
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location());
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        location,
        format!("failed to parse {}: {err}", document.path),
    )
}

fn toml_de_parse_diagnostic(document: &SourceDocument, err: &toml::de::Error) -> LintDiagnostic {
    let location = err
        .span()
        .map(|span| document.span_location(span))
        .unwrap_or_else(|| document.document_location());
    LintDiagnostic::rototo(
        parse_failed_rule(&document.kind),
        LintStage::Parse,
        entity_for_document(document),
        location,
        format!("failed to parse {}: {err}", document.path),
    )
}

fn json_parse_diagnostic(document: &SourceDocument, err: &serde_json::Error) -> LintDiagnostic {
    let line = err.line().saturating_sub(1);
    let column = err.column();
    let start = document.line_index.offset_for_line_character(line, column);
    let end = start.saturating_add(1).min(document.text.len());
    LintDiagnostic::rototo(
        RototoRuleId::SchemaParseFailed,
        LintStage::Parse,
        entity_for_document(document),
        document.span_location(start..end),
        format!("failed to parse {}: {err}", document.path),
    )
}

fn parse_failed_rule(kind: &DocumentKind) -> RototoRuleId {
    match kind {
        DocumentKind::Manifest => RototoRuleId::WorkspaceManifestParseFailed,
        DocumentKind::Qualifier { .. } => RototoRuleId::QualifierParseFailed,
        DocumentKind::Variable { .. } => RototoRuleId::VariableParseFailed,
        DocumentKind::ExternalValue { .. } => RototoRuleId::VariableExternalValueParseFailed,
        DocumentKind::Schema => RototoRuleId::SchemaParseFailed,
        DocumentKind::CustomLint => RototoRuleId::CustomLintFailed,
    }
}

fn entity_for_document(document: &SourceDocument) -> EntityId {
    match &document.kind {
        DocumentKind::Manifest => EntityId::Manifest,
        DocumentKind::Qualifier { id } => EntityId::Qualifier { id: id.clone() },
        DocumentKind::Variable { id } => EntityId::Variable { id: id.clone() },
        DocumentKind::ExternalValue {
            variable_id,
            value_key,
        } => EntityId::Value {
            variable: variable_id.clone(),
            key: value_key.clone(),
        },
        DocumentKind::Schema => EntityId::Schema {
            path: document.path.clone(),
        },
        DocumentKind::CustomLint => EntityId::CustomLint {
            path: document.path.clone(),
        },
    }
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

async fn sorted_directory_entries(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(path).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let metadata = entry.metadata().await?;
        if metadata.is_file() {
            entries.push(entry.path());
        }
    }
    entries.sort();
    Ok(entries)
}

fn workspace_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
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
}
