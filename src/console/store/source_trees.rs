use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::console::time::now_iso;
use crate::error::{Result, RototoError};

use super::Store;
use super::rows::{source_tree_from_row, workspace_from_row};
use super::types::{
    DiscoveredWorkspaceInput, RegisterSourceTreeInput, SourceTreeWithWorkspaces, WorkspaceRecord,
};
use super::util::{db_err, new_id};

/// Stable identity for a discovered workspace within one source tree.
///
/// Discovery can produce rows in any order, so cleanup compares by workspace
/// path and git ref instead of row id. The key lives only during one discovery
/// transaction.
#[derive(Hash, PartialEq, Eq)]
struct WorkspaceKey {
    path: String,
    git_ref: String,
}

/// Existing workspace row paired with its discovery identity.
///
/// The cleanup step uses the row id for updates/deletes and the key for set
/// membership against the newly discovered workspaces.
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
    pub async fn upsert_source_tree_with_workspaces(
        &self,
        input: RegisterSourceTreeInput,
    ) -> Result<SourceTreeWithWorkspaces> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let tx = conn.unchecked_transaction().map_err(db_err)?;

            let source_tree_id = upsert_source_tree_row(&tx, &input, &now)?;
            let discovered_keys = upsert_discovered_workspaces(
                &tx,
                &source_tree_id,
                &input.workspace_owner,
                &input.workspace_name,
                &input.workspaces,
                &now,
            )?;
            cleanup_missing_workspaces(&tx, &source_tree_id, &discovered_keys)?;

            tx.commit().map_err(db_err)?;

            source_tree_with_workspaces_by_id(conn, &source_tree_id, &input.principal_id)?
                .ok_or_else(|| RototoError::new("source tree registration failed"))
        })
        .await
    }

    pub async fn list_source_trees_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<SourceTreeWithWorkspaces>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| list_source_trees_for_user_sync(conn, &principal_id))
            .await
    }

    pub async fn get_source_tree_for_user(
        &self,
        source_tree_id: &str,
        principal_id: &str,
    ) -> Result<Option<SourceTreeWithWorkspaces>> {
        let source_tree_id = source_tree_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            source_tree_with_workspaces_by_id(conn, &source_tree_id, &principal_id)
        })
        .await
    }

    pub async fn delete_source_tree_for_user(
        &self,
        source_tree_id: &str,
        principal_id: &str,
    ) -> Result<bool> {
        let source_tree_id = source_tree_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            if source_tree_with_workspaces_by_id(conn, &source_tree_id, &principal_id)?.is_none() {
                return Ok(false);
            }
            // ON DELETE CASCADE clears workspaces and active branch
            // selections transitively.
            conn.execute(
                "DELETE FROM source_trees WHERE id = ?1",
                params![source_tree_id],
            )
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

fn upsert_source_tree_row(
    tx: &Transaction<'_>,
    input: &RegisterSourceTreeInput,
    now: &str,
) -> Result<String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM source_trees WHERE principal_id = ?1 AND source = ?2",
            params![input.principal_id, input.source],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)?;

    if let Some(source_tree_id) = existing {
        tx.execute(
            "UPDATE source_trees
             SET kind = ?1,
                 display_name = ?2,
                 default_revision = ?3,
                 updated_at = ?4,
                 last_discovered_at = ?5
             WHERE id = ?6",
            params![
                input.kind.as_str(),
                input.display_name,
                input.default_revision,
                now,
                now,
                source_tree_id.as_str()
            ],
        )
        .map_err(db_err)?;
        return Ok(source_tree_id);
    }

    let source_tree_id = new_id();
    tx.execute(
        "INSERT INTO source_trees (
           id, principal_id, kind, source, display_name, default_revision,
           created_at, updated_at, last_discovered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            source_tree_id,
            input.principal_id,
            input.kind.as_str(),
            input.source,
            input.display_name,
            input.default_revision,
            now,
            now,
            now
        ],
    )
    .map_err(db_err)?;
    Ok(source_tree_id)
}

fn upsert_discovered_workspaces(
    tx: &Transaction<'_>,
    source_tree_id: &str,
    owner: &str,
    name: &str,
    workspaces: &[DiscoveredWorkspaceInput],
    now: &str,
) -> Result<HashSet<WorkspaceKey>> {
    let mut discovered_keys = HashSet::with_capacity(workspaces.len());

    for workspace in workspaces {
        discovered_keys.insert(WorkspaceKey::from_discovered(workspace));
        upsert_workspace_row(tx, source_tree_id, owner, name, workspace, now)?;
    }

    Ok(discovered_keys)
}

