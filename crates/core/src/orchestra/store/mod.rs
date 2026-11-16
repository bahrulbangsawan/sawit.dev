//! The runtime's store: one SQLite file in WAL mode, shared by the app's scheduler and every
//! `sawit-mcp` process an agent calls tools through. Every change that touches several records is
//! one transaction, begun `IMMEDIATE` so two processes never interleave a claim. The connection
//! sits behind a plain mutex that is only ever held inside a blocking call ([`Store::run`]),
//! never across an `.await`.

pub mod agents;
pub mod mail;
mod recovery;
mod schema;
pub mod tasks;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::{OrchestraError, Result};

/// The store's file in Sawit's data folder.
pub fn default_path(root: &Path) -> PathBuf {
    root.join("orchestra.sqlite")
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("path", &self.path).finish()
    }
}

impl Store {
    /// Opens (or creates) the store at `path` and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| OrchestraError::Store(format!("{}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        Self::prepare(conn, Some(path.to_path_buf()))
    }

    /// A private store in memory, for tests and previews.
    pub fn open_in_memory() -> Result<Store> {
        Self::prepare(Connection::open_in_memory()?, None)
    }

    fn prepare(mut conn: Connection, path: Option<PathBuf>) -> Result<Store> {
        conn.busy_timeout(Duration::from_secs(10))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        if path.is_some() {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        schema::migrate(&mut conn)?;
        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
            path,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while the lock was held leaves SQLite itself consistent (the transaction rolls
        // back when dropped), so a poisoned lock is still safe to use.
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Runs `f` on the connection. Blocking: call it from [`Store::run`] or a blocking thread.
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        f(&self.lock())
    }

    /// Runs `f` in one `IMMEDIATE` transaction: committed when it returns `Ok`, rolled back
    /// otherwise. Blocking.
    pub fn tx<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Runs blocking store work on Tokio's blocking pool, off the async workers.
    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Store) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .map_err(|e| OrchestraError::Store(format!("store task failed: {e}")))?
    }
}

/// `value` as JSON text for a column.
pub(crate) fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| OrchestraError::Store(e.to_string()))
}

/// A JSON column read back; a column that does not parse is a store error, never a panic.
pub(crate) fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// An unknown enum name read from a column.
pub(crate) fn bad_column(what: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        format!("unknown {what} `{value}`").into(),
    )
}
