use super::source_trees::workspace_slug;
use super::types::{ActiveBranchRecord, ActiveBranchStatus, SourceTreeRecord, WorkspaceRecord};

pub(super) fn source_tree_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceTreeRecord> {
    Ok(SourceTreeRecord {
        id: row.get(0)?,
        principal_id: row.get(1)?,
        owner: row.get(2)?,
        name: row.get(3)?,
        default_ref: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        last_discovered_at: row.get(7)?,
    })
}

pub(super) fn workspace_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRecord> {
    workspace_from_row_at(row, 0)
}

pub(super) fn workspace_from_row_at(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<WorkspaceRecord> {
    let name: String = row.get(offset + 3)?;
    let path: String = row.get(offset + 4)?;
    Ok(WorkspaceRecord {
        id: row.get(offset)?,
        slug: workspace_slug(&name, &path),
        source_tree_id: row.get(offset + 1)?,
        owner: row.get(offset + 2)?,
        name,
        path,
        git_ref: row.get(offset + 5)?,
        source: row.get(offset + 6)?,
        discovered_at: row.get(offset + 7)?,
    })
}

pub(super) fn active_branch_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ActiveBranchRecord> {
    let status: String = row.get(13)?;
    Ok(ActiveBranchRecord {
        id: row.get(0)?,
        source_tree_id: row.get(1)?,
        principal_id: row.get(2)?,
        branch: row.get(3)?,
        base_ref: row.get(4)?,
        base_commit: row.get(5)?,
        pr_url: row.get(6)?,
        pr_number: row.get(7)?,
        pr_state: row.get(8)?,
        pr_merged_at: row.get(9)?,
        pr_synced_at: row.get(10)?,
        last_selected_workspace_path: row.get(11)?,
        last_seen_commit: row.get(12)?,
        status: match status.as_str() {
            "recent" => ActiveBranchStatus::Recent,
            "archived" => ActiveBranchStatus::Archived,
            _ => ActiveBranchStatus::Active,
        },
        created_at: row.get(14)?,
        last_opened_at: row.get(15)?,
        last_edited_at: row.get(16)?,
        archived_at: row.get(17)?,
    })
}
