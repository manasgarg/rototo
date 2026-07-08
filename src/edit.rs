//! The edit engine (`design/console-semantic.md`): named semantic operations
//! compiled into format-preserving file edits.
//!
//! ```text
//! apply(staged tree, [operation]) -> { edit plan, [change record] }
//! ```
//!
//! The engine validates each operation structurally (the target exists, the
//! value parses, the index is in range), splices the new text into the
//! touched files, and returns the file writes plus a structured record of
//! what changed. Comments and formatting outside the spliced ranges survive
//! untouched. The engine refuses nonsense; lint judges meaning on the
//! post-edit tree.
//!
//! Operations point at entities with the package addressing grammar
//! (`design/addressing.md`), and change records point back with the same
//! grammar, so lint targets, grant scopes, edit operations, and change
//! records all share one way of naming things.
//!
//! V1 compiles operations against entities the package owns. The ownership
//! parameter ([`EditOptions::inherited`]) is part of the contract from day
//! one; compilation to overlay shapes (`.update.toml` and `.deleted.toml`
//! markers) lands behind it later without reshaping callers.

use std::path::Path;

use crate::error::{Result, RototoError};

mod create;
mod engine;
mod entry;
mod layer;
mod lists;
mod operation;
mod paths;
#[cfg(test)]
mod tests;
mod tree;
mod value;
mod variable;

pub use operation::{
    AllocationArmInput, ChangeRecord, EditOperation, EditOptions, EditOutcome, EditPlan,
    PlannedWrite,
};
pub use tree::EditTree;

/// Applies operations to a staged package tree, returning the edit plan and
/// the change records. Pure: nothing touches disk.
pub fn apply(
    tree: &EditTree,
    operations: &[EditOperation],
    options: &EditOptions,
) -> Result<EditOutcome> {
    engine::apply(tree, operations, options)
}

/// Snapshots the package at `root` and applies operations against it. The
/// plan is returned, not written; pair with [`write_plan`] to persist it.
pub async fn apply_to_package(
    root: &Path,
    operations: &[EditOperation],
    options: &EditOptions,
) -> Result<EditOutcome> {
    let tree = EditTree::snapshot(root).await?;
    apply(&tree, operations, options)
}

/// Writes an edit plan into the package at `root`: file writes first
/// (creating parent directories), deletions after.
pub async fn write_plan(root: &Path, plan: &EditPlan) -> Result<()> {
    for write in &plan.writes {
        let path = root.join(&write.path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|err| {
                RototoError::new(format!(
                    "failed to create directory {}: {err}",
                    parent.display()
                ))
            })?;
        }
        tokio::fs::write(&path, &write.content)
            .await
            .map_err(|err| {
                RototoError::new(format!("failed to write {}: {err}", path.display()))
            })?;
    }
    for delete in &plan.deletes {
        let path = root.join(delete);
        tokio::fs::remove_file(&path).await.map_err(|err| {
            RototoError::new(format!("failed to delete {}: {err}", path.display()))
        })?;
    }
    Ok(())
}
