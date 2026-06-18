use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{OptionalExtension, params};

use crate::console::identity::ActorIdentity;
use crate::console::time::{now_iso, now_iso_minus, now_iso_plus};
use crate::error::{Result, RototoError};

use super::Store;
use super::types::{NewSession, SessionUser};
use super::util::db_err;

const SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 14);
const OAUTH_STATE_TTL: Duration = Duration::from_secs(60 * 10);
const SESSION_TOKEN_BYTES: usize = 32;

impl Store {
    pub async fn create_session(&self, input: NewSession) -> Result<String> {
        self.with_conn(move |conn, crypto| {
            let session_token = new_session_token()?;
            let now = now_iso();
            let expires_at = now_iso_plus(SESSION_TTL);
            let ActorIdentity::GitHub {
                id,
                login,
                name,
                avatar_url,
            } = input.identity
            else {
                return Err(RototoError::new(
                    "GitHub OAuth sessions require a GitHub identity",
                ));
            };
            let principal_id = format!("github:{id}");
            conn.execute(
                "INSERT INTO sessions (
               id, principal_id, github_login, github_name, github_avatar_url,
               github_token_ciphertext, created_at, expires_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session_token_hash(&session_token),
                    principal_id,
                    login,
                    name,
                    avatar_url,
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
                    "SELECT id, principal_id, github_login, github_name, github_avatar_url,
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
            let Some((session_id, principal_id, login, name, avatar, ciphertext, expires_at)) = row
            else {
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
            let github_id = principal_id
                .strip_prefix("github:")
                .unwrap_or(principal_id.as_str())
                .to_owned();
            Ok(Some(SessionUser {
                session_hash: session_id,
                principal_id,
                identity: ActorIdentity::GitHub {
                    id: github_id,
                    login,
                    name,
                    avatar_url: avatar,
                },
                github_token: Some(github_token),
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
}

fn now_iso_minus_state_ttl() -> String {
    now_iso_minus(OAUTH_STATE_TTL)
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
