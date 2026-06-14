use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::console::time::now_iso;
use crate::error::{Result, RototoError};

use super::Store;
use super::rows::{repo_from_row, workspace_from_row};
use super::types::{DiscoveredWorkspaceInput, RepoWithWorkspaces, WorkspaceRecord};
use super::util::{db_err, new_id};

#[derive(Hash, PartialEq, Eq)]
struct WorkspaceKey {
    path: String,
    git_ref: String,
}

struct WorkspaceRowKey {
    id: String,
    key: WorkspaceKey,
}

impl WorkspaceKey {
    fn from_discovered(workspace: &DiscoveredWorkspaceInput) -> Self {
        Self {
            path: workspace.path.clone(),
            git_ref: workspace.git_ref.clone(),
        }
    }
}

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

            let repo_id = upsert_repo_row(&tx, &principal_id, &owner, &name, &default_ref, &now)?;
            let discovered_keys =
                upsert_discovered_workspaces(&tx, &repo_id, &owner, &name, &workspaces, &now)?;
            cleanup_missing_workspaces(&tx, &repo_id, &discovered_keys)?;

            tx.commit().map_err(db_err)?;

            repo_with_workspaces_by_id(conn, &repo_id, &principal_id)?
                .ok_or_else(|| RototoError::new("repo registration failed"))
        })
        .await
    }

    pub async fn list_repos_for_user(&self, principal_id: &str) -> Result<Vec<RepoWithWorkspaces>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| list_repos_for_user_sync(conn, &principal_id))
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
            let by_id = workspace_by_id_for_user(conn, &workspace_handle, &principal_id)?;
            if by_id.is_some() {
                return Ok(by_id);
            }
            workspace_by_slug_for_user(conn, &workspace_handle, &principal_id)
        })
        .await
    }
}

fn upsert_repo_row(
    tx: &Transaction<'_>,
    principal_id: &str,
    owner: &str,
    name: &str,
    default_ref: &str,
    now: &str,
) -> Result<String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM repos WHERE principal_id = ?1 AND owner = ?2 AND name = ?3",
            params![principal_id, owner, name],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)?;

    if let Some(repo_id) = existing {
        tx.execute(
            "UPDATE repos SET default_ref = ?1, updated_at = ?2, last_discovered_at = ?3
             WHERE id = ?4",
            params![default_ref, now, now, repo_id.as_str()],
        )
        .map_err(db_err)?;
        return Ok(repo_id);
    }

    let repo_id = new_id();
    tx.execute(
        "INSERT INTO repos (
           id, principal_id, owner, name, default_ref,
           created_at, updated_at, last_discovered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            repo_id,
            principal_id,
            owner,
            name,
            default_ref,
            now,
            now,
            now
        ],
    )
    .map_err(db_err)?;
    Ok(repo_id)
}

fn upsert_discovered_workspaces(
    tx: &Transaction<'_>,
    repo_id: &str,
    owner: &str,
    name: &str,
    workspaces: &[DiscoveredWorkspaceInput],
    now: &str,
) -> Result<HashSet<WorkspaceKey>> {
    let mut discovered_keys = HashSet::with_capacity(workspaces.len());

    for workspace in workspaces {
        discovered_keys.insert(WorkspaceKey::from_discovered(workspace));
        upsert_workspace_row(tx, repo_id, owner, name, workspace, now)?;
    }

    Ok(discovered_keys)
}

fn upsert_workspace_row(
    tx: &Transaction<'_>,
    repo_id: &str,
    owner: &str,
    name: &str,
    workspace: &DiscoveredWorkspaceInput,
    now: &str,
) -> Result<()> {
    let updated = tx
        .execute(
            "UPDATE workspaces
             SET owner = ?1, name = ?2, source = ?3, discovered_at = ?4, active = 1
             WHERE repo_id = ?5 AND path = ?6 AND ref_ = ?7",
            params![
                owner,
                name,
                workspace.source.as_str(),
                now,
                repo_id,
                workspace.path.as_str(),
                workspace.git_ref.as_str(),
            ],
        )
        .map_err(db_err)?;

    if updated != 0 {
        return Ok(());
    }

    let workspace_id = new_id();
    tx.execute(
        "INSERT INTO workspaces (
           id, repo_id, owner, name, path, ref_, source, discovered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            workspace_id,
            repo_id,
            owner,
            name,
            workspace.path.as_str(),
            workspace.git_ref.as_str(),
            workspace.source.as_str(),
            now,
        ],
    )
    .map_err(db_err)?;
    Ok(())
}

