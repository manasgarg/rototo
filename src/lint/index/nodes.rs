use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value as JsonValue;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, DocId, LintStage, SemanticEntity,
    SemanticField, SemanticTarget,
};

use super::ids::{CatalogId, QualifierId, ValueKey, VariableId, WorkspacePath};
use super::targets::RegisteredLintSelector;

pub(in crate::lint) struct ManifestNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) extends: WorkspaceExtendsCollection,
}

impl ManifestNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Manifest.into()
    }

    #[allow(dead_code)]
    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(SemanticEntity::Manifest, field)
    }
}

pub(in crate::lint) struct WorkspaceExtendNode {
    pub(in crate::lint) source: String,
    pub(in crate::lint) location: DiagnosticLocation,
}

pub(in crate::lint) enum WorkspaceExtendsCollection {
    Missing,
    Invalid {
        location: DiagnosticLocation,
    },
    Sources {
        location: DiagnosticLocation,
        values: Vec<WorkspaceExtendNode>,
    },
}

pub(in crate::lint) struct QualifierNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: QualifierId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) schema_version: ProjectField<i64>,
    pub(in crate::lint) description: Option<ProjectField<String>>,
    pub(in crate::lint) predicates: PredicateCollection,
}

impl QualifierNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Qualifier {
            id: self.id.clone(),
        }
        .into()
    }

    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::Qualifier {
                id: self.id.clone(),
            },
            field,
        )
    }
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

impl PredicateNode {
    pub(in crate::lint) fn target(&self, qualifier_id: &str) -> SemanticTarget {
        SemanticEntity::Predicate {
            qualifier: qualifier_id.to_owned(),
            index: self.index,
        }
        .into()
    }

    pub(in crate::lint) fn field_target(
        &self,
        qualifier_id: &str,
        field: SemanticField,
    ) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::Predicate {
                qualifier: qualifier_id.to_owned(),
                index: self.index,
            },
            field,
        )
    }
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
    pub(in crate::lint) resolve: ResolveNode,
}

impl VariableNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Variable {
            id: self.id.clone(),
        }
        .into()
    }

    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::Variable {
                id: self.id.clone(),
            },
            field,
        )
    }
}

pub(in crate::lint) enum TypeSourceNode {
    Primitive(Spanned<String>),
    Catalog(Spanned<String>),
    Schema(Spanned<String>),
    Missing { location: DiagnosticLocation },
    Conflict { location: DiagnosticLocation },
    Invalid { location: DiagnosticLocation },
}

impl TypeSourceNode {
    pub(in crate::lint) fn location(&self) -> DiagnosticLocation {
        match self {
            Self::Primitive(type_name) => type_name.location.clone(),
            Self::Catalog(catalog) => catalog.location.clone(),
            Self::Schema(schema) => schema.location.clone(),
            Self::Missing { location }
            | Self::Conflict { location }
            | Self::Invalid { location } => location.clone(),
        }
    }
}

pub(in crate::lint) struct CatalogNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: CatalogId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) schema_version: ProjectField<i64>,
    pub(in crate::lint) description: Option<ProjectField<String>>,
    pub(in crate::lint) schema: ProjectField<String>,
}

impl CatalogNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Catalog {
            id: self.id.clone(),
        }
        .into()
    }

    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::Catalog {
                id: self.id.clone(),
            },
            field,
        )
    }
}

pub(in crate::lint) struct CatalogEntryNode {
    pub(in crate::lint) catalog_id: CatalogId,
    pub(in crate::lint) key: ValueKey,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) value: JsonValue,
}

impl CatalogEntryNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::CatalogEntry {
            catalog: self.catalog_id.clone(),
            key: self.key.clone(),
        }
        .into()
    }

    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::CatalogEntry {
                catalog: self.catalog_id.clone(),
                key: self.key.clone(),
            },
            field,
        )
    }
}

pub(in crate::lint) struct ValuesNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) inline_values: BTreeMap<ValueKey, ValueNode>,
    pub(in crate::lint) invalid_shape: bool,
}

pub(in crate::lint) struct ValueNode {
    pub(in crate::lint) variable_id: VariableId,
    pub(in crate::lint) key: ValueKey,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) value: JsonValue,
    pub(in crate::lint) origin: ValueOrigin,
}

