use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{Result, RototoError};

use super::util::db_err;

const CURRENT_SCHEMA_VERSION: i32 = 4;
const BASELINE_SCHEMA_VERSION: i32 = 1;

pub(super) fn initialize_schema(conn: &Connection) -> Result<()> {
    configure_connection(conn)?;
    migrate_schema(conn)
}

fn configure_connection(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        "#,
    )
    .map_err(|err| RototoError::new(format!("failed to configure console database: {err}")))
}

fn migrate_schema(conn: &Connection) -> Result<()> {
    let version = schema_version(conn)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(RototoError::new(format!(
            "console database schema version {version} is newer than this rototo binary supports ({CURRENT_SCHEMA_VERSION})"
        )));
    }
    if version == CURRENT_SCHEMA_VERSION {
        return Ok(());
    }

    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(db_err)?;
    let mut version = schema_version(&tx)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(RototoError::new(format!(
            "console database schema version {version} is newer than this rototo binary supports ({CURRENT_SCHEMA_VERSION})"
        )));
    }

    if version == 0 {
        create_schema_v1(&tx)?;
        baseline_legacy_schema(&tx)?;
        set_schema_version(&tx, BASELINE_SCHEMA_VERSION)?;
        version = BASELINE_SCHEMA_VERSION;
    }

    while version < CURRENT_SCHEMA_VERSION {
        version = migrate_one_schema_version(&tx, version)?;
        set_schema_version(&tx, version)?;
    }

    tx.commit().map_err(db_err)?;
    Ok(())
}

