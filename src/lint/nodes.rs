use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, DocId, Severity};

#[derive(Default)]
pub(super) struct SemanticIndex {
    pub(super) manifest: Option<ManifestNode>,
    pub(super) qualifiers: BTreeMap<String, QualifierNode>,
    pub(super) variables: BTreeMap<String, VariableNode>,
    pub(super) external_values: BTreeMap<String, BTreeMap<String, ValueNode>>,
}

pub(super) struct ManifestNode {
    pub(super) doc: DocId,
    pub(super) location: DiagnosticLocation,
    pub(super) environments: WorkspaceEnvironmentCollection,
    pub(super) context_schema: Option<ContextSchemaNode>,
    pub(super) custom_rules: CustomRuleCollection,
}

pub(super) struct WorkspaceEnvironmentNode {
    pub(super) name: String,
    pub(super) location: DiagnosticLocation,
}

pub(super) enum WorkspaceEnvironmentCollection {
    Missing,
    Invalid {
        location: DiagnosticLocation,
    },
    Environments {
        location: DiagnosticLocation,
        values: Vec<WorkspaceEnvironmentNode>,
    },
}

pub(super) struct ContextSchemaNode {
    pub(super) location: DiagnosticLocation,
    pub(super) schema: ProjectField<String>,
    pub(super) invalid_shape: bool,
}

pub(super) struct QualifierNode {
    pub(super) doc: DocId,
    pub(super) id: String,
    pub(super) location: DiagnosticLocation,
    pub(super) schema_version: ProjectField<i64>,
    pub(super) description: Option<ProjectField<String>>,
    pub(super) predicates: PredicateCollection,
}

pub(super) struct PredicateNode {
    pub(super) index: usize,
    pub(super) location: DiagnosticLocation,
    pub(super) attribute: ProjectField<String>,
    pub(super) op: ProjectField<PredicateOp>,
    pub(super) value: Option<ValueShapeNode>,
    pub(super) salt: Option<ProjectField<String>>,
    pub(super) range: Option<BucketRangeNode>,
    pub(super) has_bucket_value: bool,
}

pub(super) enum PredicateCollection {
    Missing { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
    Predicates(Vec<PredicateNode>),
}

#[derive(Clone)]
pub(super) enum PredicateOp {
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

impl PredicateOp {
    pub(super) const COMPLETION_LABELS: &'static [&'static str] = &[
        "eq", "neq", "in", "not_in", "gt", "gte", "lt", "lte", "bucket",
    ];

    pub(super) fn from_str(op: &str) -> Self {
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

    pub(super) fn as_str(&self) -> &str {
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

pub(super) struct BucketRangeNode {
    pub(super) location: DiagnosticLocation,
    pub(super) is_array: bool,
    pub(super) len: usize,
    pub(super) start: Option<i64>,
    pub(super) end: Option<i64>,
}

pub(super) struct VariableNode {
    pub(super) doc: DocId,
    pub(super) id: String,
    pub(super) location: DiagnosticLocation,
    pub(super) schema_version: ProjectField<i64>,
    pub(super) description: Option<ProjectField<String>>,
    pub(super) type_source: TypeSourceNode,
    pub(super) values: ValuesNode,
    pub(super) environments: EnvironmentCollection,
}

pub(super) enum TypeSourceNode {
    Primitive(Spanned<String>),
    Schema(Spanned<String>),
    Missing { location: DiagnosticLocation },
    Conflict { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
}

pub(super) struct ValuesNode {
    pub(super) location: DiagnosticLocation,
    pub(super) inline_keys: BTreeSet<String>,
    pub(super) inline_values: BTreeMap<String, ValueNode>,
    pub(super) external_keys: BTreeSet<String>,
    pub(super) invalid_shape: bool,
}

pub(super) struct ValueNode {
    pub(super) key: String,
    pub(super) location: DiagnosticLocation,
    pub(super) value: JsonValue,
}

pub(super) enum CustomRuleCollection {
    Rules(Vec<CustomRuleDeclarationNode>),
    Invalid { location: DiagnosticLocation },
}

pub(super) struct CustomRuleDeclarationNode {
    pub(super) location: DiagnosticLocation,
    pub(super) id: ProjectField<String>,
    pub(super) title: ProjectField<String>,
    pub(super) help: ProjectField<String>,
    pub(super) severity: Option<ProjectField<Severity>>,
}

pub(super) enum EnvironmentCollection {
    Missing { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
    Environments(BTreeMap<String, EnvironmentBlockNode>),
}

pub(super) struct EnvironmentBlockNode {
    pub(super) environment: String,
    pub(super) location: DiagnosticLocation,
    pub(super) value: ProjectField<String>,
    pub(super) rules: RuleCollection,
}

pub(super) enum RuleCollection {
    Rules(Vec<VariableRuleNode>),
    Invalid { location: DiagnosticLocation },
}

pub(super) struct VariableRuleNode {
    pub(super) index: usize,
    pub(super) location: DiagnosticLocation,
    pub(super) qualifier: ProjectField<String>,
    pub(super) value: ProjectField<String>,
    pub(super) invalid_shape: bool,
}

#[derive(Clone)]
pub(super) struct Spanned<T> {
    pub(super) value: T,
    pub(super) location: DiagnosticLocation,
}

pub(super) enum ProjectField<T> {
    Present(Spanned<T>),
    Invalid { location: DiagnosticLocation },
    Missing { location: DiagnosticLocation },
}

impl<T> ProjectField<T> {
    pub(super) fn location(&self) -> DiagnosticLocation {
        match self {
            Self::Present(value) => value.location.clone(),
            Self::Invalid { location } | Self::Missing { location } => location.clone(),
        }
    }
}

pub(super) struct ValueShapeNode {
    pub(super) location: DiagnosticLocation,
    pub(super) shape: ValueShape,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueShape {
    String,
    Integer,
    Float,
    Boolean,
    Array,
    Table,
}

impl ValueShape {
    pub(super) fn as_str(self) -> &'static str {
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
