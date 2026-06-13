use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{Result, RototoError};

use super::time::{now_iso, now_iso_plus};
use super::token_crypto::TokenCrypto;

const SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 14);
const OAUTH_STATE_TTL: Duration = Duration::from_secs(60 * 10);
const SESSION_TOKEN_BYTES: usize = 32;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub session_hash: String,
    pub github_user_id: String,
    pub github_login: String,
    pub github_name: Option<String>,
    pub github_avatar_url: Option<String>,
    #[serde(skip)]
    pub github_token: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRecord {
    pub id: String,
    pub github_user_id: String,
    pub owner: String,
    pub name: String,
    pub default_ref: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_discovered_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRecord {
    pub id: String,
    /// Derived, human-readable URL handle (repo name + workspace path).
    /// Stable across re-discovery, unlike the row id.
    pub slug: String,
    pub repo_id: String,
    pub owner: String,
    pub name: String,
    pub path: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub source: String,
    pub discovered_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RepoWithWorkspaces {
    #[serde(flatten)]
    pub repo: RepoRecord,
    pub workspaces: Vec<WorkspaceRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftStatus {
    Open,
    Published,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSessionRecord {
    pub id: String,
    pub workspace_id: String,
    pub github_user_id: String,
    pub branch: String,
    pub base_ref: String,
    pub status: DraftStatus,
    pub pr_url: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_state: Option<String>,
    pub pr_merged_at: Option<String>,
    pub pr_synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub published_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftChangeRecord {
    pub id: String,
    pub draft_id: String,
    pub file_path: String,
    pub variable_id: String,
    pub value_key: String,
    pub before_json: String,
    pub after_json: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftEventRecord {
    pub id: String,
    pub draft_id: String,
    pub kind: String,
    pub summary: String,
    pub detail_json: Option<String>,
    pub created_at: String,
}

pub struct NewSession {
    pub github_user_id: String,
    pub github_login: String,
    pub github_name: Option<String>,
    pub github_avatar_url: Option<String>,
    pub github_token: String,
}

pub struct NewDraftSession {
    pub workspace_id: String,
    pub github_user_id: String,
    pub branch: String,
    pub base_ref: String,
}

pub struct DraftChangeInput {
    pub draft_id: String,
    pub file_path: String,
    pub variable_id: String,
    pub value_key: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
}

pub struct DraftEventInput {
    pub draft_id: String,
    pub kind: String,
    pub summary: String,
    pub detail: Option<serde_json::Value>,
}

pub struct PullRequestStateInput {
    pub draft_id: String,
    pub pr_number: i64,
    pub pr_state: String,
    pub pr_url: String,
    pub pr_merged_at: Option<String>,
}

pub struct DiscoveredWorkspaceInput {
    pub path: String,
    pub git_ref: String,
    pub source: String,
}

/// SQLite-backed console state. All public methods are async and run their
/// statements on the blocking pool; the connection itself is serialized
/// behind a mutex, which is enough for the console's small write volume.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    crypto: TokenCrypto,
}

impl Store {
    pub fn open(path: &Path, crypto: TokenCrypto) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|err| RototoError::new(format!("failed to open console database: {err}")))?;
        Self::initialize(conn, crypto)
    }

    #[cfg(test)]
    pub fn open_in_memory(crypto: TokenCrypto) -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|err| {
            RototoError::new(format!("failed to open in-memory console database: {err}"))
        })?;
        Self::initialize(conn, crypto)
    }

    fn initialize(conn: Connection, crypto: TokenCrypto) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS sessions (
              id TEXT PRIMARY KEY,
              github_user_id TEXT NOT NULL,
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
              github_user_id TEXT NOT NULL,
              owner TEXT NOT NULL,
              name TEXT NOT NULL,
              default_ref TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              last_discovered_at TEXT,
              UNIQUE(github_user_id, owner, name)
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
              UNIQUE(repo_id, path, ref_),
              FOREIGN KEY(repo_id) REFERENCES repos(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS draft_sessions (
              id TEXT PRIMARY KEY,
              workspace_id TEXT NOT NULL,
              github_user_id TEXT NOT NULL,
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
        .map_err(|err| RototoError::new(format!("failed to initialize console database: {err}")))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            crypto,
        })
    }

    async fn with_conn<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &TokenCrypto) -> Result<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        let crypto = self.crypto.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| RototoError::new("console database lock was poisoned"))?;
            work(&conn, &crypto)
        })
        .await
        .map_err(|err| RototoError::new(format!("console database task failed: {err}")))?
    }

    pub async fn create_session(&self, input: NewSession) -> Result<String> {
        self.with_conn(move |conn, crypto| {
            let session_token = new_session_token()?;
            let now = now_iso();
            let expires_at = now_iso_plus(SESSION_TTL);
            conn.execute(
                "INSERT INTO sessions (
                   id, github_user_id, github_login, github_name, github_avatar_url,
                   github_token_ciphertext, created_at, expires_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session_token_hash(&session_token),
                    input.github_user_id,
                    input.github_login,
                    input.github_name,
                    input.github_avatar_url,
                    crypto.encrypt(&input.github_token)?,
                    now,
                    expires_at,
                ],
            )
            .map_err(db_err)?;
            Ok(session_token)
        })
        .await
    }

    pub async fn get_session(&self, session_token: &str) -> Result<Option<SessionUser>> {
        let session_token = session_token.to_owned();
        self.with_conn(move |conn, crypto| {
            let hash = session_token_hash(&session_token);
            let row = conn
                .query_row(
                    "SELECT id, github_user_id, github_login, github_name, github_avatar_url,
                            github_token_ciphertext, expires_at
                     FROM sessions WHERE id = ?1",
                    params![hash],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_err)?;
            let Some((id, user_id, login, name, avatar, ciphertext, expires_at)) = row else {
                return Ok(None);
            };
            if expires_at.as_str() <= now_iso().as_str() {
                conn.execute("DELETE FROM sessions WHERE id = ?1", params![hash])
                    .map_err(db_err)?;
                return Ok(None);
            }
            let Ok(github_token) = crypto.decrypt(&ciphertext) else {
                return Ok(None);
            };
            Ok(Some(SessionUser {
                session_hash: id,
                github_user_id: user_id,
                github_login: login,
                github_name: name,
                github_avatar_url: avatar,
                github_token,
            }))
        })
        .await
    }

    pub async fn delete_session(&self, session_token: &str) -> Result<()> {
        let session_token = session_token.to_owned();
        self.with_conn(move |conn, _| {
            conn.execute(
                "DELETE FROM sessions WHERE id = ?1",
                params![session_token_hash(&session_token)],
            )
            .map_err(db_err)?;
            Ok(())
        })
        .await
    }

    pub async fn create_oauth_state(&self, state: &str) -> Result<()> {
        let state = state.to_owned();
        self.with_conn(move |conn, _| {
            conn.execute(
                "INSERT OR REPLACE INTO oauth_states (state, created_at) VALUES (?1, ?2)",
                params![state, now_iso()],
            )
            .map_err(db_err)?;
            Ok(())
        })
        .await
    }

    pub async fn consume_oauth_state(&self, state: &str) -> Result<bool> {
        let state = state.to_owned();
        self.with_conn(move |conn, _| {
            let created_at: Option<String> = conn
                .query_row(
                    "SELECT created_at FROM oauth_states WHERE state = ?1",
                    params![state],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_err)?;
            conn.execute("DELETE FROM oauth_states WHERE state = ?1", params![state])
                .map_err(db_err)?;
            let Some(created_at) = created_at else {
                return Ok(false);
            };
            Ok(created_at.as_str() > now_iso_minus_state_ttl().as_str())
        })
        .await
    }

    pub async fn upsert_repo_with_workspaces(
        &self,
        github_user_id: String,
        owner: String,
        name: String,
        default_ref: String,
        workspaces: Vec<DiscoveredWorkspaceInput>,
    ) -> Result<RepoWithWorkspaces> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM repos WHERE github_user_id = ?1 AND owner = ?2 AND name = ?3",
                    params![github_user_id, owner, name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_err)?;
            let repo_id = match existing {
                Some(repo_id) => {
                    conn.execute(
                        "UPDATE repos SET default_ref = ?1, updated_at = ?2, last_discovered_at = ?3
                         WHERE id = ?4",
                        params![default_ref, now, now, repo_id],
                    )
                    .map_err(db_err)?;
                    repo_id
                }
                None => {
                    let repo_id = new_id();
                    conn.execute(
                        "INSERT INTO repos (
                           id, github_user_id, owner, name, default_ref,
                           created_at, updated_at, last_discovered_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            repo_id,
                            github_user_id,
                            owner,
                            name,
                            default_ref,
                            now,
                            now,
                            now
                        ],
                    )
                    .map_err(db_err)?;
                    repo_id
                }
            };

            conn.execute(
                "DELETE FROM workspaces WHERE repo_id = ?1",
                params![repo_id],
            )
            .map_err(db_err)?;
            for workspace in workspaces {
                conn.execute(
                    "INSERT INTO workspaces (
                       id, repo_id, owner, name, path, ref_, source, discovered_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        new_id(),
                        repo_id,
                        owner,
                        name,
                        workspace.path,
                        workspace.git_ref,
                        workspace.source,
                        now,
                    ],
                )
                .map_err(db_err)?;
            }

            repo_with_workspaces_by_id(conn, &repo_id, &github_user_id)?
                .ok_or_else(|| RototoError::new("repo registration failed"))
        })
        .await
    }

    pub async fn list_repos_for_user(
        &self,
        github_user_id: &str,
    ) -> Result<Vec<RepoWithWorkspaces>> {
        let github_user_id = github_user_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT id FROM repos WHERE github_user_id = ?1
                     ORDER BY updated_at DESC, owner ASC, name ASC",
                )
                .map_err(db_err)?;
            let ids: Vec<String> = statement
                .query_map(params![github_user_id], |row| row.get(0))
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            ids.iter()
                .map(|id| {
                    repo_with_workspaces_by_id(conn, id, &github_user_id)?
                        .ok_or_else(|| RototoError::new("repo listing failed"))
                })
                .collect()
        })
        .await
    }

    pub async fn delete_repo_for_user(&self, repo_id: &str, github_user_id: &str) -> Result<bool> {
        let repo_id = repo_id.to_owned();
        let github_user_id = github_user_id.to_owned();
        self.with_conn(move |conn, _| {
            if repo_with_workspaces_by_id(conn, &repo_id, &github_user_id)?.is_none() {
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
        github_user_id: &str,
    ) -> Result<Vec<WorkspaceRecord>> {
        let github_user_id = github_user_id.to_owned();
        self.with_conn(move |conn, _| list_workspaces_for_user_sync(conn, &github_user_id))
            .await
    }

    /// Accepts the row id or the derived slug, so friendly URLs and older id
    /// URLs both resolve.
    pub async fn get_workspace_for_user(
        &self,
        workspace_handle: &str,
        github_user_id: &str,
    ) -> Result<Option<WorkspaceRecord>> {
        let workspace_handle = workspace_handle.to_owned();
        let github_user_id = github_user_id.to_owned();
        self.with_conn(move |conn, _| {
            let by_id = conn
                .query_row(
                    "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
                     FROM workspaces w
                     INNER JOIN repos r ON r.id = w.repo_id
                     WHERE w.id = ?1 AND r.github_user_id = ?2",
                    params![workspace_handle, github_user_id],
                    workspace_from_row,
                )
                .optional()
                .map_err(db_err)?;
            if by_id.is_some() {
                return Ok(by_id);
            }
            Ok(list_workspaces_for_user_sync(conn, &github_user_id)?
                .into_iter()
                .find(|workspace| workspace.slug == workspace_handle))
        })
        .await
    }

    pub async fn create_draft_session(&self, input: NewDraftSession) -> Result<DraftSessionRecord> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let id = new_id();
            conn.execute(
                "INSERT INTO draft_sessions (
                   id, workspace_id, github_user_id, branch, base_ref, status,
                   created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7)",
                params![
                    id,
                    input.workspace_id,
                    input.github_user_id,
                    input.branch,
                    input.base_ref,
                    now,
                    now,
                ],
            )
            .map_err(db_err)?;
            let draft = draft_session_by_id(conn, &id)?
                .ok_or_else(|| RototoError::new("draft session creation failed"))?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft.id.clone(),
                    kind: "draft.created".to_owned(),
                    summary: format!("Created draft branch {}", draft.branch),
                    detail: Some(serde_json::json!({
                        "branch": draft.branch,
                        "baseRef": draft.base_ref,
                    })),
                },
            )?;
            Ok(draft)
        })
        .await
    }

    pub async fn list_draft_sessions_for_workspace(
        &self,
        workspace_id: &str,
        github_user_id: &str,
    ) -> Result<Vec<DraftSessionRecord>> {
        let workspace_id = workspace_id.to_owned();
        let github_user_id = github_user_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT id, workspace_id, github_user_id, branch, base_ref, status,
                            pr_url, pr_number, pr_state, pr_merged_at, pr_synced_at,
                            created_at, updated_at, published_at
                     FROM draft_sessions
                     WHERE workspace_id = ?1 AND github_user_id = ?2
                     ORDER BY updated_at DESC",
                )
                .map_err(db_err)?;
            let drafts = statement
                .query_map(params![workspace_id, github_user_id], draft_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            Ok(drafts)
        })
        .await
    }

    pub async fn get_draft_session_for_user(
        &self,
        draft_id: &str,
        workspace_id: &str,
        github_user_id: &str,
    ) -> Result<Option<DraftSessionRecord>> {
        let draft_id = draft_id.to_owned();
        let workspace_id = workspace_id.to_owned();
        let github_user_id = github_user_id.to_owned();
        self.with_conn(move |conn, _| {
            conn.query_row(
                "SELECT id, workspace_id, github_user_id, branch, base_ref, status,
                        pr_url, pr_number, pr_state, pr_merged_at, pr_synced_at,
                        created_at, updated_at, published_at
                 FROM draft_sessions
                 WHERE id = ?1 AND workspace_id = ?2 AND github_user_id = ?3",
                params![draft_id, workspace_id, github_user_id],
                draft_from_row,
            )
            .optional()
            .map_err(db_err)
        })
        .await
    }

    /// Records one semantic change. Reverting a change back to its original
    /// value deletes the row and returns `None`.
    pub async fn record_draft_change(
        &self,
        input: DraftChangeInput,
    ) -> Result<Option<DraftChangeRecord>> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let existing = conn
                .query_row(
                    "SELECT id, draft_id, file_path, variable_id, value_key,
                            before_json, after_json, updated_at
                     FROM draft_changes
                     WHERE draft_id = ?1 AND variable_id = ?2 AND value_key = ?3",
                    params![input.draft_id, input.variable_id, input.value_key],
                    change_from_row,
                )
                .optional()
                .map_err(db_err)?;
            let before = existing
                .as_ref()
                .and_then(|change| serde_json::from_str(&change.before_json).ok())
                .unwrap_or_else(|| input.before.clone());

            if before == input.after {
                if existing.is_some() {
                    conn.execute(
                        "DELETE FROM draft_changes
                         WHERE draft_id = ?1 AND variable_id = ?2 AND value_key = ?3",
                        params![input.draft_id, input.variable_id, input.value_key],
                    )
                    .map_err(db_err)?;
                    conn.execute(
                        "UPDATE draft_sessions SET updated_at = ?1 WHERE id = ?2",
                        params![now, input.draft_id],
                    )
                    .map_err(db_err)?;
                    record_draft_event_sync(
                        conn,
                        &DraftEventInput {
                            draft_id: input.draft_id.clone(),
                            kind: "change.reverted".to_owned(),
                            summary: format!("Reverted {} {}", input.variable_id, input.value_key),
                            detail: Some(serde_json::json!({
                                "filePath": input.file_path,
                                "variableId": input.variable_id,
                                "valueKey": input.value_key,
                            })),
                        },
                    )?;
                }
                return Ok(None);
            }

            if existing.is_some() {
                conn.execute(
                    "UPDATE draft_changes
                     SET file_path = ?1, after_json = ?2, updated_at = ?3
                     WHERE draft_id = ?4 AND variable_id = ?5 AND value_key = ?6",
                    params![
                        input.file_path,
                        input.after.to_string(),
                        now,
                        input.draft_id,
                        input.variable_id,
                        input.value_key,
                    ],
                )
                .map_err(db_err)?;
            } else {
                conn.execute(
                    "INSERT INTO draft_changes (
                       id, draft_id, file_path, variable_id, value_key,
                       before_json, after_json, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        new_id(),
                        input.draft_id,
                        input.file_path,
                        input.variable_id,
                        input.value_key,
                        input.before.to_string(),
                        input.after.to_string(),
                        now,
                    ],
                )
                .map_err(db_err)?;
            }
            let change = conn
                .query_row(
                    "SELECT id, draft_id, file_path, variable_id, value_key,
                            before_json, after_json, updated_at
                     FROM draft_changes
                     WHERE draft_id = ?1 AND variable_id = ?2 AND value_key = ?3",
                    params![input.draft_id, input.variable_id, input.value_key],
                    change_from_row,
                )
                .map_err(db_err)?;
            conn.execute(
                "UPDATE draft_sessions SET updated_at = ?1 WHERE id = ?2",
                params![now, input.draft_id],
            )
            .map_err(db_err)?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: input.draft_id.clone(),
                    kind: if existing.is_some() {
                        "change.updated"
                    } else {
                        "change.created"
                    }
                    .to_owned(),
                    summary: format!(
                        "{} {} {}",
                        if existing.is_some() {
                            "Updated"
                        } else {
                            "Changed"
                        },
                        input.variable_id,
                        input.value_key
                    ),
                    detail: Some(serde_json::json!({
                        "filePath": input.file_path,
                        "variableId": input.variable_id,
                        "valueKey": input.value_key,
                    })),
                },
            )?;
            Ok(Some(change))
        })
        .await
    }

    pub async fn list_draft_changes(&self, draft_id: &str) -> Result<Vec<DraftChangeRecord>> {
        let draft_id = draft_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT id, draft_id, file_path, variable_id, value_key,
                            before_json, after_json, updated_at
                     FROM draft_changes
                     WHERE draft_id = ?1
                     ORDER BY updated_at ASC, variable_id ASC",
                )
                .map_err(db_err)?;
            let changes: Vec<DraftChangeRecord> = statement
                .query_map(params![draft_id], change_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            Ok(changes.into_iter().filter(is_net_draft_change).collect())
        })
        .await
    }

    pub async fn mark_draft_published(
        &self,
        draft_id: &str,
        pr_number: i64,
        pr_state: &str,
        pr_url: &str,
    ) -> Result<()> {
        let draft_id = draft_id.to_owned();
        let pr_state = pr_state.to_owned();
        let pr_url = pr_url.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE draft_sessions
                 SET status = 'published', pr_url = ?1, pr_number = ?2, pr_state = ?3,
                     pr_synced_at = ?4, updated_at = ?5, published_at = ?6
                 WHERE id = ?7",
                params![pr_url, pr_number, pr_state, now, now, now, draft_id],
            )
            .map_err(db_err)?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft_id.clone(),
                    kind: "pr.created".to_owned(),
                    summary: format!("Created pull request #{pr_number}"),
                    detail: Some(serde_json::json!({
                        "prUrl": pr_url,
                        "prNumber": pr_number,
                        "prState": pr_state,
                    })),
                },
            )?;
            Ok(())
        })
        .await
    }

    pub async fn update_draft_branch(
        &self,
        draft_id: &str,
        branch: &str,
        previous_branch: &str,
    ) -> Result<DraftSessionRecord> {
        let draft_id = draft_id.to_owned();
        let branch = branch.to_owned();
        let previous_branch = previous_branch.to_owned();
        self.with_conn(move |conn, _| {
            conn.execute(
                "UPDATE draft_sessions SET branch = ?1, updated_at = ?2 WHERE id = ?3",
                params![branch, now_iso(), draft_id],
            )
            .map_err(db_err)?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft_id.clone(),
                    kind: "draft.branch_renamed".to_owned(),
                    summary: format!("Renamed draft branch to {branch}"),
                    detail: Some(serde_json::json!({
                        "previousBranch": previous_branch,
                        "branch": branch,
                    })),
                },
            )?;
            draft_session_by_id(conn, &draft_id)?
                .ok_or_else(|| RototoError::new("draft session update failed"))
        })
        .await
    }

    pub async fn update_draft_pull_request_state(
        &self,
        input: PullRequestStateInput,
    ) -> Result<DraftSessionRecord> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let existing = draft_session_by_id(conn, &input.draft_id)?;
            let changed = existing.as_ref().is_none_or(|draft| {
                draft.pr_number != Some(input.pr_number)
                    || draft.pr_state.as_deref() != Some(input.pr_state.as_str())
                    || draft.pr_merged_at != input.pr_merged_at
            });
            let updated_at = if changed {
                now.clone()
            } else {
                existing
                    .as_ref()
                    .map(|draft| draft.updated_at.clone())
                    .unwrap_or_else(|| now.clone())
            };

            // A pull request closed without merging ends the publish attempt,
            // not the draft: reopen it so the branch can be edited and
            // published again. The closed pull request stays on GitHub and in
            // the draft's activity.
            let reopened = input.pr_state == "closed"
                && input.pr_merged_at.is_none()
                && existing
                    .as_ref()
                    .is_some_and(|draft| draft.status == DraftStatus::Published);
            if reopened {
                conn.execute(
                    "UPDATE draft_sessions
                     SET status = 'open', pr_number = NULL, pr_state = NULL, pr_url = NULL,
                         pr_merged_at = NULL, pr_synced_at = ?1, published_at = NULL,
                         updated_at = ?2
                     WHERE id = ?3",
                    params![now, now, input.draft_id],
                )
                .map_err(db_err)?;
                record_draft_event_sync(
                    conn,
                    &DraftEventInput {
                        draft_id: input.draft_id.clone(),
                        kind: "pr.closed".to_owned(),
                        summary: format!(
                            "Pull request #{} was closed without merging — draft reopened",
                            input.pr_number
                        ),
                        detail: Some(serde_json::json!({
                            "prNumber": input.pr_number,
                            "prUrl": input.pr_url,
                        })),
                    },
                )?;
            } else {
                conn.execute(
                    "UPDATE draft_sessions
                     SET pr_number = ?1, pr_state = ?2, pr_url = ?3, pr_merged_at = ?4,
                         pr_synced_at = ?5, updated_at = ?6
                     WHERE id = ?7",
                    params![
                        input.pr_number,
                        input.pr_state,
                        input.pr_url,
                        input.pr_merged_at,
                        now,
                        updated_at,
                        input.draft_id,
                    ],
                )
                .map_err(db_err)?;
                if changed {
                    record_draft_event_sync(
                        conn,
                        &DraftEventInput {
                            draft_id: input.draft_id.clone(),
                            kind: "pr.synced".to_owned(),
                            summary: format!(
                                "Synced pull request #{}: {}",
                                input.pr_number, input.pr_state
                            ),
                            detail: Some(serde_json::json!({
                                "prNumber": input.pr_number,
                                "prState": input.pr_state,
                                "prUrl": input.pr_url,
                                "prMergedAt": input.pr_merged_at,
                            })),
                        },
                    )?;
                }
            }
            draft_session_by_id(conn, &input.draft_id)?
                .ok_or_else(|| RototoError::new("draft pull request state update failed"))
        })
        .await
    }

    pub async fn record_draft_event(&self, input: DraftEventInput) -> Result<DraftEventRecord> {
        self.with_conn(move |conn, _| record_draft_event_sync(conn, &input))
            .await
    }

    pub async fn list_draft_events(&self, draft_id: &str) -> Result<Vec<DraftEventRecord>> {
        let draft_id = draft_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT id, draft_id, kind, summary, detail_json, created_at
                     FROM draft_events
                     WHERE draft_id = ?1
                     ORDER BY created_at ASC, rowid ASC",
                )
                .map_err(db_err)?;
            let events = statement
                .query_map(params![draft_id], event_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            Ok(events)
        })
        .await
    }
}

