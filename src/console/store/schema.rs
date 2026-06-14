use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{Result, RototoError};

use super::util::db_err;

const CURRENT_SCHEMA_VERSION: i32 = 1;
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

fn migrate_one_schema_version(_conn: &Connection, version: i32) -> Result<i32> {
    Err(RototoError::new(format!(
        "missing console database migration from schema version {version}"
    )))
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
