//! SQLite files: Cloudflare D1's local databases (the `.sqlite` files
//! Miniflare keeps in `.wrangler/state`) and any `.sqlite` file opened by
//! hand. What the table browser, SQL editor, schema tree, diagrams and
//! exports need, over the same types the PostgreSQL code returns.
//!
//! Sawit leaves a database's folder as it found it. While `wrangler dev`
//! (or Vite, or `cf dev`) runs, workerd owns the file and keeps its
//! changes in the `-wal` file beside it, even between runs (the main file
//! can be a bare 4 KiB header). So:
//!
//! - Every connection lives for one operation and is closed before the next
//!   one on the same file starts ([`with_read`], [`with_write`]), so Sawit
//!   never holds a file open while it idles, and its own connections never
//!   keep each other from cleaning up.
//! - Browsing opens read-only (`SQLITE_OPEN_READONLY`, `query_only`) and
//!   reads through the existing `-wal`/`-shm`. A WAL database with no `-wal`
//!   file is opened `immutable`: everything is in the main file, and a
//!   read-only connection would otherwise create `-wal` and `-shm` files it
//!   can't remove.
//! - A write straight to the file (no dev server has it open) keeps a `-wal`
//!   file that was already there (no checkpoint on close, as workerd leaves
//!   it), and removes the ones it had to make itself (the normal checkpoint
//!   on close). A rollback journal never outlives its transaction.

mod open;
mod rows;
mod run;
mod schema;

use rusqlite::types::ValueRef;

use crate::CoreError;

pub use open::{sidecar, with_read, with_write};
pub use rows::{bulk_sql, fetch_rows, order_sql, row_count, select_sql, where_sql};
pub use run::{export_rows, run_if_read_only, run_read_only, run_write};
pub use schema::{load_schema, primary_key};

/// The one schema of a SQLite database, as the tree and diagrams name it.
pub const MAIN: &str = "main";

pub(crate) fn lite_error(e: rusqlite::Error) -> CoreError {
    CoreError::Sql(e.to_string())
}

/// Quotes an identifier for SQLite.
pub fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A string literal. SQLite strings take backslashes literally, so only quotes double.
pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A cell as the grid shows it: NULL is `None`, a blob is `\x` and hex (as PostgreSQL's
/// `bytea`), a whole REAL keeps its `.0`.
pub fn value_text(value: ValueRef<'_>) -> Option<String> {
    Some(match value {
        ValueRef::Null => return None,
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 => {
            format!("{f:.1}")
        }
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut text = String::with_capacity(2 + 2 * b.len());
            text.push_str("\\x");
            for &x in b {
                text.push(char::from(HEX[usize::from(x >> 4)]));
                text.push(char::from(HEX[usize::from(x & 0xf)]));
            }
            text
        }
    })
}

#[cfg(test)]
mod tests;
