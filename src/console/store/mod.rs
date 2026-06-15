use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{Result, RototoError};

use super::token_crypto::TokenCrypto;

mod branches;
mod repos;
mod rows;
mod schema;
mod sessions;
#[cfg(test)]
mod tests;
mod types;
mod util;

pub use types::*;

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
        schema::initialize_schema(&conn)?;
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
}
