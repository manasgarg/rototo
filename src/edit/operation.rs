use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// One semantic operation. The serde shape is the wire shape editors submit:
/// `{"op": "set_default", "variable": "checkout_redesign", "value": true}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditOperation {
    CreateVariable {
        id: String,
        #[serde(rename = "type")]
        variable_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        default: JsonValue,
    },
    CreateCatalog {
        id: String,
        schema: JsonValue,
    },
    CreateEntry {
        catalog: String,
        key: String,
        fields: JsonValue,
    },
    CreateList {
        id: String,
        #[serde(rename = "type")]
        member_type: String,
        members: Vec<JsonValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    CreateContext {
        id: String,
        schema: JsonValue,
    },
    CreateLayer {
        id: String,
        unit: String,
        buckets: i64,
    },
    CreateSample {
        context: String,
        key: String,
        content: JsonValue,
    },
    Delete {
        target: String,
    },
    /// Sets or clears a description. The target is `variable=<id>` or
    /// `list=<id>`; absent text clears it.
    SetDescription {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    /// Applies structurally; lint judges the fallout on the post-edit tree.
    SetType {
        variable: String,
        #[serde(rename = "type")]
        variable_type: String,
    },
    SetDefault {
        variable: String,
        value: JsonValue,
    },
    /// Rules are positional (first match wins); the default position is the
    /// end.
    AddRule {
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<usize>,
        when: String,
        value: JsonValue,
    },
    /// Partial update of one rule; at least one of `when` and `value`.
    UpdateRule {
        variable: String,
        index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<JsonValue>,
    },
    RemoveRule {
        variable: String,
        index: usize,
    },
    /// `to` is the rule's final position.
    MoveRule {
        variable: String,
        from: usize,
        to: usize,
    },
    /// Switches the resolve to `method = "query"` and writes the query
    /// fields whole. Any rules are removed: a query resolve has no rules
    /// to run. The default, when present, stays as the empty-result
    /// fallback.
    SetQuery {
        variable: String,
        from: String,
        filter: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        order: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<i64>,
    },
    /// Returns a query resolve to rules, keeping the default.
    ClearQuery {
        variable: String,
    },
    /// The target is an entry address with a pointer:
    /// `catalog=plans:entry=pro#/limits/api_calls`. Missing intermediate
    /// objects are created; grant checks quantize the pointer to its
    /// top-level field.
    SetField {
        target: String,
        value: JsonValue,
    },
    /// Removes an optional field; the target must exist.
    UnsetField {
        target: String,
    },
    AddMember {
        #[serde(rename = "list")]
        list_id: String,
        value: JsonValue,
    },
    RemoveMember {
        #[serde(rename = "list")]
        list_id: String,
        value: JsonValue,
    },
    /// Arms and their bucket ranges are defined together.
    AddAllocation {
        layer: String,
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        eligibility: Option<String>,
        arms: Vec<AllocationArmInput>,
    },
    RemoveAllocation {
        layer: String,
        id: String,
    },
    SetAllocationStatus {
        layer: String,
        id: String,
        status: String,
    },
    /// Absent `when` clears the eligibility expression.
    SetAllocationEligibility {
        layer: String,
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<String>,
    },
    /// The rollout dial: growing an arm from 20% to 50% is this one
    /// operation.
    SetArmBuckets {
        layer: String,
        allocation: String,
        arm: String,
        buckets: String,
    },
    /// Whole-document replace; samples are small JSON documents and
    /// field-level operations are not worth their complexity.
    ReplaceSample {
        context: String,
        key: String,
        content: JsonValue,
    },
}

impl EditOperation {
    /// The operation's wire name, as used in change records.
    pub fn name(&self) -> &'static str {
        match self {
            Self::CreateVariable { .. } => "create_variable",
            Self::CreateCatalog { .. } => "create_catalog",
            Self::CreateEntry { .. } => "create_entry",
            Self::CreateList { .. } => "create_list",
            Self::CreateContext { .. } => "create_context",
            Self::CreateLayer { .. } => "create_layer",
            Self::CreateSample { .. } => "create_sample",
            Self::Delete { .. } => "delete",
            Self::SetDescription { .. } => "set_description",
            Self::SetType { .. } => "set_type",
            Self::SetDefault { .. } => "set_default",
            Self::AddRule { .. } => "add_rule",
            Self::UpdateRule { .. } => "update_rule",
            Self::RemoveRule { .. } => "remove_rule",
            Self::MoveRule { .. } => "move_rule",
            Self::SetQuery { .. } => "set_query",
            Self::ClearQuery { .. } => "clear_query",
            Self::SetField { .. } => "set_field",
            Self::UnsetField { .. } => "unset_field",
            Self::AddMember { .. } => "add_member",
            Self::RemoveMember { .. } => "remove_member",
            Self::AddAllocation { .. } => "add_allocation",
            Self::RemoveAllocation { .. } => "remove_allocation",
            Self::SetAllocationStatus { .. } => "set_allocation_status",
            Self::SetAllocationEligibility { .. } => "set_allocation_eligibility",
            Self::SetArmBuckets { .. } => "set_arm_buckets",
            Self::ReplaceSample { .. } => "replace_sample",
        }
    }
}

/// One arm of a new allocation: a name and its bucket range (`"0-499"`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationArmInput {
    pub name: String,
    pub buckets: String,
}

/// How the engine should compile operations.
#[derive(Clone, Debug, Default)]
pub struct EditOptions {
    /// Canonical entity addresses (`variable=<id>`, `catalog=<id>`, ...) the
    /// package inherits from a base rather than owning. V1 refuses to edit
    /// them; ownership-aware compilation to overlay markers lands behind
    /// this same parameter.
    pub inherited: BTreeSet<String>,
}

/// What an apply produced: the file changes and the intent behind them.
#[derive(Clone, Debug, Serialize)]
pub struct EditOutcome {
    pub plan: EditPlan,
    pub records: Vec<ChangeRecord>,
}

/// The file changes, ready for one commit (or one local write).
#[derive(Clone, Debug, Default, Serialize)]
pub struct EditPlan {
    pub writes: Vec<PlannedWrite>,
    /// Package-relative paths to remove.
    pub deletes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlannedWrite {
    /// Package-relative path with forward slashes.
    pub path: String,
    pub content: String,
}

/// The intent of one applied operation: the operation name, the canonical
/// address of what changed, and the value before and after. These feed
/// field-level grant checks, PR summaries, and the change-set diary without
/// diff archaeology.
#[derive(Clone, Debug, Serialize)]
pub struct ChangeRecord {
    pub operation: &'static str,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<JsonValue>,
}
