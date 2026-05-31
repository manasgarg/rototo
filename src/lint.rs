use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml_edit::{ImDocument, Item, Table, TableLike, Value as EditValue};

use crate::diagnostics::{
    DiagnosticLocation, DocId, EntityId, LintDiagnostic, LintStage, RototoRuleId, SourcePosition,
    SourceRange,
};
use crate::error::{Result, RototoError};
use crate::model::{QualifierLint, SourceDocumentSummary, SourceKind, VariableLint, WorkspaceLint};
use crate::workspace::workspace_environments;

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";

pub async fn lint_workspace(workspace_root: &Path) -> Result<WorkspaceLint> {
    LintEngine::new()
        .lint_workspace(LintInput {
            root: workspace_root.to_path_buf(),
        })
        .await
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

struct LintInput {
    root: PathBuf,
}

struct LintEngine;

impl LintEngine {
    fn new() -> Self {
        Self
    }

    async fn lint_workspace(&self, input: LintInput) -> Result<WorkspaceLint> {
        let mut ctx = LintContext::new(input);

        self.run_discover(&mut ctx).await?;
        self.run_parse(&mut ctx);
        self.build_projection(&mut ctx);
        self.run_project(&mut ctx);
        self.run_reference(&mut ctx);
        self.run_value(&mut ctx);

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
                DocumentKind::Schema => {}
            }
        }
    }

    fn run_project(&self, ctx: &mut LintContext) {
        lint_manifest_shape(ctx);
        lint_qualifier_shapes(ctx);
        lint_variable_shapes(ctx);
    }

    fn run_reference(&self, ctx: &mut LintContext) {
        lint_qualifier_references(ctx);
        lint_variable_references(ctx);
    }

    fn run_value(&self, ctx: &mut LintContext) {
        lint_schema_documents(ctx);
        lint_variable_values(ctx);
    }
}

struct LintContext {
    input: LintInput,
    source: SourceStore,
    syntax: SyntaxIndex,
    index: SemanticIndex,
    diagnostics: Vec<LintDiagnostic>,
}

impl LintContext {
    fn new(input: LintInput) -> Self {
        Self {
            source: SourceStore::new(input.root.clone()),
            input,
            syntax: SyntaxIndex::default(),
            index: SemanticIndex::default(),
            diagnostics: Vec::new(),
        }
    }

    fn finish(mut self) -> WorkspaceLint {
        sort_diagnostics(&mut self.diagnostics);
        let documents = self.source.document_summaries();
        WorkspaceLint {
            root: self.source.root,
            documents,
            diagnostics: self.diagnostics,
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
}

struct QualifierNode {
    id: String,
    schema_version: ProjectField<i64>,
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
    id: String,
    location: DiagnosticLocation,
    schema_version: ProjectField<i64>,
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

enum EnvironmentCollection {
    Missing { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
    Environments(BTreeMap<String, EnvironmentBlockNode>),
}

struct EnvironmentBlockNode {
    environment: String,
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

fn project_manifest(document: &SourceDocument, _toml: &ParsedToml) -> ManifestNode {
    ManifestNode {
        doc: document.id,
        location: document.document_location(),
    }
}

fn project_qualifier(document: &SourceDocument, toml: &ParsedToml, id: &str) -> QualifierNode {
    let root = toml.edit.as_table();
    let location = document.document_location();
    let schema_version = integer_field(document, root, "schema_version", location.clone());
    let predicates = project_predicates(document, root);

    QualifierNode {
        id: id.to_owned(),
        schema_version,
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
    let type_source = project_type_source(document, root, location.clone());
    let values = project_values(document, toml, root, id, source);
    let environments = project_environments(document, root, id);

    VariableNode {
        id: id.to_owned(),
        location,
        schema_version,
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
            value: ProjectField::Invalid {
                location: location.clone(),
            },
            rules: RuleCollection::Rules(Vec::new()),
        };
    };

    EnvironmentBlockNode {
        environment: environment.to_owned(),
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
        lint_values_shape(diagnostics, variable);
        lint_environment_shapes(diagnostics, variable);
    }
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

fn lint_values_shape(diagnostics: &mut Vec<LintDiagnostic>, variable: &VariableNode) {
    if variable.values.invalid_shape {
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
    documents: BTreeMap<DocId, SourceDocument>,
    by_path: BTreeMap<String, DocId>,
}

impl SourceStore {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
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

    async fn add_disk_document(&mut self, relative_path: PathBuf, kind: DocumentKind) -> DocId {
        let path = workspace_path(&relative_path);
        if let Some(doc) = self.by_path.get(&path).copied() {
            return doc;
        }

        let id = DocId(self.documents.len() as u32);
        let absolute_path = self.root.join(&relative_path);
        let (text, read_error) = match tokio::fs::read_to_string(&absolute_path).await {
            Ok(text) => (text, None),
            Err(err) => (String::new(), Some(err.to_string())),
        };
        let document = SourceDocument {
            id,
            path: path.clone(),
            uri: file_uri(&absolute_path),
            version: None,
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
}

impl DocumentKind {
    fn summary_kind(&self) -> SourceKind {
        match self {
            Self::Manifest => SourceKind::Manifest,
            Self::Qualifier { .. } => SourceKind::Qualifier,
            Self::Variable { .. } => SourceKind::Variable,
            Self::ExternalValue { .. } => SourceKind::ExternalValue,
            Self::Schema => SourceKind::Schema,
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
