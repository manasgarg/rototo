use super::repos::workspace_slug;
use super::types::{RepoRecord, TrackedBranchRecord, TrackedBranchStatus, WorkspaceRecord};

pub(super) fn repo_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepoRecord> {
    Ok(RepoRecord {
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
        repo_id: row.get(offset + 1)?,
        owner: row.get(offset + 2)?,
        name,
        path,
        git_ref: row.get(offset + 5)?,
        source: row.get(offset + 6)?,
        discovered_at: row.get(offset + 7)?,
    })
}

pub(super) fn tracked_branch_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<TrackedBranchRecord> {
    let status: String = row.get(13)?;
    Ok(TrackedBranchRecord {
        id: row.get(0)?,
        repo_id: row.get(1)?,
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
            "recent" => TrackedBranchStatus::Recent,
            "archived" => TrackedBranchStatus::Archived,
            _ => TrackedBranchStatus::Active,
        },
        created_at: row.get(14)?,
        last_opened_at: row.get(15)?,
        last_edited_at: row.get(16)?,
        archived_at: row.get(17)?,
    })
}
