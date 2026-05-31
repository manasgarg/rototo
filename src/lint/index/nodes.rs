use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::Value as JsonValue;

use crate::diagnostics::{DiagnosticLocation, DocId, Severity};

use super::ids::{EnvironmentId, QualifierId, ValueKey, VariableId, WorkspacePath};

pub(in crate::lint) struct ManifestNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) environments: WorkspaceEnvironmentCollection,
    pub(in crate::lint) context_schema: Option<ContextSchemaNode>,
    pub(in crate::lint) custom_rules: CustomRuleCollection,
}

pub(in crate::lint) struct WorkspaceEnvironmentNode {
    pub(in crate::lint) name: EnvironmentId,
    pub(in crate::lint) location: DiagnosticLocation,
}

pub(in crate::lint) enum WorkspaceEnvironmentCollection {
    Missing,
    Invalid {
        location: DiagnosticLocation,
    },
    Environments {
        location: DiagnosticLocation,
        values: Vec<WorkspaceEnvironmentNode>,
    },
}

pub(in crate::lint) struct ContextSchemaNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) schema: ProjectField<String>,
    pub(in crate::lint) invalid_shape: bool,
}

pub(in crate::lint) struct QualifierNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: QualifierId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) schema_version: ProjectField<i64>,
    pub(in crate::lint) description: Option<ProjectField<String>>,
    pub(in crate::lint) predicates: PredicateCollection,
}

pub(in crate::lint) struct PredicateNode {
    pub(in crate::lint) index: usize,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) attribute: ProjectField<String>,
    pub(in crate::lint) op: ProjectField<PredicateOp>,
    pub(in crate::lint) value: Option<ValueShapeNode>,
    pub(in crate::lint) salt: Option<ProjectField<String>>,
    pub(in crate::lint) range: Option<BucketRangeNode>,
    pub(in crate::lint) has_bucket_value: bool,
}

pub(in crate::lint) enum PredicateCollection {
    Missing { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
    Predicates(Vec<PredicateNode>),
}

#[derive(Clone)]
pub(in crate::lint) enum PredicateOp {
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
    pub(in crate::lint) const COMPLETION_LABELS: &'static [&'static str] = &[
        "eq", "neq", "in", "not_in", "gt", "gte", "lt", "lte", "bucket",
    ];

    pub(in crate::lint) fn from_str(op: &str) -> Self {
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

    pub(in crate::lint) fn as_str(&self) -> &str {
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

pub(in crate::lint) struct BucketRangeNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) is_array: bool,
    pub(in crate::lint) len: usize,
    pub(in crate::lint) start: Option<i64>,
    pub(in crate::lint) end: Option<i64>,
}

pub(in crate::lint) struct VariableNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: VariableId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) schema_version: ProjectField<i64>,
    pub(in crate::lint) description: Option<ProjectField<String>>,
    pub(in crate::lint) type_source: TypeSourceNode,
    pub(in crate::lint) values: ValuesNode,
    pub(in crate::lint) environments: EnvironmentCollection,
}

pub(in crate::lint) enum TypeSourceNode {
    Primitive(Spanned<String>),
    Schema(Spanned<String>),
    Missing { location: DiagnosticLocation },
    Conflict { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
}

impl TypeSourceNode {
    pub(in crate::lint) fn location(&self) -> DiagnosticLocation {
        match self {
            Self::Primitive(type_name) => type_name.location.clone(),
            Self::Schema(schema) => schema.location.clone(),
            Self::Missing { location }
            | Self::Conflict { location }
            | Self::Invalid { location } => location.clone(),
        }
    }
}

pub(in crate::lint) struct ValuesNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) inline_keys: BTreeSet<ValueKey>,
    pub(in crate::lint) inline_values: BTreeMap<ValueKey, ValueNode>,
    pub(in crate::lint) external_keys: BTreeSet<ValueKey>,
    pub(in crate::lint) invalid_shape: bool,
}

pub(in crate::lint) struct ValueNode {
    pub(in crate::lint) key: ValueKey,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) value: JsonValue,
}

pub(in crate::lint) struct SchemaNode {
    #[allow(dead_code)]
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) path: WorkspacePath,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) json: Option<JsonValue>,
    pub(in crate::lint) validator: Option<Arc<jsonschema::Validator>>,
    pub(in crate::lint) invalid_message: Option<String>,
}

pub(in crate::lint) enum CustomRuleCollection {
    Rules(Vec<CustomRuleDeclarationNode>),
    Invalid { location: DiagnosticLocation },
}

pub(in crate::lint) struct CustomRuleDeclarationNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) id: ProjectField<String>,
    pub(in crate::lint) title: ProjectField<String>,
    pub(in crate::lint) help: ProjectField<String>,
    pub(in crate::lint) severity: Option<ProjectField<Severity>>,
}

pub(in crate::lint) enum EnvironmentCollection {
    Missing { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
    Environments(BTreeMap<EnvironmentId, EnvironmentBlockNode>),
}

pub(in crate::lint) struct EnvironmentBlockNode {
    pub(in crate::lint) environment: EnvironmentId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) value: ProjectField<String>,
    pub(in crate::lint) rules: RuleCollection,
}

pub(in crate::lint) enum RuleCollection {
    Rules(Vec<VariableRuleNode>),
    Invalid { location: DiagnosticLocation },
}

pub(in crate::lint) struct VariableRuleNode {
    pub(in crate::lint) index: usize,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) qualifier: ProjectField<String>,
    pub(in crate::lint) value: ProjectField<String>,
    pub(in crate::lint) invalid_shape: bool,
}

#[derive(Clone)]
pub(in crate::lint) struct Spanned<T> {
    pub(in crate::lint) value: T,
    pub(in crate::lint) location: DiagnosticLocation,
}

pub(in crate::lint) enum ProjectField<T> {
    Present(Spanned<T>),
    Invalid { location: DiagnosticLocation },
    Missing { location: DiagnosticLocation },
}

impl<T> ProjectField<T> {
    pub(in crate::lint) fn location(&self) -> DiagnosticLocation {
        match self {
            Self::Present(value) => value.location.clone(),
            Self::Invalid { location } | Self::Missing { location } => location.clone(),
        }
    }
}

pub(in crate::lint) struct ValueShapeNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) shape: ValueShape,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::lint) enum ValueShape {
    String,
    Integer,
    Float,
    Boolean,
    Array,
    Table,
}

impl ValueShape {
    pub(in crate::lint) fn as_str(self) -> &'static str {
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
