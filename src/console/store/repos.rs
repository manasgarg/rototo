use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::console::time::now_iso;
use crate::error::{Result, RototoError};

use super::Store;
use super::rows::workspace_from_row;
use super::types::{DiscoveredWorkspaceInput, RepoRecord, RepoWithWorkspaces, WorkspaceRecord};
use super::util::{db_err, new_id};

impl Store {
    pub async fn upsert_repo_with_workspaces(
        &self,
        principal_id: String,
        owner: String,
        name: String,
        default_ref: String,
        workspaces: Vec<DiscoveredWorkspaceInput>,
    ) -> Result<RepoWithWorkspaces> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let tx = conn.unchecked_transaction().map_err(db_err)?;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM repos WHERE principal_id = ?1 AND owner = ?2 AND name = ?3",
                    params![principal_id.as_str(), owner.as_str(), name.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_err)?;
            let repo_id = match existing {
                Some(repo_id) => {
                    tx.execute(
                    "UPDATE repos SET default_ref = ?1, updated_at = ?2, last_discovered_at = ?3
                     WHERE id = ?4",
                    params![
                        default_ref.as_str(),
                        now.as_str(),
                        now.as_str(),
                        repo_id.as_str()
                    ],
                )
                .map_err(db_err)?;
                    repo_id
                }
                None => {
                    let repo_id = new_id();
                    tx.execute(
                        "INSERT INTO repos (
                       id, principal_id, owner, name, default_ref,
                       created_at, updated_at, last_discovered_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            repo_id.as_str(),
                            principal_id.as_str(),
                            owner.as_str(),
                            name.as_str(),
                            default_ref.as_str(),
                            now.as_str(),
                            now.as_str(),
                            now.as_str()
                        ],
                    )
                    .map_err(db_err)?;
                    repo_id
                }
            };

            let mut discovered_keys = HashSet::new();
            for workspace in workspaces {
                discovered_keys.insert((workspace.path.clone(), workspace.git_ref.clone()));
                let updated = tx
                    .execute(
                        "UPDATE workspaces
                     SET owner = ?1, name = ?2, source = ?3, discovered_at = ?4, active = 1
                     WHERE repo_id = ?5 AND path = ?6 AND ref_ = ?7",
                        params![
                            owner.as_str(),
                            name.as_str(),
                            workspace.source.as_str(),
                            now.as_str(),
                            repo_id.as_str(),
                            workspace.path.as_str(),
                            workspace.git_ref.as_str(),
                        ],
                    )
                    .map_err(db_err)?;
                if updated == 0 {
                    let workspace_id = new_id();
                    tx.execute(
                        "INSERT INTO workspaces (
                       id, repo_id, owner, name, path, ref_, source, discovered_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            workspace_id.as_str(),
                            repo_id.as_str(),
                            owner.as_str(),
                            name.as_str(),
                            workspace.path.as_str(),
                            workspace.git_ref.as_str(),
                            workspace.source.as_str(),
                            now.as_str(),
                        ],
                    )
                    .map_err(db_err)?;
                }
            }

            let existing_workspaces = {
                let mut statement = tx
                    .prepare("SELECT id, path, ref_ FROM workspaces WHERE repo_id = ?1")
                    .map_err(db_err)?;
                statement
                    .query_map(params![repo_id.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(db_err)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(db_err)?
            };
            for (workspace_id, path, git_ref) in existing_workspaces {
                if discovered_keys.contains(&(path, git_ref)) {
                    continue;
                }
                tx.execute(
                    "DELETE FROM workspaces
                 WHERE id = ?1
                   AND NOT EXISTS (
                     SELECT 1 FROM draft_sessions WHERE workspace_id = ?1
                   )",
                    params![workspace_id.as_str()],
                )
                .map_err(db_err)?;
                tx.execute(
                    "UPDATE workspaces SET active = 0 WHERE id = ?1",
                    params![workspace_id.as_str()],
                )
                .map_err(db_err)?;
            }
            tx.commit().map_err(db_err)?;

            repo_with_workspaces_by_id(conn, &repo_id, &principal_id)?
                .ok_or_else(|| RototoError::new("repo registration failed"))
        })
        .await
    }

    pub async fn list_repos_for_user(&self, principal_id: &str) -> Result<Vec<RepoWithWorkspaces>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT id FROM repos WHERE principal_id = ?1
                 ORDER BY updated_at DESC, owner ASC, name ASC",
                )
                .map_err(db_err)?;
            let ids: Vec<String> = statement
                .query_map(params![principal_id], |row| row.get(0))
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            ids.iter()
                .map(|id| {
                    repo_with_workspaces_by_id(conn, id, &principal_id)?
                        .ok_or_else(|| RototoError::new("repo listing failed"))
                })
                .collect()
        })
        .await
    }

    pub async fn delete_repo_for_user(&self, repo_id: &str, principal_id: &str) -> Result<bool> {
        let repo_id = repo_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            if repo_with_workspaces_by_id(conn, &repo_id, &principal_id)?.is_none() {
                return Ok(false);
            }
            // ON DELETE CASCADE clears workspaces, draft sessions, changes,
            // and events transitively.
            conn.execute("DELETE FROM repos WHERE id = ?1", params![repo_id])
                .map_err(db_err)?;
            Ok(true)
        })
        .await
    }

    pub async fn list_workspaces_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<WorkspaceRecord>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| list_workspaces_for_user_sync(conn, &principal_id))
            .await
    }

    /// Accepts the row id or the derived slug, so friendly URLs and older id
    /// URLs both resolve.
    pub async fn get_workspace_for_user(
        &self,
        workspace_handle: &str,
        principal_id: &str,
    ) -> Result<Option<WorkspaceRecord>> {
        let workspace_handle = workspace_handle.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let by_id = conn
            .query_row(
                "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
                 FROM workspaces w
                 INNER JOIN repos r ON r.id = w.repo_id
                 WHERE w.id = ?1 AND r.principal_id = ?2",
                params![workspace_handle, principal_id],
                workspace_from_row,
            )
            .optional()
            .map_err(db_err)?;
            if by_id.is_some() {
                return Ok(by_id);
            }
            let active_match = list_workspaces_for_user_sync(conn, &principal_id)?
                .into_iter()
                .find(|workspace| workspace.slug == workspace_handle);
            if active_match.is_some() {
                return Ok(active_match);
            }
            Ok(list_all_workspaces_for_user_sync(conn, &principal_id)?
                .into_iter()
                .find(|workspace| workspace.slug == workspace_handle))
        })
        .await
    }
}

