use rusqlite::{Connection, OptionalExtension, params};

use crate::console::time::now_iso;
use crate::error::{Result, RototoError};

use super::Store;
use super::rows::{active_branch_from_row, package_from_row_at};
use super::types::{
    ActiveBranchRecord, ActiveBranchStatus, ActiveBranchWithPackageRecord, BranchPullRequestInput,
    PackageRecord, SelectBranchInput,
};
use super::util::{db_err, new_id};

struct PackageBranchBase {
    source_tree_id: String,
    package_path: String,
}

impl Store {
    pub async fn select_branch(&self, input: SelectBranchInput) -> Result<ActiveBranchRecord> {
        self.with_conn(move |conn, _| {
            let package = branch_package_base(conn, &input.package_id, &input.principal_id)?
                .ok_or_else(|| RototoError::new("package not found for principal"))?;
            let now = now_iso();
            let branch_id = match active_branch_id_by_source_tree_branch(
                conn,
                &package.source_tree_id,
                &input.principal_id,
                &input.branch,
            )? {
                Some(branch_id) => {
                    conn.execute(
                        "UPDATE active_branches
                         SET base_ref = ?1,
                             base_commit = ?2,
                             last_seen_commit = ?3,
                             last_selected_package_path = ?4,
                             status = 'active',
                             last_opened_at = ?5,
                             archived_at = NULL
                         WHERE id = ?6",
                        params![
                            input.base_ref,
                            input.base_commit,
                            input.last_seen_commit,
                            package.package_path,
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
                        "INSERT INTO active_branches (
                           id, source_tree_id, principal_id, branch, base_ref, base_commit,
                           last_selected_package_path, last_seen_commit, status,
                           created_at, last_opened_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?10)",
                        params![
                            branch_id,
                            package.source_tree_id,
                            input.principal_id,
                            input.branch,
                            input.base_ref,
                            input.base_commit,
                            package.package_path,
                            input.last_seen_commit,
                            now,
                            now,
                        ],
                    )
                    .map_err(db_err)?;
                    branch_id
                }
            };
            insert_active_branch_package_sync(conn, &branch_id, &package.package_path, &now)?;
            active_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("active branch creation failed"))
        })
        .await
    }

    pub async fn ensure_active_branch_package(
        &self,
        branch_id: &str,
        package_id: &str,
        principal_id: &str,
    ) -> Result<ActiveBranchRecord> {
        let branch_id = branch_id.to_owned();
        let package_id = package_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let branch = active_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("active branch not found"))?;
            if branch.principal_id != principal_id {
                return Err(RototoError::new("active branch not found"));
            }
            let package = branch_package_base(conn, &package_id, &principal_id)?
                .ok_or_else(|| RototoError::new("package not found for principal"))?;
            if package.source_tree_id != branch.source_tree_id {
                return Err(RototoError::new(
                    "active branch package must belong to the same source tree",
                ));
            }
            let now = now_iso();
            insert_active_branch_package_sync(conn, &branch_id, &package.package_path, &now)?;
            conn.execute(
                "UPDATE active_branches
                 SET last_selected_package_path = ?1,
                     status = 'active',
                     last_opened_at = ?2,
                     archived_at = NULL
                 WHERE id = ?3",
                params![package.package_path, now, branch_id],
            )
            .map_err(db_err)?;
            active_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("active branch package update failed"))
        })
        .await
    }

    pub async fn list_active_branches_for_package(
        &self,
        package_id: &str,
        principal_id: &str,
    ) -> Result<Vec<ActiveBranchRecord>> {
        let package_id = package_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(&format!(
                    "{ACTIVE_BRANCH_SELECT}
                     INNER JOIN source_tree_packages requested_package
                       ON requested_package.id = ?1
                     INNER JOIN active_branch_packages abw
                       ON abw.branch_id = b.id
                      AND abw.package_path = requested_package.path
                     WHERE b.source_tree_id = requested_package.source_tree_id
                       AND b.principal_id = ?2
                       AND b.status != 'archived'
                     ORDER BY b.last_opened_at DESC, b.branch ASC"
                ))
                .map_err(db_err)?;
            statement
                .query_map(params![package_id, principal_id], active_branch_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    #[cfg(test)]
    pub async fn list_active_branches_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<ActiveBranchRecord>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(&format!(
                    "{ACTIVE_BRANCH_SELECT}
                     WHERE b.principal_id = ?1
                       AND b.status != 'archived'
                     ORDER BY b.last_opened_at DESC, b.branch ASC"
                ))
                .map_err(db_err)?;
            statement
                .query_map(params![principal_id], active_branch_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    pub async fn list_active_branches_with_packages_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<ActiveBranchWithPackageRecord>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(&format!(
                    "SELECT {ACTIVE_BRANCH_COLUMNS},
                            w.id, w.source_tree_id, w.path, w.revision, w.source_tree_label, w.source, w.discovered_at
                     FROM active_branches b
                     INNER JOIN active_branch_packages abw ON abw.branch_id = b.id
                     INNER JOIN source_tree_packages w
                       ON w.source_tree_id = b.source_tree_id
                      AND w.path = abw.package_path
                     WHERE b.principal_id = ?1
                       AND b.status != 'archived'
                     ORDER BY b.last_opened_at DESC, b.branch ASC, w.path ASC"
                ))
                .map_err(db_err)?;
            statement
                .query_map(params![principal_id], |row| {
                    Ok(ActiveBranchWithPackageRecord {
                        branch: active_branch_from_row(row)?,
                        package: package_from_row_at(row, 18)?,
                    })
                })
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    pub async fn get_active_branch_for_user(
        &self,
        branch_id: &str,
        package_id: &str,
        principal_id: &str,
    ) -> Result<Option<ActiveBranchRecord>> {
        let branch_id = branch_id.to_owned();
        let package_id = package_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            conn.query_row(
                &format!(
                    "{ACTIVE_BRANCH_SELECT}
                     INNER JOIN source_tree_packages requested_package
                       ON requested_package.id = ?2
                     INNER JOIN active_branch_packages abw
                       ON abw.branch_id = b.id
                      AND abw.package_path = requested_package.path
                     WHERE b.id = ?1
                       AND b.source_tree_id = requested_package.source_tree_id
                       AND b.principal_id = ?3"
                ),
                params![branch_id, package_id, principal_id],
                active_branch_from_row,
            )
            .optional()
            .map_err(db_err)
        })
        .await
    }

    pub async fn find_active_branch_for_source_tree_branch(
        &self,
        package_id: &str,
        principal_id: &str,
        branch: &str,
    ) -> Result<Option<ActiveBranchRecord>> {
        let package_id = package_id.to_owned();
        let principal_id = principal_id.to_owned();
        let branch = branch.to_owned();
        self.with_conn(move |conn, _| {
            conn.query_row(
                &format!(
                    "{ACTIVE_BRANCH_SELECT}
                     INNER JOIN source_tree_packages requested_package ON requested_package.id = ?1
                     WHERE b.source_tree_id = requested_package.source_tree_id
                       AND b.principal_id = ?2
                       AND b.branch = ?3
                       AND b.status = 'active'
                     ORDER BY b.last_opened_at DESC
                     LIMIT 1"
                ),
                params![package_id, principal_id, branch],
                active_branch_from_row,
            )
            .optional()
            .map_err(db_err)
        })
        .await
    }

    pub async fn list_packages_for_active_branch(
        &self,
        branch_id: &str,
    ) -> Result<Vec<PackageRecord>> {
        let branch_id = branch_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT w.id, w.source_tree_id, w.path, w.revision, w.source_tree_label, w.source, w.discovered_at
                     FROM active_branch_packages abw
                     INNER JOIN active_branches b ON b.id = abw.branch_id
                     INNER JOIN source_tree_packages w
                       ON w.source_tree_id = b.source_tree_id
                      AND w.path = abw.package_path
                     WHERE abw.branch_id = ?1
                     ORDER BY w.path ASC",
                )
                .map_err(db_err)?;
            statement
                .query_map(params![branch_id], |row| package_from_row_at(row, 0))
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    #[cfg(test)]
    pub async fn mark_active_branch_recent(&self, branch_id: &str) -> Result<ActiveBranchRecord> {
        update_active_branch_status(self, branch_id, ActiveBranchStatus::Recent).await
    }

    pub async fn archive_active_branch(&self, branch_id: &str) -> Result<ActiveBranchRecord> {
        update_active_branch_status(self, branch_id, ActiveBranchStatus::Archived).await
    }

    pub async fn rename_active_branch(
        &self,
        branch_id: &str,
        branch: &str,
    ) -> Result<ActiveBranchRecord> {
        let branch_id = branch_id.to_owned();
        let branch = branch.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE active_branches
                 SET branch = ?1,
                     last_opened_at = ?2,
                     status = 'active',
                     archived_at = NULL
                 WHERE id = ?3",
                params![branch, now, branch_id],
            )
            .map_err(db_err)?;
            active_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("active branch rename failed"))
        })
        .await
    }

    pub async fn record_active_branch_edit(
        &self,
        branch_id: &str,
        last_seen_commit: Option<String>,
    ) -> Result<ActiveBranchRecord> {
        let branch_id = branch_id.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE active_branches
                 SET last_edited_at = ?1,
                     last_seen_commit = COALESCE(?2, last_seen_commit),
                     status = 'active',
                     archived_at = NULL
                 WHERE id = ?3",
                params![now, last_seen_commit, branch_id],
            )
            .map_err(db_err)?;
            active_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("active branch edit update failed"))
        })
        .await
    }

    pub async fn update_active_branch_pull_request_state(
        &self,
        input: BranchPullRequestInput,
    ) -> Result<ActiveBranchRecord> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE active_branches
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
            active_branch_by_id(conn, &input.branch_id)?
                .ok_or_else(|| RototoError::new("active branch pull request update failed"))
        })
        .await
    }
}

