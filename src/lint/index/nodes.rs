use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value as JsonValue;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, DocId, LintStage, SemanticEntity,
    SemanticField, SemanticTarget,
};
use crate::expression::Expression;

use super::ids::{
    CatalogId, EnumId, EvaluationContextId, EvaluationContextSampleId, LayerId, PackagePath,
    ValueKey, VariableId,
};
use super::targets::RegisteredLintSelector;

pub(in crate::lint) struct ManifestNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) extends: PackageExtendsCollection,
    pub(in crate::lint) trace: Vec<TracePolicyNode>,
}

impl ManifestNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Manifest.into()
    }
}

/// One `[[trace]]` policy declared in the manifest. The `when` is a CEL boolean
/// that may, uniquely, read `env.resolving.*` (the entity being resolved).
pub(in crate::lint) struct TracePolicyNode {
    /// Position in the `[[trace]]` array, used to label the policy.
    pub(in crate::lint) index: usize,
    pub(in crate::lint) when: ProjectField<Expression>,
}

pub(in crate::lint) struct PackageExtendNode {
    pub(in crate::lint) source: String,
    pub(in crate::lint) location: DiagnosticLocation,
}

pub(in crate::lint) enum PackageExtendsCollection {
    Missing,
    Invalid {
        location: DiagnosticLocation,
    },
    Sources {
        location: DiagnosticLocation,
        values: Vec<PackageExtendNode>,
    },
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::lint) enum VariableTypeKind {
    Primitive(String),
    Catalog(String),
    Enum(String),
    List(Box<VariableTypeKind>),
}

impl VariableTypeKind {
    pub(in crate::lint) fn catalog_ids(&self) -> Vec<&str> {
        match self {
            Self::Primitive(_) | Self::Enum(_) => Vec::new(),
            Self::Catalog(catalog) => vec![catalog.as_str()],
            Self::List(item) => item.catalog_ids(),
        }
    }

    #[allow(dead_code)]
    pub(in crate::lint) fn enum_ids(&self) -> Vec<&str> {
        match self {
            Self::Primitive(_) | Self::Catalog(_) => Vec::new(),
            Self::Enum(id) => vec![id.as_str()],
            Self::List(item) => item.enum_ids(),
        }
    }

    pub(in crate::lint) fn list_catalog(&self) -> Option<&str> {
        match self {
            Self::List(item) => match item.as_ref() {
                Self::Catalog(catalog) => Some(catalog.as_str()),
                _ => None,
            },
            _ => None,
        }
    }
}

pub(in crate::lint) fn variable_type_kind(
    source: &TypeSourceNode,
) -> Option<Spanned<VariableTypeKind>> {
    match source {
        TypeSourceNode::Primitive(type_name) => {
            parse_variable_type(&type_name.value).map(|value| Spanned {
                value,
                location: type_name.location.clone(),
            })
        }
        TypeSourceNode::Catalog(catalog) => Some(Spanned {
            value: VariableTypeKind::Catalog(catalog.value.clone()),
            location: catalog.location.clone(),
        }),
        TypeSourceNode::Schema(_)
        | TypeSourceNode::Missing { .. }
        | TypeSourceNode::Conflict { .. }
        | TypeSourceNode::Invalid { .. } => None,
    }
}

fn parse_variable_type(value: &str) -> Option<VariableTypeKind> {
    let value = value.trim();
    if let Some(inner) = value
        .strip_prefix("list<")
        .and_then(|value| value.strip_suffix('>'))
    {
        return parse_variable_type(inner).map(|item| VariableTypeKind::List(Box::new(item)));
    }
    if let Some(catalog) = value.strip_prefix("catalog:") {
        if catalog.is_empty() {
            return None;
        }
        return Some(VariableTypeKind::Catalog(catalog.to_owned()));
    }
    if let Some(id) = value.strip_prefix("enum:") {
        if id.is_empty() {
            return None;
        }
        return Some(VariableTypeKind::Enum(id.to_owned()));
    }
    Some(VariableTypeKind::Primitive(value.to_owned()))
}

/// A named enum declaration under `model/enums/<id>.toml`: the contract half
/// (the member scalar type), with the members themselves under
/// `data/enums/<id>.toml`.
pub(in crate::lint) struct EnumNode {
    #[allow(dead_code)]
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: EnumId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) schema_version: ProjectField<i64>,
    #[allow(dead_code)]
    pub(in crate::lint) description: Option<ProjectField<String>>,
    pub(in crate::lint) member_type: ProjectField<String>,
}

impl EnumNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Enum {
            id: self.id.clone(),
        }
        .into()
    }

    #[allow(dead_code)]
    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::Enum {
                id: self.id.clone(),
            },
            field,
        )
    }
}