fn create_schema_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
          id TEXT PRIMARY KEY,
          principal_id TEXT NOT NULL,
          github_login TEXT NOT NULL,
          github_name TEXT,
          github_avatar_url TEXT,
          github_token_ciphertext TEXT NOT NULL,
          created_at TEXT NOT NULL,
          expires_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS oauth_states (
          state TEXT PRIMARY KEY,
          created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS repos (
          id TEXT PRIMARY KEY,
          principal_id TEXT NOT NULL,
          owner TEXT NOT NULL,
          name TEXT NOT NULL,
          default_ref TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          last_discovered_at TEXT,
          UNIQUE(principal_id, owner, name)
        );

        CREATE TABLE IF NOT EXISTS workspaces (
          id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          owner TEXT NOT NULL,
          name TEXT NOT NULL,
          path TEXT NOT NULL,
          ref_ TEXT NOT NULL,
          source TEXT NOT NULL,
          discovered_at TEXT NOT NULL,
          active INTEGER NOT NULL DEFAULT 1,
          UNIQUE(repo_id, path, ref_),
          FOREIGN KEY(repo_id) REFERENCES repos(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS draft_sessions (
          id TEXT PRIMARY KEY,
          workspace_id TEXT NOT NULL,
          principal_id TEXT NOT NULL,
          branch TEXT NOT NULL,
          base_ref TEXT NOT NULL,
          status TEXT NOT NULL,
          pr_url TEXT,
          pr_number INTEGER,
          pr_state TEXT,
          pr_merged_at TEXT,
          pr_synced_at TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          published_at TEXT,
          FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS draft_changes (
          id TEXT PRIMARY KEY,
          draft_id TEXT NOT NULL,
          file_path TEXT NOT NULL,
          variable_id TEXT NOT NULL,
          value_key TEXT NOT NULL,
          before_json TEXT NOT NULL,
          after_json TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          UNIQUE(draft_id, variable_id, value_key),
          FOREIGN KEY(draft_id) REFERENCES draft_sessions(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS draft_events (
          id TEXT PRIMARY KEY,
          draft_id TEXT NOT NULL,
          kind TEXT NOT NULL,
          summary TEXT NOT NULL,
          detail_json TEXT,
          created_at TEXT NOT NULL,
          FOREIGN KEY(draft_id) REFERENCES draft_sessions(id) ON DELETE CASCADE
        );
        "#,
    )
    .map_err(|err| {
        RototoError::new(format!(
            "failed to initialize console database schema: {err}"
        ))
    })?;
    Ok(())
}

fn baseline_legacy_schema(conn: &Connection) -> Result<()> {
    ensure_column(conn, "workspaces", "active", "INTEGER NOT NULL DEFAULT 1")?;
    Ok(())
}

fn migrate_one_schema_version(conn: &Connection, version: i32) -> Result<i32> {
    match version {
        1 => {
            migrate_schema_v2(conn)?;
            Ok(2)
        }
        2 => {
            migrate_schema_v3(conn)?;
            Ok(3)
        }
        3 => {
            migrate_schema_v4(conn)?;
            Ok(4)
        }
        _ => Err(RototoError::new(format!(
            "missing console database migration from schema version {version}"
        ))),
    }
}

fn migrate_schema_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS tracked_branches (
          id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          principal_id TEXT NOT NULL,
          branch TEXT NOT NULL,
          base_ref TEXT NOT NULL,
          base_commit TEXT,
          pr_url TEXT,
          pr_number INTEGER,
          pr_state TEXT,
          pr_merged_at TEXT,
          pr_synced_at TEXT,
          last_selected_workspace_path TEXT,
          last_seen_commit TEXT,
          status TEXT NOT NULL,
          created_at TEXT NOT NULL,
          last_opened_at TEXT NOT NULL,
          last_edited_at TEXT,
          archived_at TEXT,
          UNIQUE(repo_id, principal_id, branch),
          FOREIGN KEY(repo_id) REFERENCES repos(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS tracked_branch_workspaces (
          branch_id TEXT NOT NULL,
          workspace_id TEXT NOT NULL,
          added_at TEXT NOT NULL,
          PRIMARY KEY(branch_id, workspace_id),
          FOREIGN KEY(branch_id) REFERENCES tracked_branches(id) ON DELETE CASCADE,
          FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        );
        "#,
    )
    .map_err(db_err)?;
    Ok(())
}

fn migrate_schema_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS draft_workspaces (
          draft_id TEXT NOT NULL,
          workspace_id TEXT NOT NULL,
          added_at TEXT NOT NULL,
          PRIMARY KEY(draft_id, workspace_id),
          FOREIGN KEY(draft_id) REFERENCES draft_sessions(id) ON DELETE CASCADE,
          FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        );

        INSERT OR IGNORE INTO draft_workspaces (draft_id, workspace_id, added_at)
        SELECT id, workspace_id, created_at
        FROM draft_sessions;
        "#,
    )
    .map_err(db_err)?;
    Ok(())
}

fn migrate_schema_v2(conn: &Connection) -> Result<()> {
    /// Snapshot of the pre-v2 `draft_changes` row shape.
    ///
    /// The migration reads every legacy row into this adapter, derives the new
    /// `target_path`, writes the replacement table, and then drops the legacy
    /// shape. It is never used at runtime after schema initialization.
    struct LegacyDraftChange {
        id: String,
        draft_id: String,
        file_path: String,
        value_key: String,
        before_json: String,
        after_json: String,
        updated_at: String,
    }

    let changes = {
        let mut statement = conn
            .prepare(
                "SELECT id, draft_id, file_path, value_key, before_json, after_json, updated_at
                 FROM draft_changes",
            )
            .map_err(db_err)?;
        statement
            .query_map([], |row| {
                Ok(LegacyDraftChange {
                    id: row.get(0)?,
                    draft_id: row.get(1)?,
                    file_path: row.get(2)?,
                    value_key: row.get(3)?,
                    before_json: row.get(4)?,
                    after_json: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(db_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_err)?
    };

    conn.execute_batch(
        r#"
        ALTER TABLE draft_changes RENAME TO draft_changes_v1;

        CREATE TABLE draft_changes (
          id TEXT PRIMARY KEY,
          draft_id TEXT NOT NULL,
          file_path TEXT NOT NULL,
          target_path TEXT,
          before_json TEXT NOT NULL,
          after_json TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(draft_id) REFERENCES draft_sessions(id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX draft_changes_target_idx
        ON draft_changes(draft_id, file_path, COALESCE(target_path, ''));
        "#,
    )
    .map_err(db_err)?;

    for change in changes {
        conn.execute(
            "INSERT INTO draft_changes (
               id, draft_id, file_path, target_path, before_json, after_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                change.id,
                change.draft_id,
                change.file_path,
                value_target_path(&change.value_key),
                change.before_json,
                change.after_json,
                change.updated_at,
            ],
        )
        .map_err(db_err)?;
    }

    conn.execute_batch("DROP TABLE draft_changes_v1")
        .map_err(db_err)?;
    Ok(())
}

fn schema_version(conn: &Connection) -> Result<i32> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(db_err)
}

fn set_schema_version(conn: &Connection, version: i32) -> Result<()> {
    conn.execute_batch(&format!("PRAGMA user_version = {version}"))
        .map_err(db_err)
}

fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(db_err)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_err)?;
    if columns.iter().any(|existing| existing == column) {
        return Ok(());
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"),
        [],
    )
    .map_err(db_err)?;
    Ok(())
}

fn value_target_path(value_key: &str) -> String {
    format!("/values/{}", json_pointer_escape(value_key))
}

fn json_pointer_escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}