fn cleanup_missing_workspaces(
    tx: &Transaction<'_>,
    repo_id: &str,
    discovered_keys: &HashSet<WorkspaceKey>,
) -> Result<()> {
    for workspace in workspace_keys_for_repo(tx, repo_id)? {
        if discovered_keys.contains(&workspace.key) {
            continue;
        }
        delete_or_deactivate_workspace(tx, &workspace.id)?;
    }
    Ok(())
}

fn workspace_keys_for_repo(tx: &Transaction<'_>, repo_id: &str) -> Result<Vec<WorkspaceRowKey>> {
    let mut statement = tx
        .prepare("SELECT id, path, ref_ FROM workspaces WHERE repo_id = ?1")
        .map_err(db_err)?;
    statement
        .query_map(params![repo_id], |row| {
            Ok(WorkspaceRowKey {
                id: row.get(0)?,
                key: WorkspaceKey {
                    path: row.get(1)?,
                    git_ref: row.get(2)?,
                },
            })
        })
        .map_err(db_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_err)
}

fn delete_or_deactivate_workspace(tx: &Transaction<'_>, workspace_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM workspaces
         WHERE id = ?1
           AND NOT EXISTS (
             SELECT 1 FROM draft_sessions WHERE workspace_id = ?1
           )",
        params![workspace_id],
    )
    .map_err(db_err)?;
    tx.execute(
        "UPDATE workspaces SET active = 0 WHERE id = ?1",
        params![workspace_id],
    )
    .map_err(db_err)?;
    Ok(())
}

fn list_repos_for_user_sync(
    conn: &Connection,
    principal_id: &str,
) -> Result<Vec<RepoWithWorkspaces>> {
    list_repo_ids_for_user(conn, principal_id)?
        .into_iter()
        .map(|id| {
            repo_with_workspaces_by_id(conn, &id, principal_id)?
                .ok_or_else(|| RototoError::new("repo listing failed"))
        })
        .collect()
}

fn list_repo_ids_for_user(conn: &Connection, principal_id: &str) -> Result<Vec<String>> {
    let mut statement = conn
        .prepare(
            "SELECT id FROM repos WHERE principal_id = ?1
             ORDER BY updated_at DESC, owner ASC, name ASC",
        )
        .map_err(db_err)?;
    statement
        .query_map(params![principal_id], |row| row.get(0))
        .map_err(db_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_err)
}

fn workspace_by_id_for_user(
    conn: &Connection,
    workspace_id: &str,
    principal_id: &str,
) -> Result<Option<WorkspaceRecord>> {
    conn.query_row(
        "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
         FROM workspaces w
         INNER JOIN repos r ON r.id = w.repo_id
         WHERE w.id = ?1 AND r.principal_id = ?2",
        params![workspace_id, principal_id],
        workspace_from_row,
    )
    .optional()
    .map_err(db_err)
}

fn workspace_by_slug_for_user(
    conn: &Connection,
    slug: &str,
    principal_id: &str,
) -> Result<Option<WorkspaceRecord>> {
    let active_match =
        find_workspace_by_slug(list_workspaces_for_user_sync(conn, principal_id)?, slug);
    if active_match.is_some() {
        return Ok(active_match);
    }

    Ok(find_workspace_by_slug(
        list_all_workspaces_for_user_sync(conn, principal_id)?,
        slug,
    ))
}

fn find_workspace_by_slug(workspaces: Vec<WorkspaceRecord>, slug: &str) -> Option<WorkspaceRecord> {
    workspaces
        .into_iter()
        .find(|workspace| workspace.slug == slug)
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
            repo_from_row,
        )
        .optional()
        .map_err(db_err)?;
    let Some(repo) = repo else {
        return Ok(None);
    };
    let workspaces = active_workspaces_for_repo(conn, &repo.id)?;
    Ok(Some(RepoWithWorkspaces { repo, workspaces }))
}

fn active_workspaces_for_repo(conn: &Connection, repo_id: &str) -> Result<Vec<WorkspaceRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT id, repo_id, owner, name, path, ref_, source, discovered_at
             FROM workspaces
             WHERE repo_id = ?1 AND active = 1
             ORDER BY path ASC",
        )
        .map_err(db_err)?;
    statement
        .query_map(params![repo_id], workspace_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)
}