/// The data half of an enum: `members = [...]` under `data/enums/<id>.toml`.
pub(in crate::lint) struct EnumMembersNode {
    #[allow(dead_code)]
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: EnumId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) members: ProjectField<Vec<Spanned<JsonValue>>>,
}

impl EnumMembersNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Enum {
            id: self.id.clone(),
        }
        .into()
    }
}

/// The layering contract at `governance.toml`: one block per governed
/// entity, each a gate over operations plus scoped policies.
pub(in crate::lint) struct GovernanceNode {
    #[allow(dead_code)]
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) blocks: Vec<GovernanceBlockNode>,
    /// Top-level keys that are not one of the governed kinds.
    pub(in crate::lint) unknown_kinds: Vec<Spanned<String>>,
}

/// One `[<kind>.<id>]` block: which operations a layer below may perform on
/// this entity, and (for update/delete) where.
pub(in crate::lint) struct GovernanceBlockNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) kind: String,
    pub(in crate::lint) id: String,
    pub(in crate::lint) allowed_operations: Option<ProjectField<Vec<Spanned<String>>>>,
    pub(in crate::lint) denied_operations: Option<ProjectField<Vec<Spanned<String>>>>,
    pub(in crate::lint) update_policy: Option<GovernancePolicyNode>,
    pub(in crate::lint) delete_policy: Option<GovernancePolicyNode>,
    /// Keys under the block rototo does not recognize.
    pub(in crate::lint) unknown_keys: Vec<Spanned<String>>,
}

pub(in crate::lint) struct GovernancePolicyNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) allowed_entries: Option<ProjectField<Vec<Spanned<String>>>>,
    pub(in crate::lint) denied_entries: Option<ProjectField<Vec<Spanned<String>>>>,
    pub(in crate::lint) allowed_fields: Option<ProjectField<Vec<Spanned<String>>>>,
    pub(in crate::lint) denied_fields: Option<ProjectField<Vec<Spanned<String>>>>,
}

/// A layer under `layers/<id>.toml`: a diversion (`unit`, `buckets`) plus the
/// allocations that claim slices of it. The file stem is the layer id.
pub(in crate::lint) struct LayerNode {
    #[allow(dead_code)]
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: LayerId,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) schema_version: ProjectField<i64>,
    #[allow(dead_code)]
    pub(in crate::lint) description: Option<ProjectField<String>>,
    pub(in crate::lint) unit: ProjectField<Expression>,
    pub(in crate::lint) buckets: ProjectField<i64>,
    pub(in crate::lint) allocations: Vec<AllocationNode>,
    /// True when `allocation` exists but is not an array of tables.
    pub(in crate::lint) allocations_invalid: bool,
}

impl LayerNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::Layer {
            id: self.id.clone(),
        }
        .into()
    }
}

/// One `[[allocation]]` table inside a layer: a named claim on buckets,
/// divided into arms.
pub(in crate::lint) struct AllocationNode {
    pub(in crate::lint) index: usize,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) id: ProjectField<String>,
    pub(in crate::lint) status: Option<ProjectField<String>>,
    pub(in crate::lint) eligibility: Option<ProjectField<Expression>>,
    pub(in crate::lint) arms: Vec<ArmNode>,
    /// True when `arm` exists but is not an array of tables.
    pub(in crate::lint) arms_invalid: bool,
    pub(in crate::lint) invalid_shape: bool,
}

/// One `[[allocation.arm]]` table: a named slice of the allocation's buckets.
pub(in crate::lint) struct ArmNode {
    pub(in crate::lint) index: usize,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) name: ProjectField<String>,
    pub(in crate::lint) buckets: ProjectField<String>,
    pub(in crate::lint) invalid_shape: bool,
}

/// The `method = "allocation"` parameters on `[resolve]`: the allocation the
/// variable consumes and the per-arm value assignments.
pub(in crate::lint) struct AssignmentsNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) allocation: ProjectField<String>,
    pub(in crate::lint) assigns: Vec<AssignNode>,
    /// True when `assign` exists but is not an array of tables.
    pub(in crate::lint) assigns_invalid: bool,
}

/// One `[[resolve.assign]]` table: the value one arm assigns to the variable.
pub(in crate::lint) struct AssignNode {
    #[allow(dead_code)]
    pub(in crate::lint) index: usize,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) arm: ProjectField<String>,
    pub(in crate::lint) value: ProjectField<JsonValue>,
    pub(in crate::lint) invalid_shape: bool,
}