pub fn workspace_slug(name: &str, path: &str) -> String {
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

fn list_workspaces_for_user_sync(
    conn: &Connection,
    github_user_id: &str,
) -> Result<Vec<WorkspaceRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
             FROM workspaces w
             INNER JOIN repos r ON r.id = w.repo_id
             WHERE r.github_user_id = ?1
             ORDER BY w.owner ASC, w.name ASC, w.path ASC",
        )
        .map_err(db_err)?;
    let workspaces = statement
        .query_map(params![github_user_id], workspace_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)?;
    Ok(workspaces)
}

fn repo_with_workspaces_by_id(
    conn: &Connection,
    repo_id: &str,
    github_user_id: &str,
) -> Result<Option<RepoWithWorkspaces>> {
    let repo = conn
        .query_row(
            "SELECT id, github_user_id, owner, name, default_ref,
                    created_at, updated_at, last_discovered_at
             FROM repos WHERE id = ?1 AND github_user_id = ?2",
            params![repo_id, github_user_id],
            |row| {
                Ok(RepoRecord {
                    id: row.get(0)?,
                    github_user_id: row.get(1)?,
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
             FROM workspaces WHERE repo_id = ?1 ORDER BY path ASC",
        )
        .map_err(db_err)?;
    let workspaces = statement
        .query_map(params![repo.id], workspace_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)?;
    Ok(Some(RepoWithWorkspaces { repo, workspaces }))
}

fn draft_session_by_id(conn: &Connection, draft_id: &str) -> Result<Option<DraftSessionRecord>> {
    conn.query_row(
        "SELECT id, workspace_id, github_user_id, branch, base_ref, status,
                pr_url, pr_number, pr_state, pr_merged_at, pr_synced_at,
                created_at, updated_at, published_at
         FROM draft_sessions WHERE id = ?1",
        params![draft_id],
        draft_from_row,
    )
    .optional()
    .map_err(db_err)
}

fn record_draft_event_sync(conn: &Connection, input: &DraftEventInput) -> Result<DraftEventRecord> {
    let id = new_id();
    let now = now_iso();
    let detail_json = input.detail.as_ref().map(|detail| detail.to_string());
    conn.execute(
        "INSERT INTO draft_events (id, draft_id, kind, summary, detail_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            input.draft_id,
            input.kind,
            input.summary,
            detail_json,
            now
        ],
    )
    .map_err(db_err)?;
    Ok(DraftEventRecord {
        id,
        draft_id: input.draft_id.clone(),
        kind: input.kind.clone(),
        summary: input.summary.clone(),
        detail_json,
        created_at: now,
    })
}

fn workspace_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRecord> {
    let name: String = row.get(3)?;
    let path: String = row.get(4)?;
    Ok(WorkspaceRecord {
        id: row.get(0)?,
        slug: workspace_slug(&name, &path),
        repo_id: row.get(1)?,
        owner: row.get(2)?,
        name,
        path,
        git_ref: row.get(5)?,
        source: row.get(6)?,
        discovered_at: row.get(7)?,
    })
}

fn draft_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftSessionRecord> {
    let status: String = row.get(5)?;
    Ok(DraftSessionRecord {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        github_user_id: row.get(2)?,
        branch: row.get(3)?,
        base_ref: row.get(4)?,
        status: if status == "published" {
            DraftStatus::Published
        } else {
            DraftStatus::Open
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

fn change_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftChangeRecord> {
    Ok(DraftChangeRecord {
        id: row.get(0)?,
        draft_id: row.get(1)?,
        file_path: row.get(2)?,
        variable_id: row.get(3)?,
        value_key: row.get(4)?,
        before_json: row.get(5)?,
        after_json: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftEventRecord> {
    Ok(DraftEventRecord {
        id: row.get(0)?,
        draft_id: row.get(1)?,
        kind: row.get(2)?,
        summary: row.get(3)?,
        detail_json: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn is_net_draft_change(change: &DraftChangeRecord) -> bool {
    let before: Option<serde_json::Value> = serde_json::from_str(&change.before_json).ok();
    let after: Option<serde_json::Value> = serde_json::from_str(&change.after_json).ok();
    match (before, after) {
        (Some(before), Some(after)) => before != after,
        _ => true,
    }
}

fn now_iso_minus_state_ttl() -> String {
    super::time::now_iso_minus(OAUTH_STATE_TTL)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn new_session_token() -> Result<String> {
    let mut bytes = [0u8; SESSION_TOKEN_BYTES];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| RototoError::new("failed to generate a session token"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn session_token_hash(session_token: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, session_token.as_bytes());
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn db_err(err: rusqlite::Error) -> RototoError {
    RototoError::new(format!("console database error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> Store {
        Store::open_in_memory(TokenCrypto::generate().unwrap()).unwrap()
    }

    fn discovered(path: &str) -> DiscoveredWorkspaceInput {
        DiscoveredWorkspaceInput {
            path: path.to_owned(),
            git_ref: "main".to_owned(),
            source: format!("https://api.github.com/repos/o/r/tarball/main#:{path}"),
        }
    }

    #[tokio::test]
    async fn sessions_round_trip_and_expire_tokens_encrypted() {
        let store = test_store().await;
        let token = store
            .create_session(NewSession {
                github_user_id: "42".to_owned(),
                github_login: "octocat".to_owned(),
                github_name: Some("Octo Cat".to_owned()),
                github_avatar_url: None,
                github_token: "gho_secret".to_owned(),
            })
            .await
            .unwrap();
        let user = store.get_session(&token).await.unwrap().unwrap();
        assert_eq!(user.github_login, "octocat");
        assert_eq!(user.github_token, "gho_secret");
        store.delete_session(&token).await.unwrap();
        assert!(store.get_session(&token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn oauth_states_consume_once() {
        let store = test_store().await;
        store.create_oauth_state("abc").await.unwrap();
        assert!(store.consume_oauth_state("abc").await.unwrap());
        assert!(!store.consume_oauth_state("abc").await.unwrap());
        assert!(!store.consume_oauth_state("missing").await.unwrap());
    }

    #[tokio::test]
    async fn repo_upsert_lists_workspaces_with_slugs() {
        let store = test_store().await;
        let repo = store
            .upsert_repo_with_workspaces(
                "42".to_owned(),
                "octo".to_owned(),
                "configs".to_owned(),
                "main".to_owned(),
                vec![discovered("."), discovered("payments/flags")],
            )
            .await
            .unwrap();
        assert_eq!(repo.workspaces.len(), 2);
        assert_eq!(repo.workspaces[0].slug, "configs");
        assert_eq!(repo.workspaces[1].slug, "configs-payments-flags");

        let by_slug = store
            .get_workspace_for_user("configs-payments-flags", "42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_slug.path, "payments/flags");
        let by_id = store
            .get_workspace_for_user(&by_slug.id, "42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_id.id, by_slug.id);
        assert!(
            store
                .get_workspace_for_user(&by_slug.id, "999")
                .await
                .unwrap()
                .is_none()
        );

        assert!(
            store
                .delete_repo_for_user(&repo.repo.id, "42")
                .await
                .unwrap()
        );
        assert!(
            store
                .list_workspaces_for_user("42")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn draft_change_revert_deletes_row() {
        let store = test_store().await;
        let repo = store
            .upsert_repo_with_workspaces(
                "42".to_owned(),
                "octo".to_owned(),
                "configs".to_owned(),
                "main".to_owned(),
                vec![discovered(".")],
            )
            .await
            .unwrap();
        let workspace = repo.workspaces[0].clone();
        let draft = store
            .create_draft_session(NewDraftSession {
                workspace_id: workspace.id.clone(),
                github_user_id: "42".to_owned(),
                branch: "rototo-console/octocat/abc/20260613000000".to_owned(),
                base_ref: "main".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(draft.status, DraftStatus::Open);

        let change = store
            .record_draft_change(DraftChangeInput {
                draft_id: draft.id.clone(),
                file_path: "variables/banner.toml".to_owned(),
                variable_id: "banner".to_owned(),
                value_key: "control".to_owned(),
                before: serde_json::json!(false),
                after: serde_json::json!(true),
            })
            .await
            .unwrap();
        assert!(change.is_some());
        assert_eq!(store.list_draft_changes(&draft.id).await.unwrap().len(), 1);

        // Reverting back to the original value clears the tracked change.
        let reverted = store
            .record_draft_change(DraftChangeInput {
                draft_id: draft.id.clone(),
                file_path: "variables/banner.toml".to_owned(),
                variable_id: "banner".to_owned(),
                value_key: "control".to_owned(),
                before: serde_json::json!(true),
                after: serde_json::json!(false),
            })
            .await
            .unwrap();
        assert!(reverted.is_none());
        assert!(
            store
                .list_draft_changes(&draft.id)
                .await
                .unwrap()
                .is_empty()
        );

        let kinds: Vec<String> = store
            .list_draft_events(&draft.id)
            .await
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(
            kinds,
            ["draft.created", "change.created", "change.reverted"]
        );
    }

    #[tokio::test]
    async fn closed_unmerged_pull_request_reopens_draft() {
        let store = test_store().await;
        let repo = store
            .upsert_repo_with_workspaces(
                "42".to_owned(),
                "octo".to_owned(),
                "configs".to_owned(),
                "main".to_owned(),
                vec![discovered(".")],
            )
            .await
            .unwrap();
        let draft = store
            .create_draft_session(NewDraftSession {
                workspace_id: repo.workspaces[0].id.clone(),
                github_user_id: "42".to_owned(),
                branch: "draft-branch".to_owned(),
                base_ref: "main".to_owned(),
            })
            .await
            .unwrap();
        store
            .mark_draft_published(
                &draft.id,
                7,
                "open",
                "https://github.com/octo/configs/pull/7",
            )
            .await
            .unwrap();
        let published = store
            .get_draft_session_for_user(&draft.id, &draft.workspace_id, "42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(published.status, DraftStatus::Published);

        let reopened = store
            .update_draft_pull_request_state(PullRequestStateInput {
                draft_id: draft.id.clone(),
                pr_number: 7,
                pr_state: "closed".to_owned(),
                pr_url: "https://github.com/octo/configs/pull/7".to_owned(),
                pr_merged_at: None,
            })
            .await
            .unwrap();
        assert_eq!(reopened.status, DraftStatus::Open);
        assert_eq!(reopened.pr_number, None);
        assert_eq!(reopened.pr_url, None);
    }
}
