use std::collections::BTreeMap;

use toml_edit::DocumentMut;

use crate::address::{Address, EntityClass, StepId};
use crate::error::{Result, RototoError};

use super::operation::{
    ChangeRecord, EditOperation, EditOptions, EditOutcome, EditPlan, PlannedWrite,
};
use super::tree::EditTree;
use super::value::parse_toml_document;
use super::{create, entry, enums, layer, paths, variable};

pub(super) fn apply(
    tree: &EditTree,
    operations: &[EditOperation],
    options: &EditOptions,
) -> Result<EditOutcome> {
    let mut work = WorkingTree::new(tree);
    let mut records = Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        let applied = apply_operation(&mut work, operation, options).map_err(|err| {
            RototoError::new(format!("operation {index} ({}): {err}", operation.name()))
        })?;
        records.extend(applied);
    }
    Ok(EditOutcome {
        plan: work.into_plan(),
        records,
    })
}

fn apply_operation(
    work: &mut WorkingTree<'_>,
    operation: &EditOperation,
    options: &EditOptions,
) -> Result<Vec<ChangeRecord>> {
    match operation {
        EditOperation::CreateVariable {
            id,
            variable_type,
            description,
            default,
        } => {
            ensure_owned(options, &[format!("variable={id}")])?;
            Ok(vec![create::create_variable(
                work,
                id,
                variable_type,
                description.as_deref(),
                default,
            )?])
        }
        EditOperation::CreateCatalog { id, schema } => {
            ensure_owned(options, &[format!("catalog={id}")])?;
            Ok(vec![create::create_catalog(work, id, schema)?])
        }
        EditOperation::CreateEntry {
            catalog,
            key,
            fields,
        } => {
            ensure_owned(
                options,
                &[
                    format!("catalog={catalog}"),
                    format!("catalog={catalog}:entry={key}"),
                ],
            )?;
            Ok(vec![create::create_entry(work, catalog, key, fields)?])
        }
        EditOperation::CreateEnum {
            id,
            member_type,
            members,
            description,
        } => {
            ensure_owned(options, &[format!("enum={id}")])?;
            Ok(vec![create::create_enum(
                work,
                id,
                member_type,
                members,
                description.as_deref(),
            )?])
        }
        EditOperation::CreateContext { id, schema } => {
            ensure_owned(options, &[format!("evaluation-context={id}")])?;
            create::create_context(work, id, schema)
        }
        EditOperation::CreateLayer { id, unit, buckets } => {
            ensure_owned(options, &[format!("layer={id}")])?;
            Ok(vec![create::create_layer(work, id, unit, *buckets)?])
        }
        EditOperation::CreateSample {
            context,
            key,
            content,
        } => {
            ensure_owned(
                options,
                &[
                    format!("evaluation-context={context}"),
                    format!("evaluation-context={context}:sample={key}"),
                ],
            )?;
            Ok(vec![create::create_sample(work, context, key, content)?])
        }
        EditOperation::Delete { target } => {
            let address = Address::parse(target)?;
            ensure_owned(options, &delete_ownership_addresses(&address))?;
            create::delete(work, &address)
        }
        EditOperation::SetDescription { target, text } => {
            let (path, canonical) = description_target(target)?;
            ensure_owned(options, std::slice::from_ref(&canonical))?;
            Ok(vec![variable::set_description(
                work,
                &path,
                &canonical,
                text.as_deref(),
            )?])
        }
        EditOperation::SetType {
            variable,
            variable_type,
        } => {
            ensure_owned(options, &[format!("variable={variable}")])?;
            Ok(vec![variable::set_type(work, variable, variable_type)?])
        }
        EditOperation::SetDefault { variable, value } => {
            ensure_owned(options, &[format!("variable={variable}")])?;
            Ok(vec![variable::set_default(work, variable, value)?])
        }
        EditOperation::AddRule {
            variable,
            position,
            when,
            value,
        } => {
            ensure_owned(options, &[format!("variable={variable}")])?;
            Ok(vec![variable::add_rule(
                work, variable, *position, when, value,
            )?])
        }
        EditOperation::UpdateRule {
            variable,
            index,
            when,
            value,
        } => {
            ensure_owned(options, &[format!("variable={variable}")])?;
            Ok(vec![variable::update_rule(
                work,
                variable,
                *index,
                when.as_deref(),
                value.as_ref(),
            )?])
        }
        EditOperation::RemoveRule { variable, index } => {
            ensure_owned(options, &[format!("variable={variable}")])?;
            Ok(vec![variable::remove_rule(work, variable, *index)?])
        }
        EditOperation::MoveRule { variable, from, to } => {
            ensure_owned(options, &[format!("variable={variable}")])?;
            Ok(vec![variable::move_rule(work, variable, *from, *to)?])
        }
        EditOperation::SetField { target, value } => {
            let target = entry::parse_entry_target(target)?;
            ensure_owned(options, &target.ownership_addresses())?;
            Ok(vec![entry::set_field(work, &target, value)?])
        }
        EditOperation::UnsetField { target } => {
            let target = entry::parse_entry_target(target)?;
            ensure_owned(options, &target.ownership_addresses())?;
            Ok(vec![entry::unset_field(work, &target)?])
        }
        EditOperation::AddMember { enum_id, value } => {
            ensure_owned(options, &[format!("enum={enum_id}")])?;
            Ok(vec![enums::add_member(work, enum_id, value)?])
        }
        EditOperation::RemoveMember { enum_id, value } => {
            ensure_owned(options, &[format!("enum={enum_id}")])?;
            Ok(vec![enums::remove_member(work, enum_id, value)?])
        }
        EditOperation::AddAllocation {
            layer: layer_id,
            id,
            status,
            eligibility,
            arms,
        } => {
            ensure_owned(options, &[format!("layer={layer_id}")])?;
            Ok(vec![layer::add_allocation(
                work,
                layer_id,
                id,
                status.as_deref(),
                eligibility.as_deref(),
                arms,
            )?])
        }
        EditOperation::RemoveAllocation {
            layer: layer_id,
            id,
        } => {
            ensure_owned(options, &[format!("layer={layer_id}")])?;
            Ok(vec![layer::remove_allocation(work, layer_id, id)?])
        }
        EditOperation::SetAllocationStatus {
            layer: layer_id,
            id,
            status,
        } => {
            ensure_owned(options, &[format!("layer={layer_id}")])?;
            Ok(vec![layer::set_allocation_status(
                work, layer_id, id, status,
            )?])
        }
        EditOperation::SetAllocationEligibility {
            layer: layer_id,
            id,
            when,
        } => {
            ensure_owned(options, &[format!("layer={layer_id}")])?;
            Ok(vec![layer::set_allocation_eligibility(
                work,
                layer_id,
                id,
                when.as_deref(),
            )?])
        }
        EditOperation::SetArmBuckets {
            layer: layer_id,
            allocation,
            arm,
            buckets,
        } => {
            ensure_owned(options, &[format!("layer={layer_id}")])?;
            Ok(vec![layer::set_arm_buckets(
                work, layer_id, allocation, arm, buckets,
            )?])
        }
        EditOperation::ReplaceSample {
            context,
            key,
            content,
        } => {
            ensure_owned(
                options,
                &[
                    format!("evaluation-context={context}"),
                    format!("evaluation-context={context}:sample={key}"),
                ],
            )?;
            Ok(vec![create::replace_sample(work, context, key, content)?])
        }
    }
}