fn upsert_workspace_row(
    tx: &Transaction<'_>,
    source_tree_id: &str,
    owner: &str,
    name: &str,
    workspace: &DiscoveredWorkspaceInput,
    now: &str,
) -> Result<()> {
    let updated = tx
        .execute(
            "UPDATE source_tree_workspaces
             SET owner = ?1, name = ?2, source = ?3, discovered_at = ?4, active = 1
             WHERE source_tree_id = ?5 AND path = ?6 AND ref_ = ?7",
            params![
                owner,
                name,
                workspace.source.as_str(),
                now,
                source_tree_id,
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
        "INSERT INTO source_tree_workspaces (
           id, source_tree_id, owner, name, path, ref_, source, discovered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            workspace_id,
            source_tree_id,
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
    source_tree_id: &str,
    discovered_keys: &HashSet<WorkspaceKey>,
) -> Result<()> {
    for workspace in workspace_keys_for_source_tree(tx, source_tree_id)? {
        if discovered_keys.contains(&workspace.key) {
            continue;
        }
        delete_or_deactivate_workspace(tx, &workspace.id)?;
    }
    Ok(())
}

fn workspace_keys_for_source_tree(
    tx: &Transaction<'_>,
    source_tree_id: &str,
) -> Result<Vec<WorkspaceRowKey>> {
    let mut statement = tx
        .prepare("SELECT id, path, ref_ FROM source_tree_workspaces WHERE source_tree_id = ?1")
        .map_err(db_err)?;
    statement
        .query_map(params![source_tree_id], |row| {
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
        "DELETE FROM source_tree_workspaces
         WHERE id = ?1
           AND NOT EXISTS (
             SELECT 1
             FROM active_branch_workspaces abw
             INNER JOIN active_branches b ON b.id = abw.branch_id
             WHERE b.source_tree_id = source_tree_workspaces.source_tree_id
               AND abw.workspace_path = source_tree_workspaces.path
           )",
        params![workspace_id],
    )
    .map_err(db_err)?;
    tx.execute(
        "UPDATE source_tree_workspaces SET active = 0 WHERE id = ?1",
        params![workspace_id],
    )
    .map_err(db_err)?;
    Ok(())
}

fn list_source_trees_for_user_sync(
    conn: &Connection,
    principal_id: &str,
) -> Result<Vec<SourceTreeWithWorkspaces>> {
    list_source_tree_ids_for_user(conn, principal_id)?
        .into_iter()
        .map(|id| {
            source_tree_with_workspaces_by_id(conn, &id, principal_id)?
                .ok_or_else(|| RototoError::new("source tree listing failed"))
        })
        .collect()
}

fn list_source_tree_ids_for_user(conn: &Connection, principal_id: &str) -> Result<Vec<String>> {
    let mut statement = conn
        .prepare(
            "SELECT id FROM source_trees WHERE principal_id = ?1
             ORDER BY updated_at DESC, display_name ASC, source ASC",
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
        "SELECT w.id, w.source_tree_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
         FROM source_tree_workspaces w
         INNER JOIN source_trees r ON r.id = w.source_tree_id
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
            "SELECT w.id, w.source_tree_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
             FROM source_tree_workspaces w
             INNER JOIN source_trees r ON r.id = w.source_tree_id
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
            "SELECT w.id, w.source_tree_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
             FROM source_tree_workspaces w
             INNER JOIN source_trees r ON r.id = w.source_tree_id
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

fn source_tree_with_workspaces_by_id(
    conn: &Connection,
    source_tree_id: &str,
    principal_id: &str,
) -> Result<Option<SourceTreeWithWorkspaces>> {
    let source_tree = conn
        .query_row(
            "SELECT id, principal_id, kind, source, display_name, default_revision,
                    created_at, updated_at, last_discovered_at
             FROM source_trees WHERE id = ?1 AND principal_id = ?2",
            params![source_tree_id, principal_id],
            source_tree_from_row,
        )
        .optional()
        .map_err(db_err)?;
    let Some(source_tree) = source_tree else {
        return Ok(None);
    };
    let workspaces = active_workspaces_for_source_tree(conn, &source_tree.id)?;
    Ok(Some(SourceTreeWithWorkspaces {
        source_tree,
        workspaces,
    }))
}

fn active_workspaces_for_source_tree(
    conn: &Connection,
    source_tree_id: &str,
) -> Result<Vec<WorkspaceRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT id, source_tree_id, owner, name, path, ref_, source, discovered_at
             FROM source_tree_workspaces
             WHERE source_tree_id = ?1 AND active = 1
             ORDER BY path ASC",
        )
        .map_err(db_err)?;
    statement
        .query_map(params![source_tree_id], workspace_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)
}