impl ValueNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Value {
            variable: self.variable_id.clone(),
            key: self.key.clone(),
        }
        .into()
    }
}

pub(in crate::lint) enum ValueOrigin {
    Inline { variable_doc: DocId },
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

impl SchemaNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Schema {
            path: self.path.clone(),
        }
        .into()
    }

    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::Schema {
                path: self.path.clone(),
            },
            field,
        )
    }
}

#[derive(Default)]
pub(in crate::lint) struct CustomLintRegistry {
    pub(in crate::lint) rules: BTreeMap<CustomRuleId, CustomRuleDefinitionNode>,
    pub(in crate::lint) files: BTreeMap<WorkspacePath, CustomLintFileNode>,
    pub(in crate::lint) registrations: Vec<CustomLintRegistration>,
}

#[derive(Clone)]
pub(in crate::lint) struct CustomRuleDefinitionNode {
    pub(in crate::lint) definition: CustomRuleDefinition,
    #[allow(dead_code)]
    pub(in crate::lint) location: DiagnosticLocation,
}

#[derive(Clone)]
pub(in crate::lint) struct CustomLintFileNode {
    pub(in crate::lint) path: WorkspacePath,
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) location: DiagnosticLocation,
}

impl CustomLintFileNode {
    #[allow(dead_code)]
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::CustomLint {
            path: self.path.clone(),
        }
        .into()
    }
}

#[derive(Clone)]
pub(in crate::lint) struct CustomLintRegistration {
    pub(in crate::lint) file_path: WorkspacePath,
    pub(in crate::lint) rule: CustomRuleId,
    pub(in crate::lint) stage: LintStage,
    pub(in crate::lint) selector: RegisteredLintSelector,
    pub(in crate::lint) handler: String,
    #[allow(dead_code)]
    pub(in crate::lint) location: DiagnosticLocation,
}

pub(in crate::lint) enum ResolveNode {
    Missing {
        location: DiagnosticLocation,
    },
    Invalid {
        location: DiagnosticLocation,
    },
    Resolve {
        location: DiagnosticLocation,
        default: Box<ProjectField<JsonValue>>,
        rules: RuleCollection,
    },
}

impl ResolveNode {
    pub(in crate::lint) fn location(&self) -> DiagnosticLocation {
        match self {
            Self::Missing { location }
            | Self::Invalid { location }
            | Self::Resolve { location, .. } => location.clone(),
        }
    }
}

pub(in crate::lint) enum RuleCollection {
    Rules(Vec<VariableRuleNode>),
    Invalid { location: DiagnosticLocation },
}

pub(in crate::lint) struct VariableRuleNode {
    pub(in crate::lint) index: usize,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) qualifier: ProjectField<String>,
    pub(in crate::lint) value: ProjectField<JsonValue>,
    pub(in crate::lint) invalid_shape: bool,
}

impl VariableRuleNode {
    pub(in crate::lint) fn target(&self, variable_id: &str) -> SemanticTarget {
        SemanticEntity::Rule {
            variable: variable_id.to_owned(),
            index: self.index,
        }
        .into()
    }

    pub(in crate::lint) fn field_target(
        &self,
        variable_id: &str,
        field: SemanticField,
    ) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::Rule {
                variable: variable_id.to_owned(),
                index: self.index,
            },
            field,
        )
    }
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
    pub(in crate::lint) value: JsonValue,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_completion_labels_stay_in_sync_with_known_ops() {
        for label in PredicateOp::COMPLETION_LABELS {
            let op = PredicateOp::from_str(label);
            assert_eq!(op.as_str(), *label);
            assert!(!matches!(op, PredicateOp::Unknown(_)));
        }

        let known = [
            PredicateOp::Eq,
            PredicateOp::Neq,
            PredicateOp::In,
            PredicateOp::NotIn,
            PredicateOp::Gt,
            PredicateOp::Gte,
            PredicateOp::Lt,
            PredicateOp::Lte,
            PredicateOp::Bucket,
        ];
        for op in known {
            assert!(
                PredicateOp::COMPLETION_LABELS.contains(&op.as_str()),
                "missing completion label for {}",
                op.as_str()
            );
        }
    }
}
