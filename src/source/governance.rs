//! Compose-time enforcement of the `governance.toml` layering contract.
//!
//! When the projection built from the layers below carries a governance
//! contract, the next layer up is default-closed: every operation it performs
//! on a base-declared entity needs a grant. A projection with no
//! `governance.toml` is ungoverned and composes freely, which keeps plain
//! `extends` splitting (one team, several files) working unchanged.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Result, RototoError};

/// The three governed operations. Only `update` and `delete` carry a scope.
/// `override` and `constrain` are retired names: they must not parse and must
/// not come back for new operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Add,
    Update,
    Delete,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "add" => Some(Self::Add),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct GovernanceContract {
    blocks: BTreeMap<(String, String), Gate>,
}

#[derive(Debug, Default)]
struct Gate {
    allowed: Vec<Operation>,
    denied: Vec<Operation>,
    update_policy: Option<Policy>,
    delete_policy: Option<Policy>,
}

#[derive(Debug, Default)]
struct Policy {
    allowed_entries: Option<Vec<String>>,
    denied_entries: Option<Vec<String>>,
    allowed_fields: Option<Vec<String>>,
    denied_fields: Option<Vec<String>>,
}

/// Read the contract carried by a projection, if any. Parse and shape errors
/// are left for lint to report with locations; enforcement treats an
/// unreadable contract as ungoverned rather than failing the load twice.
pub(super) fn read_governance_contract(root: &Path) -> Option<GovernanceContract> {
    let text = std::fs::read_to_string(root.join("governance.toml")).ok()?;
    let value = text.parse::<toml::Value>().ok()?;
    Some(parse_contract(&value))
}

pub(super) fn parse_contract_value(value: &toml::Value) -> GovernanceContract {
    parse_contract(value)
}

fn parse_contract(value: &toml::Value) -> GovernanceContract {
    let mut contract = GovernanceContract::default();
    let Some(root) = value.as_table() else {
        return contract;
    };
    for (kind, blocks) in root {
        let Some(blocks) = blocks.as_table() else {
            continue;
        };
        for (id, block) in blocks {
            let Some(block) = block.as_table() else {
                continue;
            };
            contract.blocks.insert(
                (kind.clone(), id.clone()),
                Gate {
                    allowed: operations(block.get("allowed_operations")),
                    denied: operations(block.get("denied_operations")),
                    update_policy: block.get("update_policy").map(parse_policy),
                    delete_policy: block.get("delete_policy").map(parse_policy),
                },
            );
        }
    }
    contract
}

fn operations(value: Option<&toml::Value>) -> Vec<Operation> {
    string_list(value)
        .unwrap_or_default()
        .iter()
        .filter_map(|name| Operation::parse(name))
        .collect()
}

fn parse_policy(value: &toml::Value) -> Policy {
    Policy {
        allowed_entries: string_list(value.get("allowed_entries")),
        denied_entries: string_list(value.get("denied_entries")),
        allowed_fields: string_list(value.get("allowed_fields")),
        denied_fields: string_list(value.get("denied_fields")),
    }
}

fn string_list(value: Option<&toml::Value>) -> Option<Vec<String>> {
    Some(
        value?
            .as_array()?
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
    )
}

impl GovernanceContract {
    fn gate(&self, kind: &str, id: &str) -> Option<&Gate> {
        self.blocks.get(&(kind.to_owned(), id.to_owned()))
    }

    /// Whether the gate turns an operation on for a target. Default-closed:
    /// no block, or an operation absent from `allowed_operations`, is denied.
    /// Deny wins over allow.
    fn operation_allowed(&self, kind: &str, id: &str, operation: Operation) -> bool {
        let Some(gate) = self.gate(kind, id) else {
            return false;
        };
        gate.allowed.contains(&operation) && !gate.denied.contains(&operation)
    }