async fn update_active_branch_status(
    store: &Store,
    branch_id: &str,
    status: ActiveBranchStatus,
) -> Result<ActiveBranchRecord> {
    let branch_id = branch_id.to_owned();
    store
        .with_conn(move |conn, _| {
            let now = now_iso();
            let (status_value, archived_at) = match status {
                ActiveBranchStatus::Active => ("active", None),
                ActiveBranchStatus::Recent => ("recent", None),
                ActiveBranchStatus::Archived => ("archived", Some(now.as_str())),
            };
            conn.execute(
                "UPDATE active_branches
                 SET status = ?1,
                     archived_at = ?2
                 WHERE id = ?3",
                params![status_value, archived_at, branch_id],
            )
            .map_err(db_err)?;
            active_branch_by_id(conn, &branch_id)?
                .ok_or_else(|| RototoError::new("active branch status update failed"))
        })
        .await
}

const ACTIVE_BRANCH_COLUMNS: &str = "b.id, b.source_tree_id, b.principal_id, b.branch, b.base_ref,
    b.base_commit, b.pr_url, b.pr_number, b.pr_state, b.pr_merged_at, b.pr_synced_at,
    b.last_selected_package_path, b.last_seen_commit, b.status, b.created_at,
    b.last_opened_at, b.last_edited_at, b.archived_at";