/// V1 compiles against owned entities only. The check is here so overlay
/// compilation can replace it without touching the per-entity editors.
fn ensure_owned(options: &EditOptions, addresses: &[String]) -> Result<()> {
    for address in addresses {
        if options.inherited.contains(address) {
            return Err(RototoError::new(format!(
                "`{address}` is inherited from a base package; \
                 editing inherited entities is not supported yet"
            )));
        }
    }
    Ok(())
}

fn delete_ownership_addresses(address: &Address) -> Vec<String> {
    let mut addresses = Vec::new();
    let mut prefix = String::new();
    for step in address.steps() {
        if !prefix.is_empty() {
            prefix.push(':');
        }
        prefix.push_str(step.class.as_str());
        prefix.push('=');
        if let StepId::Entity(id) = &step.id {
            prefix.push_str(id);
        }
        addresses.push(prefix.clone());
    }
    addresses
}

fn description_target(target: &str) -> Result<(String, String)> {
    let address = Address::parse(target)?;
    if address.pointer().is_some() {
        return Err(RototoError::new(
            "set_description takes an entity target without a # pointer",
        ));
    }
    let step = address.last_step();
    match (address.steps().len(), step.class, &step.id) {
        (1, EntityClass::Variable, StepId::Entity(id)) => {
            Ok((paths::variable_path(id), format!("variable={id}")))
        }
        (1, EntityClass::Enum, StepId::Entity(id)) => {
            Ok((paths::enum_path(id), format!("enum={id}")))
        }
        _ => Err(RototoError::new(
            "set_description takes a `variable=<id>` or `enum=<id>` target",
        )),
    }
}

/// The tree with this apply's edits layered on top; operations later in the
/// list see what earlier ones wrote.
pub(super) struct WorkingTree<'a> {
    base: &'a EditTree,
    edits: BTreeMap<String, Option<String>>,
}

impl<'a> WorkingTree<'a> {
    fn new(base: &'a EditTree) -> Self {
        Self {
            base,
            edits: BTreeMap::new(),
        }
    }

    pub(super) fn exists(&self, path: &str) -> bool {
        match self.edits.get(path) {
            Some(edit) => edit.is_some(),
            None => self.base.contains(path),
        }
    }

    pub(super) fn content(&self, path: &str) -> Option<&str> {
        match self.edits.get(path) {
            Some(edit) => edit.as_deref(),
            None => self.base.content(path),
        }
    }

    pub(super) fn write(&mut self, path: String, content: String) {
        self.edits.insert(path, Some(content));
    }

    pub(super) fn delete(&mut self, path: &str) {
        self.edits.insert(path.to_owned(), None);
    }

    /// Every live file path under a `dir/` prefix, edits included.
    pub(super) fn paths_under(&self, prefix: &str) -> Vec<String> {
        let mut paths: Vec<String> = self
            .base
            .paths()
            .filter(|path| path.starts_with(prefix))
            .map(str::to_owned)
            .collect();
        for (path, edit) in &self.edits {
            match edit {
                Some(_) if path.starts_with(prefix) && !paths.contains(path) => {
                    paths.push(path.clone());
                }
                None => paths.retain(|kept| kept != path),
                _ => {}
            }
        }
        paths.sort();
        paths
    }

    /// Parses an existing TOML document, with a friendly error when the
    /// entity does not exist.
    pub(super) fn parse_existing(&self, path: &str, entity: &str) -> Result<DocumentMut> {
        let content = self.content(path).ok_or_else(|| {
            RototoError::new(format!("{entity} does not exist ({path} not found)"))
        })?;
        parse_toml_document(path, content)
    }

    fn into_plan(self) -> EditPlan {
        let mut plan = EditPlan::default();
        for (path, edit) in self.edits {
            match edit {
                Some(content) => {
                    if self.base.content(&path) != Some(content.as_str()) {
                        plan.writes.push(PlannedWrite { path, content });
                    }
                }
                None => {
                    if self.base.contains(&path) {
                        plan.deletes.push(path);
                    }
                }
            }
        }
        plan
    }
}