    /// Check one operation the layer above performs; the error tells the
    /// author which grant is missing.
    pub(super) fn check(
        &self,
        kind: &str,
        id: &str,
        operation: Operation,
        entry: Option<&str>,
        fields: &[String],
    ) -> Result<()> {
        if !self.operation_allowed(kind, id, operation) {
            return Err(RototoError::new(format!(
                "governance denies {} on {kind}.{id}: the base grants no such operation",
                operation.name()
            )));
        }
        let policy = match operation {
            Operation::Update => self
                .gate(kind, id)
                .and_then(|gate| gate.update_policy.as_ref()),
            Operation::Delete => self
                .gate(kind, id)
                .and_then(|gate| gate.delete_policy.as_ref()),
            _ => None,
        };
        let Some(policy) = policy else {
            return Ok(());
        };
        if let Some(entry) = entry
            && !passes(entry, &policy.allowed_entries, &policy.denied_entries)
        {
            return Err(RototoError::new(format!(
                "governance denies {} of entry {entry} on {kind}.{id}",
                operation.name()
            )));
        }
        for field in fields {
            if !passes(field, &policy.allowed_fields, &policy.denied_fields) {
                return Err(RototoError::new(format!(
                    "governance denies {} of field {field} on {kind}.{id}",
                    operation.name()
                )));
            }
        }
        Ok(())
    }

