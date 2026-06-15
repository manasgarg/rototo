#![allow(dead_code)]

use rusqlite::{Connection, OptionalExtension, params};

use crate::console::time::now_iso;
use crate::error::{Result, RototoError};

use super::Store;
use super::rows::{tracked_branch_from_row, workspace_from_row_at};
use super::types::{
    TrackBranchInput, TrackedBranchPullRequestInput, TrackedBranchRecord, TrackedBranchStatus,
    TrackedBranchWithWorkspaceRecord, WorkspaceRecord,
};
use super::util::{db_err, new_id};

struct WorkspaceBranchBase {
    repo_id: String,
    workspace_path: String,
}

impl Store {
    pub async fn track_branch(&self, input: TrackBranchInput) -> Result<TrackedBranchRecord> {
        self.with_conn(move |conn, _| {
            let workspace = branch_workspace_base(conn, &input.workspace_id, &input.principal_id)?
                .ok_or_else(|| RototoError::new("workspace not found for principal"))?;
            let now = now_iso();
            let branch_id = match tracked_branch_id_by_repo_branch(
                conn,
                &workspace.repo_id,
                &input.principal_id,
                &input.branch,
            )? {
                Some(branch_id) => {
                    conn.execute(
                        "UPDATE tracked_branches
                         SET base_ref = ?1,
                             base_commit = ?2,
                             last_seen_commit = ?3,
                             last_selected_workspace_path = ?4,
                             status = 'active',
                             last_opened_at = ?5,
                             archived_at = NULL
                         WHERE id = ?6",
                        params![
                            input.base_ref,
                            input.base_commit,
                            input.last_seen_commit,
                            workspace.workspace_path,
                            now,
                            branch_id,
                        ],
                    )
                    .map_err(db_err)?;
                    branch_id
                }
                None => {
                    let branch_id = new_id();
                    conn.execute(
                        "INSERT INTO tracked_branches (
                           id, repo_id, principal_id, branch, base_ref, base_commit,
                           last_selected_workspace_path, last_seen_commit, status,
                           created_at, last_opened_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?10)",
                        params![
                            branch_id,
                            workspace.repo_id,
                            input.principal_id,
                            input.branch,
                            input.base_ref,
                            input.base_commit,
                            workspace.workspace_path,
                            input.last_seen_commit,
                            now,
                            now,
                        ],
                    )
                    .map_err(db_err)?;
                    branch_id
                }
            };
            insert_tracked_branch_workspace_sync(conn, &branch_id, &input.workspace_id, &now)?;
            tracked_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("tracked branch creation failed"))
        })
        .await
    }

    pub async fn ensure_tracked_branch_workspace(
        &self,
        branch_id: &str,
        workspace_id: &str,
        principal_id: &str,
    ) -> Result<TrackedBranchRecord> {
        let branch_id = branch_id.to_owned();
        let workspace_id = workspace_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let branch = tracked_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("tracked branch not found"))?;
            if branch.principal_id != principal_id {
                return Err(RototoError::new("tracked branch not found"));
            }
            let workspace = branch_workspace_base(conn, &workspace_id, &principal_id)?
                .ok_or_else(|| RototoError::new("workspace not found for principal"))?;
            if workspace.repo_id != branch.repo_id {
                return Err(RototoError::new(
                    "tracked branch workspace must belong to the same repo",
                ));
            }
            let now = now_iso();
            insert_tracked_branch_workspace_sync(conn, &branch_id, &workspace_id, &now)?;
            conn.execute(
                "UPDATE tracked_branches
                 SET last_selected_workspace_path = ?1,
                     status = 'active',
                     last_opened_at = ?2,
                     archived_at = NULL
                 WHERE id = ?3",
                params![workspace.workspace_path, now, branch_id],
            )
            .map_err(db_err)?;
            tracked_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("tracked branch workspace update failed"))
        })
        .await
    }

    pub async fn list_tracked_branches_for_workspace(
        &self,
        workspace_id: &str,
        principal_id: &str,
    ) -> Result<Vec<TrackedBranchRecord>> {
        let workspace_id = workspace_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(&format!(
                    "{TRACKED_BRANCH_SELECT}
                     INNER JOIN tracked_branch_workspaces tbw ON tbw.branch_id = b.id
                     WHERE tbw.workspace_id = ?1
                       AND b.principal_id = ?2
                       AND b.status != 'archived'
                     ORDER BY b.last_opened_at DESC, b.branch ASC"
                ))
                .map_err(db_err)?;
            statement
                .query_map(params![workspace_id, principal_id], tracked_branch_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    pub async fn list_tracked_branches_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<TrackedBranchRecord>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(&format!(
                    "{TRACKED_BRANCH_SELECT}
                     WHERE b.principal_id = ?1
                       AND b.status != 'archived'
                     ORDER BY b.last_opened_at DESC, b.branch ASC"
                ))
                .map_err(db_err)?;
            statement
                .query_map(params![principal_id], tracked_branch_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    pub async fn list_tracked_branches_with_workspaces_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<TrackedBranchWithWorkspaceRecord>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(&format!(
                    "SELECT {TRACKED_BRANCH_COLUMNS},
                            w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
                     FROM tracked_branches b
                     INNER JOIN tracked_branch_workspaces tbw ON tbw.branch_id = b.id
                     INNER JOIN workspaces w ON w.id = tbw.workspace_id
                     WHERE b.principal_id = ?1
                       AND b.status != 'archived'
                     ORDER BY b.last_opened_at DESC, b.branch ASC, w.path ASC"
                ))
                .map_err(db_err)?;
            statement
                .query_map(params![principal_id], |row| {
                    Ok(TrackedBranchWithWorkspaceRecord {
                        branch: tracked_branch_from_row(row)?,
                        workspace: workspace_from_row_at(row, 18)?,
                    })
                })
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    pub async fn get_tracked_branch_for_user(
        &self,
        branch_id: &str,
        workspace_id: &str,
        principal_id: &str,
    ) -> Result<Option<TrackedBranchRecord>> {
        let branch_id = branch_id.to_owned();
        let workspace_id = workspace_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            conn.query_row(
                &format!(
                    "{TRACKED_BRANCH_SELECT}
                     INNER JOIN tracked_branch_workspaces tbw ON tbw.branch_id = b.id
                     WHERE b.id = ?1
                       AND tbw.workspace_id = ?2
                       AND b.principal_id = ?3"
                ),
                params![branch_id, workspace_id, principal_id],
                tracked_branch_from_row,
            )
            .optional()
            .map_err(db_err)
        })
        .await
    }

    pub async fn find_active_tracked_branch_for_repo_branch(
        &self,
        workspace_id: &str,
        principal_id: &str,
        branch: &str,
    ) -> Result<Option<TrackedBranchRecord>> {
        let workspace_id = workspace_id.to_owned();
        let principal_id = principal_id.to_owned();
        let branch = branch.to_owned();
        self.with_conn(move |conn, _| {
            conn.query_row(
                &format!(
                    "{TRACKED_BRANCH_SELECT}
                     INNER JOIN workspaces requested_workspace ON requested_workspace.id = ?1
                     WHERE b.repo_id = requested_workspace.repo_id
                       AND b.principal_id = ?2
                       AND b.branch = ?3
                       AND b.status = 'active'
                     ORDER BY b.last_opened_at DESC
                     LIMIT 1"
                ),
                params![workspace_id, principal_id, branch],
                tracked_branch_from_row,
            )
            .optional()
            .map_err(db_err)
        })
        .await
    }

    pub async fn list_workspaces_for_tracked_branch(
        &self,
        branch_id: &str,
    ) -> Result<Vec<WorkspaceRecord>> {
        let branch_id = branch_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
                     FROM tracked_branch_workspaces tbw
                     INNER JOIN workspaces w ON w.id = tbw.workspace_id
                     WHERE tbw.branch_id = ?1
                     ORDER BY w.path ASC",
                )
                .map_err(db_err)?;
            statement
                .query_map(params![branch_id], |row| workspace_from_row_at(row, 0))
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    pub async fn mark_tracked_branch_recent(&self, branch_id: &str) -> Result<TrackedBranchRecord> {
        update_tracked_branch_status(self, branch_id, TrackedBranchStatus::Recent).await
    }

    pub async fn archive_tracked_branch(&self, branch_id: &str) -> Result<TrackedBranchRecord> {
        update_tracked_branch_status(self, branch_id, TrackedBranchStatus::Archived).await
    }

    pub async fn rename_tracked_branch(
        &self,
        branch_id: &str,
        branch: &str,
    ) -> Result<TrackedBranchRecord> {
        let branch_id = branch_id.to_owned();
        let branch = branch.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE tracked_branches
                 SET branch = ?1,
                     last_opened_at = ?2,
                     status = 'active',
                     archived_at = NULL
                 WHERE id = ?3",
                params![branch, now, branch_id],
            )
            .map_err(db_err)?;
            tracked_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("tracked branch rename failed"))
        })
        .await
    }

    pub async fn record_tracked_branch_edit(
        &self,
        branch_id: &str,
        last_seen_commit: Option<String>,
    ) -> Result<TrackedBranchRecord> {
        let branch_id = branch_id.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE tracked_branches
                 SET last_edited_at = ?1,
                     last_seen_commit = COALESCE(?2, last_seen_commit),
                     status = 'active',
                     archived_at = NULL
                 WHERE id = ?3",
                params![now, last_seen_commit, branch_id],
            )
            .map_err(db_err)?;
            tracked_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("tracked branch edit update failed"))
        })
        .await
    }

    pub async fn update_tracked_branch_pull_request_state(
        &self,
        input: TrackedBranchPullRequestInput,
    ) -> Result<TrackedBranchRecord> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE tracked_branches
                 SET pr_number = ?1,
                     pr_state = ?2,
                     pr_url = ?3,
                     pr_merged_at = ?4,
                     pr_synced_at = ?5
                 WHERE id = ?6",
                params![
                    input.pr_number,
                    input.pr_state,
                    input.pr_url,
                    input.pr_merged_at,
                    now,
                    input.branch_id,
                ],
            )
            .map_err(db_err)?;
            tracked_branch_by_id(conn, &input.branch_id)?
                .ok_or_else(|| RototoError::new("tracked branch pull request update failed"))
        })
        .await
    }
}

