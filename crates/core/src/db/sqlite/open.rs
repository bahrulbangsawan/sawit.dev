//! Short-lived connections to a SQLite file that leave its folder as Sawit found it, taking
//! turns per file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};

use super::lite_error;
use crate::{CoreError, Result};

/// How long a statement waits for another connection's lock (workerd writing) before giving up.
const BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// `<file>-wal`, `<file>-shm`, `<file>-journal`.
pub fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The lock Sawit's own connections to `path` take turns with.
fn file_lock(path: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    locks.entry(path.to_path_buf()).or_default().clone()
}

/// The header says WAL (bytes 18 and 19 are 2).
fn is_wal_mode(path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 20];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok()
        && header[18] == 2
        && header[19] == 2
}

fn uri(path: &Path, params: &str) -> String {
    // Percent-encode what a file: URI can't hold as it is.
    let mut out = String::from("file:");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'?' | b'#' | b'%' => out.push_str(&format!("%{b:02X}")),
            _ => out.push(b as char),
        }
    }
    format!("{out}?{params}")
}

fn missing(path: &Path) -> CoreError {
    CoreError::Sql(format!(
        "{} doesn't exist. For a D1 database, run the dev server or `wrangler d1 execute --local` once to create it.",
        path.display()
    ))
}

/// A read-only connection to `path` that creates no file beside it.
fn open_read_only(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(missing(path));
    }
    let wal = sidecar(path, "-wal").exists();
    let shm = sidecar(path, "-shm").exists();
    let params = if !wal && is_wal_mode(path) {
        "mode=ro&immutable=1"
    } else if wal && !shm {
        // Reads the -wal into memory instead of making a -shm.
        "mode=ro&readonly_shm=1"
    } else {
        "mode=ro"
    };
    let conn = Connection::open_with_flags(
        uri(path, params),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(lite_error)?;
    conn.busy_timeout(BUSY_WAIT).map_err(lite_error)?;
    conn.pragma_update(None, "query_only", true)
        .map_err(lite_error)?;
    Ok(conn)
}

/// A read-write connection to an existing `path`; never creates the database.
fn open_read_write(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(missing(path));
    }
    let keep_wal = sidecar(path, "-wal").exists();
    let conn = Connection::open_with_flags(
        uri(path, ""),
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(lite_error)?;
    conn.busy_timeout(BUSY_WAIT).map_err(lite_error)?;
    if keep_wal {
        // As workerd leaves it: the -wal (and -shm) stay, with these changes in them.
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .map_err(lite_error)?;
    }
    Ok(conn)
}

/// Runs `f` on a read-only connection to `path`, closed before it returns.
pub fn with_read<T>(path: &Path, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let lock = file_lock(path);
    let _turn = lock.lock().unwrap_or_else(|e| e.into_inner());
    let conn = open_read_only(path)?;
    let out = f(&conn);
    drop(conn);
    out
}

/// Runs `f` on a read-write connection to `path`, closed before it returns. Callers make
/// sure no dev server has the file open ([`super::d1_write`]).
pub fn with_write<T>(path: &Path, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let lock = file_lock(path);
    let _turn = lock.lock().unwrap_or_else(|e| e.into_inner());
    let conn = open_read_write(path)?;
    let out = f(&conn);
    conn.close().map_err(|(_, e)| lite_error(e))?;
    out
}