    /// The narrowing ceiling: every grant a layer declares for the layers
    /// below it must fit inside what this contract grants that layer. An
    /// excessive grant is rejected, not silently clamped, so the author sees
    /// it and either drops the rule or asks the layer above to widen.
    pub(super) fn check_ceiling(&self, lower: &GovernanceContract) -> Result<()> {
        for ((kind, id), gate) in &lower.blocks {
            for operation in &gate.allowed {
                if !self.operation_allowed(kind, id, *operation) {
                    return Err(RototoError::new(format!(
                        "governance grant exceeds the inherited ceiling: {kind}.{id} allows \
                         {} but the base does not grant it",
                        operation.name()
                    )));
                }
            }
            for (operation, policy) in [
                (Operation::Update, &gate.update_policy),
                (Operation::Delete, &gate.delete_policy),
            ] {
                let Some(policy) = policy else { continue };
                let Some(ceiling) = (match operation {
                    Operation::Update => self
                        .gate(kind, id)
                        .and_then(|gate| gate.update_policy.as_ref()),
                    _ => self
                        .gate(kind, id)
                        .and_then(|gate| gate.delete_policy.as_ref()),
                }) else {
                    // The layer above scoped nothing, so any scope below is a
                    // narrowing.
                    continue;
                };
                for (label, list, allowed, denied) in [
                    (
                        "entries",
                        &policy.allowed_entries,
                        &ceiling.allowed_entries,
                        &ceiling.denied_entries,
                    ),
                    (
                        "fields",
                        &policy.allowed_fields,
                        &ceiling.allowed_fields,
                        &ceiling.denied_fields,
                    ),
                ] {
                    for pattern in list.iter().flatten() {
                        if !pattern_within(pattern, allowed, denied) {
                            return Err(RototoError::new(format!(
                                "governance grant exceeds the inherited ceiling: {kind}.{id} \
                                 {} {label} allowlist includes {pattern}, which the base \
                                 does not grant",
                                operation.name()
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Allowlist restricts when present; denylist subtracts and wins absolutely.
fn passes(value: &str, allowed: &Option<Vec<String>>, denied: &Option<Vec<String>>) -> bool {
    if let Some(denied) = denied
        && denied.iter().any(|pattern| glob_match(pattern, value))
    {
        return false;
    }
    match allowed {
        Some(allowed) => allowed.iter().any(|pattern| glob_match(pattern, value)),
        None => true,
    }
}

/// Whether a lower layer's grant pattern fits inside the ceiling's lists.
/// Conservative: a literal must pass the ceiling like a value would; a glob
/// must be granted verbatim (or the ceiling must be unrestricted and deny
/// nothing), because glob-inside-glob containment is not worth solving.
fn pattern_within(
    pattern: &str,
    allowed: &Option<Vec<String>>,
    denied: &Option<Vec<String>>,
) -> bool {
    if !pattern.contains('*') {
        return passes(pattern, allowed, denied);
    }
    let allowed_ok = match allowed {
        None => true,
        Some(allowed) => allowed.iter().any(|ceiling| ceiling == pattern),
    };
    allowed_ok && denied.as_ref().is_none_or(|denied| denied.is_empty())
}

/// Minimal glob: `*` matches any run of characters; everything else is
/// literal. The same pattern language the governance lint uses.
fn glob_match(pattern: &str, value: &str) -> bool {
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else {
        return pattern == value;
    };
    if !value.starts_with(first) {
        return false;
    }
    let mut position = first.len();
    let mut last: Option<&str> = None;
    for part in parts {
        last = Some(part);
        if part.is_empty() {
            continue;
        }
        match value[position..].find(part) {
            Some(found) => position = position + found + part.len(),
            None => return false,
        }
    }
    match last {
        None => pattern == value,
        Some(last) => last.is_empty() || value.ends_with(last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(text: &str) -> GovernanceContract {
        parse_contract(&text.parse::<toml::Value>().unwrap())
    }

    #[test]
    fn default_closed_and_deny_wins() {
        let contract = contract(
            r#"
[catalog.plans]
allowed_operations = ["add", "delete"]
denied_operations = ["delete"]
"#,
        );
        assert!(contract.operation_allowed("catalog", "plans", Operation::Add));
        assert!(!contract.operation_allowed("catalog", "plans", Operation::Delete));
        assert!(!contract.operation_allowed("catalog", "plans", Operation::Update));
        assert!(!contract.operation_allowed("catalog", "offers", Operation::Add));
    }

    #[test]
    fn policies_scope_entries_and_fields() {
        let contract = contract(
            r#"
[catalog.plans]
allowed_operations = ["update", "delete"]

[catalog.plans.update_policy]
allowed_fields = ["monthly_price", "limits"]
denied_entries = ["free"]

[catalog.plans.delete_policy]
allowed_entries = ["*"]
denied_entries = ["free"]
"#,
        );
        assert!(
            contract
                .check(
                    "catalog",
                    "plans",
                    Operation::Update,
                    Some("growth"),
                    &["monthly_price".to_owned()],
                )
                .is_ok()
        );
        assert!(
            contract
                .check(
                    "catalog",
                    "plans",
                    Operation::Update,
                    Some("growth"),
                    &["name".to_owned()],
                )
                .is_err()
        );
        assert!(
            contract
                .check("catalog", "plans", Operation::Update, Some("free"), &[])
                .is_err()
        );
        assert!(
            contract
                .check("catalog", "plans", Operation::Delete, Some("growth"), &[])
                .is_ok()
        );
        assert!(
            contract
                .check("catalog", "plans", Operation::Delete, Some("free"), &[])
                .is_err()
        );
    }

    #[test]
    fn ceiling_rejects_wider_grants_below() {
        let above = contract(
            r#"
[catalog.plans]
allowed_operations = ["update"]

[catalog.plans.update_policy]
allowed_fields = ["monthly_price"]
"#,
        );
        let narrower = contract(
            r#"
[catalog.plans]
allowed_operations = ["update"]

[catalog.plans.update_policy]
allowed_fields = ["monthly_price"]
denied_entries = ["free"]
"#,
        );
        assert!(above.check_ceiling(&narrower).is_ok());

        let wider_operation = contract(
            r#"
[catalog.plans]
allowed_operations = ["update", "delete"]
"#,
        );
        assert!(above.check_ceiling(&wider_operation).is_err());

        let wider_fields = contract(
            r#"
[catalog.plans]
allowed_operations = ["update"]

[catalog.plans.update_policy]
allowed_fields = ["name"]
"#,
        );
        assert!(above.check_ceiling(&wider_fields).is_err());
    }
}
