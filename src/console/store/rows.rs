use super::repos::workspace_slug;
use super::types::{
    DraftChangeRecord, DraftEventRecord, DraftSessionRecord, DraftStatus, RepoRecord,
    WorkspaceRecord,
};

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

pub(super) fn draft_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftSessionRecord> {
    let status: String = row.get(5)?;
    Ok(DraftSessionRecord {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        principal_id: row.get(2)?,
        branch: row.get(3)?,
        base_ref: row.get(4)?,
        status: match status.as_str() {
            "published" => DraftStatus::Published,
            "abandoned" => DraftStatus::Abandoned,
            _ => DraftStatus::Open,
        },
        pr_url: row.get(6)?,
        pr_number: row.get(7)?,
        pr_state: row.get(8)?,
        pr_merged_at: row.get(9)?,
        pr_synced_at: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        published_at: row.get(13)?,
    })
}

pub(super) fn change_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftChangeRecord> {
    Ok(DraftChangeRecord {
        id: row.get(0)?,
        draft_id: row.get(1)?,
        file_path: row.get(2)?,
        target_path: row.get(3)?,
        before_json: row.get(4)?,
        after_json: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub(super) fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftEventRecord> {
    Ok(DraftEventRecord {
        id: row.get(0)?,
        draft_id: row.get(1)?,
        kind: row.get(2)?,
        summary: row.get(3)?,
        detail_json: row.get(4)?,
        created_at: row.get(5)?,
    })
}
