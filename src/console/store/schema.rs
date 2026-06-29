use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{Result, RototoError};

use super::util::db_err;

const CURRENT_SCHEMA_VERSION: i32 = 8;

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
    if version != 0 {
        return Err(RototoError::new(format!(
            "console database schema version {version} is not supported; remove the console data directory and start from a fresh source tree"
        )));
    }

    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(db_err)?;
    let version = schema_version(&tx)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(RototoError::new(format!(
            "console database schema version {version} is newer than this rototo binary supports ({CURRENT_SCHEMA_VERSION})"
        )));
    }
    if version != 0 {
        return Err(RototoError::new(format!(
            "console database schema version {version} is not supported; remove the console data directory and start from a fresh source tree"
        )));
    }
    create_schema_v8(&tx)?;
    set_schema_version(&tx, CURRENT_SCHEMA_VERSION)?;

    tx.commit().map_err(db_err)?;
    Ok(())
}

fn create_schema_v8(conn: &Connection) -> Result<()> {
    create_auth_tables(conn)?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS source_trees (
          id TEXT PRIMARY KEY,
          principal_id TEXT NOT NULL,
          kind TEXT NOT NULL,
          source TEXT NOT NULL,
          display_name TEXT NOT NULL,
          default_revision TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          last_discovered_at TEXT,
          UNIQUE(principal_id, source)
        );

        CREATE TABLE IF NOT EXISTS source_tree_packages (
          id TEXT PRIMARY KEY,
          source_tree_id TEXT NOT NULL,
          path TEXT NOT NULL,
          revision TEXT NOT NULL,
          source_tree_label TEXT NOT NULL,
          source TEXT NOT NULL,
          discovered_at TEXT NOT NULL,
          active INTEGER NOT NULL DEFAULT 1,
          UNIQUE(source_tree_id, path, revision),
          FOREIGN KEY(source_tree_id) REFERENCES source_trees(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS active_branches (
          id TEXT PRIMARY KEY,
          source_tree_id TEXT NOT NULL,
          principal_id TEXT NOT NULL,
          branch TEXT NOT NULL,
          base_ref TEXT NOT NULL,
          base_commit TEXT,
          pr_url TEXT,
          pr_number INTEGER,
          pr_state TEXT,
          pr_merged_at TEXT,
          pr_synced_at TEXT,
          last_selected_package_path TEXT,
          last_seen_commit TEXT,
          status TEXT NOT NULL,
          created_at TEXT NOT NULL,
          last_opened_at TEXT NOT NULL,
          last_edited_at TEXT,
          archived_at TEXT,
          UNIQUE(source_tree_id, principal_id, branch),
          FOREIGN KEY(source_tree_id) REFERENCES source_trees(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS active_branch_packages (
          branch_id TEXT NOT NULL,
          package_path TEXT NOT NULL,
          added_at TEXT NOT NULL,
          PRIMARY KEY(branch_id, package_path),
          FOREIGN KEY(branch_id) REFERENCES active_branches(id) ON DELETE CASCADE
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

fn create_auth_tables(conn: &Connection) -> Result<()> {
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
        "#,
    )
    .map_err(|err| {
        RototoError::new(format!(
            "failed to initialize console database schema: {err}"
        ))
    })?;
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