const ACTIVE_BRANCH_SELECT: &str =
    "SELECT b.id, b.source_tree_id, b.principal_id, b.branch, b.base_ref,
        b.base_commit, b.pr_url, b.pr_number, b.pr_state, b.pr_merged_at, b.pr_synced_at,
        b.last_selected_package_path, b.last_seen_commit, b.status, b.created_at,
        b.last_opened_at, b.last_edited_at, b.archived_at
     FROM active_branches b";

fn active_branch_by_id(conn: &Connection, branch_id: &str) -> Result<Option<ActiveBranchRecord>> {
    conn.query_row(
        &format!(
            "SELECT {ACTIVE_BRANCH_COLUMNS}
             FROM active_branches b
             WHERE b.id = ?1"
        ),
        params![branch_id],
        active_branch_from_row,
    )
    .optional()
    .map_err(db_err)
}

fn active_branch_id_by_source_tree_branch(
    conn: &Connection,
    source_tree_id: &str,
    principal_id: &str,
    branch: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM active_branches
         WHERE source_tree_id = ?1 AND principal_id = ?2 AND branch = ?3",
        params![source_tree_id, principal_id, branch],
        |row| row.get(0),
    )
    .optional()
    .map_err(db_err)
}

fn branch_package_base(
    conn: &Connection,
    package_id: &str,
    principal_id: &str,
) -> Result<Option<PackageBranchBase>> {
    conn.query_row(
        "SELECT w.source_tree_id, w.path
         FROM source_tree_packages w
         INNER JOIN source_trees r ON r.id = w.source_tree_id
         WHERE w.id = ?1 AND r.principal_id = ?2",
        params![package_id, principal_id],
        |row| {
            Ok(PackageBranchBase {
                source_tree_id: row.get(0)?,
                package_path: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(db_err)
}

fn insert_active_branch_package_sync(
    conn: &Connection,
    branch_id: &str,
    package_path: &str,
    added_at: &str,
) -> Result<bool> {
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO active_branch_packages (branch_id, package_path, added_at)
             VALUES (?1, ?2, ?3)",
            params![branch_id, package_path, added_at],
        )
        .map_err(db_err)?;
    Ok(inserted != 0)
}