async fn update_tracked_branch_status(
    store: &Store,
    branch_id: &str,
    status: TrackedBranchStatus,
) -> Result<TrackedBranchRecord> {
    let branch_id = branch_id.to_owned();
    store
        .with_conn(move |conn, _| {
            let now = now_iso();
            let (status_value, archived_at) = match status {
                TrackedBranchStatus::Active => ("active", None),
                TrackedBranchStatus::Recent => ("recent", None),
                TrackedBranchStatus::Archived => ("archived", Some(now.as_str())),
            };
            conn.execute(
                "UPDATE tracked_branches
                 SET status = ?1,
                     archived_at = ?2
                 WHERE id = ?3",
                params![status_value, archived_at, branch_id],
            )
            .map_err(db_err)?;
            tracked_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("tracked branch status update failed"))
        })
        .await
}

const TRACKED_BRANCH_COLUMNS: &str = "b.id, b.repo_id, b.principal_id, b.branch, b.base_ref,
    b.base_commit, b.pr_url, b.pr_number, b.pr_state, b.pr_merged_at, b.pr_synced_at,
    b.last_selected_workspace_path, b.last_seen_commit, b.status, b.created_at,
    b.last_opened_at, b.last_edited_at, b.archived_at";

const TRACKED_BRANCH_SELECT: &str = "SELECT b.id, b.repo_id, b.principal_id, b.branch, b.base_ref,
        b.base_commit, b.pr_url, b.pr_number, b.pr_state, b.pr_merged_at, b.pr_synced_at,
        b.last_selected_workspace_path, b.last_seen_commit, b.status, b.created_at,
        b.last_opened_at, b.last_edited_at, b.archived_at
     FROM tracked_branches b";

