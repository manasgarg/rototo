use std::path::Path;

use super::source_trees::package_slug;
use super::types::{ActiveBranchRecord, ActiveBranchStatus, PackageRecord, SourceTreeRecord};

pub(super) fn source_tree_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceTreeRecord> {
    let kind = {
        let value: String = row.get(2)?;
        super::types::SourceTreeKind::from_str(&value)
    };
    Ok(SourceTreeRecord {
        id: row.get(0)?,
        principal_id: row.get(1)?,
        kind,
        source: row.get(3)?,
        display_name: row.get(4)?,
        default_revision: row.get(5)?,
        capabilities: kind.capabilities(),
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        last_discovered_at: row.get(8)?,
    })
}

pub(super) fn package_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PackageRecord> {
    package_from_row_at(row, 0)
}

pub(super) fn package_from_row_at(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<PackageRecord> {
    let source_tree_label: String = row.get(offset + 4)?;
    let path: String = row.get(offset + 2)?;
    let source: String = row.get(offset + 5)?;
    Ok(PackageRecord {
        id: row.get(offset)?,
        slug: package_slug(&source_tree_label, &path),
        source_tree_id: row.get(offset + 1)?,
        source_tree_label,
        display_path: package_display_path(&path, &source),
        path,
        revision: row.get(offset + 3)?,
        source,
        discovered_at: row.get(offset + 6)?,
    })
}

fn package_display_path(path: &str, source: &str) -> String {
    if path != "." {
        return path.to_owned();
    }
    let local_source = source.strip_prefix("file://").unwrap_or(source);
    if local_source.contains("://") || local_source.starts_with("git+") {
        return path.to_owned();
    }
    let trimmed = local_source.trim_end_matches(['/', '\\']);
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
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
        last_selected_package_path: row.get(11)?,
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