pub(super) fn workspace_slug(name: &str, path: &str) -> String {
    let base = if path == "." {
        name.to_owned()
    } else {
        format!("{name}-{path}")
    };
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in base.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(c);
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    slug
}

pub(super) fn list_workspaces_for_user_sync(
    conn: &Connection,
    principal_id: &str,
) -> Result<Vec<WorkspaceRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
             FROM workspaces w
             INNER JOIN repos r ON r.id = w.repo_id
             WHERE r.principal_id = ?1
               AND w.active = 1
             ORDER BY w.owner ASC, w.name ASC, w.path ASC",
        )
        .map_err(db_err)?;
    let workspaces = statement
        .query_map(params![principal_id], workspace_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)?;
    Ok(workspaces)
}

fn list_all_workspaces_for_user_sync(
    conn: &Connection,
    principal_id: &str,
) -> Result<Vec<WorkspaceRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
             FROM workspaces w
             INNER JOIN repos r ON r.id = w.repo_id
             WHERE r.principal_id = ?1
             ORDER BY w.owner ASC, w.name ASC, w.path ASC",
        )
        .map_err(db_err)?;
    let workspaces = statement
        .query_map(params![principal_id], workspace_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)?;
    Ok(workspaces)
}

fn repo_with_workspaces_by_id(
    conn: &Connection,
    repo_id: &str,
    principal_id: &str,
) -> Result<Option<RepoWithWorkspaces>> {
    let repo = conn
        .query_row(
            "SELECT id, principal_id, owner, name, default_ref,
                    created_at, updated_at, last_discovered_at
             FROM repos WHERE id = ?1 AND principal_id = ?2",
            params![repo_id, principal_id],
            |row| {
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
            },
        )
        .optional()
        .map_err(db_err)?;
    let Some(repo) = repo else {
        return Ok(None);
    };
    let mut statement = conn
        .prepare(
            "SELECT id, repo_id, owner, name, path, ref_, source, discovered_at
             FROM workspaces
             WHERE repo_id = ?1 AND active = 1
             ORDER BY path ASC",
        )
        .map_err(db_err)?;
    let workspaces = statement
        .query_map(params![repo.id], workspace_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)?;
    Ok(Some(RepoWithWorkspaces { repo, workspaces }))
}