fn tracked_branch_by_id(conn: &Connection, branch_id: &str) -> Result<Option<TrackedBranchRecord>> {
    conn.query_row(
        &format!(
            "SELECT {TRACKED_BRANCH_COLUMNS}
             FROM tracked_branches b
             WHERE b.id = ?1"
        ),
        params![branch_id],
        tracked_branch_from_row,
    )
    .optional()
    .map_err(db_err)
}

fn tracked_branch_id_by_repo_branch(
    conn: &Connection,
    repo_id: &str,
    principal_id: &str,
    branch: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM tracked_branches
         WHERE repo_id = ?1 AND principal_id = ?2 AND branch = ?3",
        params![repo_id, principal_id, branch],
        |row| row.get(0),
    )
    .optional()
    .map_err(db_err)
}

fn branch_workspace_base(
    conn: &Connection,
    workspace_id: &str,
    principal_id: &str,
) -> Result<Option<WorkspaceBranchBase>> {
    conn.query_row(
        "SELECT w.repo_id, w.path
         FROM workspaces w
         INNER JOIN repos r ON r.id = w.repo_id
         WHERE w.id = ?1 AND r.principal_id = ?2",
        params![workspace_id, principal_id],
        |row| {
            Ok(WorkspaceBranchBase {
                repo_id: row.get(0)?,
                workspace_path: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(db_err)
}

fn insert_tracked_branch_workspace_sync(
    conn: &Connection,
    branch_id: &str,
    workspace_id: &str,
    added_at: &str,
) -> Result<bool> {
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO tracked_branch_workspaces (branch_id, workspace_id, added_at)
             VALUES (?1, ?2, ?3)",
            params![branch_id, workspace_id, added_at],
        )
        .map_err(db_err)?;
    Ok(inserted != 0)
}