/// Parse an arm's `buckets` range: `"7"` (one bucket) or `"0-49"` (inclusive).
/// Returns `(start, end)` with `start <= end`, or `None` for anything else.
pub(crate) fn parse_arm_buckets(value: &str) -> Option<(u32, u32)> {
    let value = value.trim();
    match value.split_once('-') {
        Some((start, end)) => {
            let start = start.trim().parse().ok()?;
            let end = end.trim().parse().ok()?;
            (start <= end).then_some((start, end))
        }
        None => {
            let bucket = value.parse().ok()?;
            Some((bucket, bucket))
        }
    }
}

pub(in crate::lint) struct CatalogNode {
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) id: CatalogId,
    pub(in crate::lint) path: PackagePath,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) json: Option<JsonValue>,
    pub(in crate::lint) validator: Option<Arc<jsonschema::Validator>>,
    pub(in crate::lint) invalid_message: Option<String>,
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

pub(in crate::lint) struct EvaluationContextNode {
    pub(in crate::lint) id: EvaluationContextId,
    pub(in crate::lint) path: PackagePath,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) json: Option<JsonValue>,
    pub(in crate::lint) validator: Option<Arc<jsonschema::Validator>>,
    pub(in crate::lint) invalid_message: Option<String>,
}

impl EvaluationContextNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::EvaluationContext {
            id: self.id.clone(),
        }
        .into()
    }

    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::EvaluationContext {
                id: self.id.clone(),
            },
            field,
        )
    }
}

pub(in crate::lint) struct EvaluationContextSampleNode {
    pub(in crate::lint) evaluation_context_id: EvaluationContextId,
    pub(in crate::lint) key: EvaluationContextSampleId,
    pub(in crate::lint) path: PackagePath,
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) value: Option<JsonValue>,
}

impl EvaluationContextSampleNode {
    pub(in crate::lint) fn target(&self) -> SemanticTarget {
        SemanticEntity::EvaluationContextSample {
            evaluation_context: self.evaluation_context_id.clone(),
            key: self.key.clone(),
        }
        .into()
    }

    pub(in crate::lint) fn field_target(&self, field: SemanticField) -> SemanticTarget {
        SemanticTarget::field(
            SemanticEntity::EvaluationContextSample {
                evaluation_context: self.evaluation_context_id.clone(),
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

#[derive(Default)]
pub(in crate::lint) struct CustomLintRegistry {
    pub(in crate::lint) rules: BTreeMap<CustomRuleId, CustomRuleDefinitionNode>,
    pub(in crate::lint) files: BTreeMap<PackagePath, CustomLintFileNode>,
    pub(in crate::lint) registrations: Vec<CustomLintRegistration>,
}

#[derive(Clone)]
pub(in crate::lint) struct CustomRuleDefinitionNode {
    pub(in crate::lint) definition: CustomRuleDefinition,
}

#[derive(Clone)]
pub(in crate::lint) struct CustomLintFileNode {
    pub(in crate::lint) path: PackagePath,
    pub(in crate::lint) doc: DocId,
    pub(in crate::lint) location: DiagnosticLocation,
}

#[derive(Clone)]
pub(in crate::lint) struct CustomLintRegistration {
    pub(in crate::lint) file_path: PackagePath,
    pub(in crate::lint) rule: CustomRuleId,
    pub(in crate::lint) stage: LintStage,
    pub(in crate::lint) selector: RegisteredLintSelector,
    pub(in crate::lint) handler: String,
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
        /// The resolution method: `rules` (the default when absent), `query`,
        /// or `allocation`.
        method: Option<Box<Spanned<String>>>,
        default: Box<ProjectField<JsonValue>>,
        rules: RuleCollection,
        query: Option<Box<QueryNode>>,
        /// The `method = "allocation"` parameters, present when the
        /// `allocation` key or any `[[resolve.assign]]` appears.
        assignments: Option<Box<AssignmentsNode>>,
    },
}

/// The `method = "query"` parameters, flat on `[resolve]`: a CEL pipeline over
/// one catalog's entries.
pub(in crate::lint) struct QueryNode {
    pub(in crate::lint) location: DiagnosticLocation,
    pub(in crate::lint) from: ProjectField<String>,
    pub(in crate::lint) filter: Option<ProjectField<Expression>>,
    pub(in crate::lint) sort: Option<ProjectField<Expression>>,
    pub(in crate::lint) order: Option<ProjectField<String>>,
    pub(in crate::lint) limit: Option<ProjectField<i64>>,
}

impl ResolveNode {
    pub(in crate::lint) fn as_query(&self) -> Option<&QueryNode> {
        match self {
            Self::Resolve { query, .. } => query.as_deref(),
            _ => None,
        }
    }

    pub(in crate::lint) fn as_assignments(&self) -> Option<&AssignmentsNode> {
        match self {
            Self::Resolve { assignments, .. } => assignments.as_deref(),
            _ => None,
        }
    }

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
    pub(in crate::lint) when: Option<ProjectField<Expression>>,
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
