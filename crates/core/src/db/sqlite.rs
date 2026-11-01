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

use std::collections::HashMap;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use rusqlite::config::DbConfig;
use rusqlite::fallible_iterator::FallibleIterator as _;
use rusqlite::types::ValueRef;
use rusqlite::{Batch, Connection, OpenFlags};

use super::export::{ExportFormat, ExportSummary, RowWriter, kinds_for, write_file};
use super::query::{
    ColumnInfo, Constraint, ConstraintKind, ForeignKey, IndexInfo, QueryOutcome, ReadOnlyTry,
    RelKind, Relation, ResultSet, RowCount, SchemaInfo,
};
use super::rows::{BulkChange, FilterOp, RowQuery, RowTarget, SortDir};
use crate::{CoreError, Result};

/// The one schema of a SQLite database, as the tree and diagrams name it.
pub const MAIN: &str = "main";

/// How long a statement waits for another connection's lock (workerd writing) before giving up.
const BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) fn lite_error(e: rusqlite::Error) -> CoreError {
    CoreError::Sql(e.to_string())
}

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
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("\\x{hex}")
        }
    })
}

/// Tables Sawit hides: SQLite's own, and D1's (`_cf_KV`, `_cf_METADATA`), which D1 refuses
/// to touch anyway.
fn is_internal(name: &str) -> bool {
    name.starts_with("sqlite_") || name.starts_with("_cf_")
}

/// The tables and views of the database, as one schema, `main`, with their columns, keys,
/// foreign keys and indexes. Row counts are exact (see [`row_count`]), so no estimate.
pub fn load_schema(conn: &Connection) -> Result<Vec<SchemaInfo>> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name",
        )
        .map_err(lite_error)?;
    let names: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)?;
    let mut relations = Vec::new();
    for (name, kind) in names.into_iter().filter(|(n, _)| !is_internal(n)) {
        relations.push(relation(conn, &name, kind == "view")?);
    }
    // A foreign key to the parent's primary key names no columns: fill them in.
    let keys: HashMap<String, Vec<String>> = relations
        .iter()
        .map(|r| (r.name.clone(), r.primary_key.clone()))
        .collect();
    for rel in &mut relations {
        for fk in &mut rel.foreign_keys {
            if fk.ref_columns.iter().all(String::is_empty) {
                fk.ref_columns = keys.get(&fk.ref_table).cloned().unwrap_or_default();
            }
        }
    }
    Ok(vec![SchemaInfo {
        name: MAIN.into(),
        relations,
    }])
}

fn relation(conn: &Connection, name: &str, view: bool) -> Result<Relation> {
    let mut rel = Relation {
        name: name.into(),
        kind: if view { RelKind::View } else { RelKind::Table },
        readable: true,
        ..Relation::default()
    };
    let mut pk: Vec<(i64, String)> = Vec::new();
    let mut stmt = conn
        .prepare(r#"SELECT name, type, "notnull", pk, hidden FROM pragma_table_xinfo(?1)"#)
        .map_err(lite_error)?;
    let mut rows = stmt.query([name]).map_err(lite_error)?;
    while let Some(row) = rows.next().map_err(lite_error)? {
        let hidden: i64 = row.get(4).map_err(lite_error)?;
        if hidden == 1 {
            continue; // a virtual table's hidden column
        }
        let column: String = row.get(0).map_err(lite_error)?;
        let key: i64 = row.get(3).map_err(lite_error)?;
        if key > 0 {
            pk.push((key, column.clone()));
        }
        rel.columns.push(ColumnInfo {
            name: column,
            data_type: row.get::<_, String>(1).map_err(lite_error)?,
            nullable: row.get::<_, i64>(2).map_err(lite_error)? == 0,
        });
    }
    if view {
        return Ok(rel);
    }
    pk.sort();
    rel.primary_key = pk.into_iter().map(|(_, c)| c).collect();
    if !rel.primary_key.is_empty() {
        rel.constraints.push(Constraint {
            name: "primary key".into(),
            kind: ConstraintKind::PrimaryKey,
            definition: format!("PRIMARY KEY ({})", rel.primary_key.join(", ")),
        });
    }
    let mut stmt = conn
        .prepare(
            r#"SELECT id, "table", "from", "to" FROM pragma_foreign_key_list(?1) ORDER BY id, seq"#,
        )
        .map_err(lite_error)?;
    let mut rows = stmt.query([name]).map_err(lite_error)?;
    let mut current: Option<i64> = None;
    while let Some(row) = rows.next().map_err(lite_error)? {
        let id: i64 = row.get(0).map_err(lite_error)?;
        let from: String = row.get(2).map_err(lite_error)?;
        let to: Option<String> = row.get(3).map_err(lite_error)?;
        if current != Some(id) {
            current = Some(id);
            rel.foreign_keys.push(ForeignKey {
                columns: vec![],
                ref_schema: MAIN.into(),
                ref_table: row.get(1).map_err(lite_error)?,
                ref_columns: vec![],
            });
        }
        let fk = rel.foreign_keys.last_mut().expect("pushed above");
        fk.columns.push(from);
        fk.ref_columns.push(to.unwrap_or_default());
    }
    for fk in &rel.foreign_keys {
        let to = if fk.ref_columns.iter().all(String::is_empty) {
            String::new()
        } else {
            format!("({})", fk.ref_columns.join(", "))
        };
        rel.constraints.push(Constraint {
            name: format!("{} → {}", fk.columns.join(", "), fk.ref_table),
            kind: ConstraintKind::ForeignKey,
            definition: format!(
                "FOREIGN KEY ({}) REFERENCES {}{to}",
                fk.columns.join(", "),
                fk.ref_table
            ),
        });
    }
    let mut stmt = conn
        .prepare(
            r#"SELECT l.name, l."unique", l.origin, m.sql FROM pragma_index_list(?1) l
               LEFT JOIN sqlite_master m ON m.type = 'index' AND m.name = l.name ORDER BY l.name"#,
        )
        .map_err(lite_error)?;
    let indexes: Vec<(String, bool, String, Option<String>)> = stmt
        .query_map([name], |r| {
            Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?, r.get(3)?))
        })
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)?;
    for (index, unique, origin, sql) in indexes {
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
            .and_then(|mut s| {
                s.query_map([&index], |r| r.get::<_, Option<String>>(0))?
                    .map(|c| c.map(Option::unwrap_or_default))
                    .collect()
            })
            .map_err(lite_error)?;
        let cols = columns.join(", ");
        if origin == "u" {
            rel.constraints.push(Constraint {
                name: index.clone(),
                kind: ConstraintKind::Unique,
                definition: format!("UNIQUE ({cols})"),
            });
        }
        rel.indexes.push(IndexInfo {
            definition: sql.unwrap_or_else(|| {
                format!(
                    "{} ({cols}), made by SQLite for {}",
                    if unique { "UNIQUE INDEX" } else { "INDEX" },
                    if origin == "pk" {
                        "the primary key"
                    } else {
                        "a UNIQUE constraint"
                    }
                )
            }),
            name: index,
            method: "btree".into(),
            unique,
            primary: origin == "pk",
        });
    }
    Ok(rel)
}

/// Primary-key columns of `table`, in key order (empty for views and keyless tables).
pub fn primary_key(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")
        .map_err(lite_error)?;
    stmt.query_map([table], |r| r.get(0))
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)
}

fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_xinfo(?1) WHERE hidden <> 1")
        .map_err(lite_error)?;
    stmt.query_map([table], |r| r.get(0))
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)
}

/// A LIKE pattern matching `value` literally, for `ESCAPE '!'`. LIKE ignores ASCII case in
/// SQLite, as ILIKE does in PostgreSQL.
fn like(prefix: &str, value: &str, suffix: &str) -> String {
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '!' | '%' | '_' => vec!['!', c],
            c => vec![c],
        })
        .collect();
    literal(&format!("{prefix}{escaped}{suffix}"))
}

/// The `WHERE` condition for `query` (SQLite's dialect of [`RowQuery::where_sql`]); the
/// whole-row search looks in each of `columns`.
pub fn where_sql(query: &RowQuery, columns: &[String]) -> Option<String> {
    let mut parts: Vec<String> = query
        .filters
        .iter()
        .map(|f| {
            let col = quote(&f.column);
            let v = || literal(&f.value);
            let text = format!("CAST({col} AS TEXT)");
            match f.op {
                // The column's affinity applies to the literal, so `id = '7'` compares numbers.
                FilterOp::Eq => format!("{col} = {}", v()),
                FilterOp::Ne => format!("{col} IS NOT {}", v()),
                FilterOp::Gt => format!("{col} > {}", v()),
                FilterOp::Ge => format!("{col} >= {}", v()),
                FilterOp::Lt => format!("{col} < {}", v()),
                FilterOp::Le => format!("{col} <= {}", v()),
                FilterOp::Contains => {
                    format!("{text} LIKE {} ESCAPE '!'", like("%", &f.value, "%"))
                }
                FilterOp::NotContains => format!(
                    "({col} IS NULL OR {text} NOT LIKE {} ESCAPE '!')",
                    like("%", &f.value, "%")
                ),
                FilterOp::StartsWith => {
                    format!("{text} LIKE {} ESCAPE '!'", like("", &f.value, "%"))
                }
                FilterOp::IsNull => format!("{col} IS NULL"),
                FilterOp::IsNotNull => format!("{col} IS NOT NULL"),
            }
        })
        .collect();
    let search = query.search.trim();
    if !search.is_empty() && !columns.is_empty() {
        let pattern = like("%", search, "%");
        let any: Vec<String> = columns
            .iter()
            .map(|c| format!("CAST({} AS TEXT) LIKE {pattern} ESCAPE '!'", quote(c)))
            .collect();
        parts.push(format!("({})", any.join(" OR ")));
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

/// The ORDER BY list: the sort column first (NULLs last either way), then `key`.
pub fn order_sql(query: &RowQuery, key: &[String]) -> Option<String> {
    let mut cols: Vec<String> = Vec::new();
    if let Some((column, dir)) = &query.sort {
        cols.push(format!(
            "{} {} NULLS LAST",
            quote(column),
            match dir {
                SortDir::Asc => "ASC",
                SortDir::Desc => "DESC",
            }
        ));
    }
    cols.extend(
        key.iter()
            .filter(|k| query.sort.as_ref().is_none_or(|(c, _)| c != *k))
            .map(|k| quote(k)),
    );
    (!cols.is_empty()).then(|| cols.join(", "))
}

/// The browser's SELECT; `page` is (offset, limit), `None` for every row.
pub fn select_sql(
    table: &str,
    columns: &[String],
    order_by: &[String],
    query: &RowQuery,
    page: Option<(u64, u64)>,
) -> String {
    let filter = where_sql(query, columns)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let order = order_sql(query, order_by)
        .map(|o| format!(" ORDER BY {o}"))
        .unwrap_or_default();
    let range = page
        .map(|(offset, limit)| format!(" LIMIT {limit} OFFSET {offset}"))
        .unwrap_or_default();
    format!("SELECT * FROM {}{filter}{order}{range}", quote(table))
}

/// Runs one query, keeping at most `max_rows` rows.
fn query_rows(conn: &Connection, sql: &str, max_rows: usize) -> Result<ResultSet> {
    let mut stmt = conn.prepare(sql).map_err(lite_error)?;
    let columns: Vec<String> = stmt.column_names().map(String::from).collect();
    let n = columns.len();
    let mut set = ResultSet {
        columns,
        ..ResultSet::default()
    };
    let mut rows = stmt.raw_query();
    while let Some(row) = rows.next().map_err(lite_error)? {
        if set.rows.len() >= max_rows {
            set.truncated = true;
            break;
        }
        set.rows.push(
            (0..n)
                .map(|i| row.get_ref(i).ok().and_then(value_text))
                .collect(),
        );
    }
    Ok(set)
}

/// One page of `table`, narrowed and sorted by `query`, `order_by` (the key) breaking ties.
pub fn fetch_rows(
    conn: &Connection,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    offset: u64,
    limit: u64,
) -> Result<ResultSet> {
    let columns = column_names(conn, table)?;
    let sql = select_sql(table, &columns, order_by, query, Some((offset, limit)));
    query_rows(conn, &sql, limit as usize)
}

/// Rows of `table` matching `query`, counted exactly (local files are small enough).
pub fn row_count(conn: &Connection, table: &str, query: &RowQuery) -> Result<RowCount> {
    let columns = column_names(conn, table)?;
    let filter = where_sql(query, &columns)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let rows: i64 = conn
        .query_row(
            &format!("SELECT count(*) FROM {}{filter}", quote(table)),
            [],
            |r| r.get(0),
        )
        .map_err(lite_error)?;
    Ok(RowCount {
        rows: rows.max(0) as u64,
        exact: true,
    })
}

/// The one statement that applies `change` to `target` in `table` (SQLite's dialect of
/// [`super::bulk_sql`]); `columns` are the table's, for the row search.
pub fn bulk_sql(
    table: &str,
    columns: &[String],
    target: &RowTarget,
    change: &BulkChange,
) -> String {
    let filter = match target {
        RowTarget::Matching(query) => where_sql(query, columns),
        RowTarget::Keys { rows, .. } if rows.is_empty() => Some("0".into()),
        RowTarget::Keys { key, rows } => Some(
            rows.iter()
                .map(|values| {
                    let terms: Vec<String> = key
                        .iter()
                        .zip(values)
                        .map(|(k, v)| format!("{} = {}", quote(k), literal(v)))
                        .collect();
                    format!("({})", terms.join(" AND "))
                })
                .collect::<Vec<_>>()
                .join(" OR "),
        ),
    }
    .map(|w| format!(" WHERE {w}"))
    .unwrap_or_default();
    match change {
        BulkChange::Delete => format!("DELETE FROM {}{filter}", quote(table)),
        BulkChange::Update { column, value } => format!(
            "UPDATE {} SET {} = {}{filter}",
            quote(table),
            quote(column),
            value
                .as_deref()
                .map(literal)
                .unwrap_or_else(|| "NULL".into())
        ),
    }
}

/// Runs every statement of `sql` on `conn`, keeping at most `max_rows` rows per result.
fn run_all(conn: &Connection, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    let started = Instant::now();
    let mut out = QueryOutcome::default();
    let mut batch = Batch::new(conn, sql);
    while let Some(mut stmt) = batch.next().map_err(lite_error)? {
        let n = stmt.column_count();
        let columns: Vec<String> = stmt.column_names().map(String::from).collect();
        let mut set = ResultSet {
            columns,
            ..ResultSet::default()
        };
        let mut rows = stmt.raw_query();
        while let Some(row) = rows.next().map_err(lite_error)? {
            if set.rows.len() < max_rows {
                set.rows.push(
                    (0..n)
                        .map(|i| row.get_ref(i).ok().and_then(value_text))
                        .collect(),
                );
            } else {
                set.truncated = true;
            }
        }
        drop(rows);
        if n > 0 {
            out.results.push(set);
        } else {
            out.affected.push(conn.changes());
        }
    }
    out.duration = started.elapsed();
    Ok(out)
}

/// Runs `sql` read-only: what it returned when every statement only reads; else, the first
/// statement that writes stops it ([`ReadOnlyTry::Writes`]), the reads before it having run
/// (they change nothing). The connection is read-only too, so a write can't get through.
pub fn run_if_read_only(conn: &Connection, sql: &str, max_rows: usize) -> Result<ReadOnlyTry> {
    let started = Instant::now();
    let mut out = QueryOutcome::default();
    let mut batch = Batch::new(conn, sql);
    while let Some(stmt) = batch.next().map_err(lite_error)? {
        if !stmt.readonly() {
            let text = stmt
                .expanded_sql()
                .unwrap_or_default()
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            return Ok(ReadOnlyTry::Writes(format!(
                "“{text}…” changes the database, and this runs read-only."
            )));
        }
        drop(stmt);
    }
    // Every statement reads: run them for real.
    let mut ran = run_all(conn, sql, max_rows)?;
    ran.duration = started.elapsed();
    out.results = ran.results;
    out.affected = ran.affected;
    out.duration = ran.duration;
    Ok(ReadOnlyTry::Read(out))
}

/// [`run_if_read_only`] for one statement, refusing writes and several statements, as
/// PostgreSQL's `run_read_only` does.
pub fn run_read_only(conn: &Connection, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    let mut count = 0;
    let mut batch = Batch::new(conn, sql);
    while batch.next().map_err(lite_error)?.is_some() {
        count += 1;
    }
    if count > 1 {
        return Err(CoreError::Sql(
            "Only one statement can run read-only at a time.".into(),
        ));
    }
    match run_if_read_only(conn, sql, max_rows)? {
        ReadOnlyTry::Read(outcome) => Ok(outcome),
        ReadOnlyTry::Writes(why) | ReadOnlyTry::Several(why) => Err(CoreError::Sql(why)),
    }
}

/// Runs `sql` (one or more statements) on a read-write connection in one transaction: all of
/// it lands, or none of it. As D1 itself, it takes no BEGIN or COMMIT of its own.
pub fn run_write(conn: &Connection, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    conn.execute_batch("BEGIN IMMEDIATE").map_err(lite_error)?;
    match run_all(conn, sql, max_rows) {
        Ok(out) => {
            conn.execute_batch("COMMIT").map_err(lite_error)?;
            Ok(out)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Every row of `table` matching `query`, in the browser's order, streamed to `path`.
#[allow(clippy::too_many_arguments)]
pub fn export_rows(
    conn: &Connection,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    format: ExportFormat,
    types: &[ColumnInfo],
    path: &Path,
    progress: &AtomicU64,
    stop: &AtomicBool,
) -> Result<ExportSummary> {
    let columns = column_names(conn, table)?;
    let sql = select_sql(table, &columns, order_by, query, None);
    let mut stmt = conn.prepare(&sql).map_err(lite_error)?;
    let names: Vec<String> = stmt.column_names().map(String::from).collect();
    let n = names.len();
    let kinds = kinds_for(&names, types);
    write_file(path, |out: BufWriter<std::fs::File>| {
        let mut w = RowWriter::new(out, format, table, &names, kinds)?;
        let mut rows = stmt.raw_query();
        while let Some(row) = rows.next().map_err(lite_error)? {
            if stop.load(Ordering::Relaxed) {
                return Err(CoreError::Sql("Export canceled.".into()));
            }
            let full = w.is_full();
            let values: Vec<Option<String>> = (0..n)
                .map(|i| row.get_ref(i).ok().and_then(value_text))
                .collect();
            let refs: Vec<Option<&str>> = values.iter().map(|v| v.as_deref()).collect();
            w.row(&refs)?;
            if full {
                break;
            }
            progress.fetch_add(1, Ordering::Relaxed);
        }
        Ok(w.finish()?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::rows::RowFilter;

    /// A WAL database like the ones workerd leaves: data in the `-wal`, `-wal` and `-shm`
    /// kept after it closes.
    fn shop(dir: &Path, keep_wal: bool) -> PathBuf {
        let path = dir.join("shop.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers, total REAL, note TEXT);
             CREATE INDEX orders_customer ON orders (customer_id);
             CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 10;
             CREATE TABLE _cf_KV (key TEXT PRIMARY KEY, value BLOB);
             INSERT INTO customers (name, email) VALUES ('Ana', 'ana@x.id'), ('Budi', NULL), ('Citra', 'c@x.id');
             INSERT INTO orders (customer_id, total, note) VALUES (1, 12.5, 'first'), (1, 3, NULL), (2, 40, 'Rush 100%'), (3, 7.25, 'gift');",
        )
        .unwrap();
        if keep_wal {
            conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
                .unwrap();
        }
        drop(conn);
        assert_eq!(sidecar(&path, "-wal").exists(), keep_wal);
        path
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn schema_lists_tables_views_keys_and_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let path = shop(dir.path(), true);
        let schemas = with_read(&path, load_schema).unwrap();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "main");
        let names: Vec<_> = schemas[0]
            .relations
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        // D1's own tables stay hidden.
        assert_eq!(names, ["big_orders", "customers", "orders"]);
        let orders = &schemas[0].relations[2];
        assert_eq!(orders.primary_key, ["id"]);
        assert_eq!(orders.columns[2].data_type, "REAL");
        assert!(orders.columns[2].nullable);
        assert_eq!(orders.foreign_keys[0].ref_table, "customers");
        // No column named: the parent's primary key.
        assert_eq!(orders.foreign_keys[0].ref_columns, ["id"]);
        assert!(
            orders
                .indexes
                .iter()
                .any(|i| i.name == "orders_customer" && i.definition.starts_with("CREATE INDEX"))
        );
        let customers = &schemas[0].relations[1];
        assert!(!customers.columns[1].nullable);
        assert!(
            customers
                .constraints
                .iter()
                .any(|c| c.kind == ConstraintKind::Unique)
        );
        assert_eq!(schemas[0].relations[0].kind, RelKind::View);
    }

    #[test]
    fn rows_sort_filter_search_and_page() {
        let dir = tempfile::tempdir().unwrap();
        let path = shop(dir.path(), true);
        let key = vec!["id".to_string()];
        let page = |q: &RowQuery, offset, limit| {
            with_read(&path, |c| fetch_rows(c, "orders", &key, q, offset, limit)).unwrap()
        };
        let all = page(&RowQuery::default(), 0, 50);
        assert_eq!(all.columns, ["id", "customer_id", "total", "note"]);
        assert_eq!(all.rows.len(), 4);
        // A whole REAL keeps its point; NULL stays NULL.
        assert_eq!(all.rows[1][2].as_deref(), Some("3.0"));
        assert_eq!(all.rows[1][3], None);
        let by_total = RowQuery {
            sort: Some(("total".into(), SortDir::Desc)),
            ..RowQuery::default()
        };
        let ids = |set: &ResultSet| -> Vec<String> {
            set.rows.iter().map(|r| r[0].clone().unwrap()).collect()
        };
        assert_eq!(ids(&page(&by_total, 0, 2)), ["3", "1"]);
        assert_eq!(ids(&page(&by_total, 2, 2)), ["4", "2"]);
        let filtered = RowQuery {
            filters: vec![RowFilter {
                column: "customer_id".into(),
                op: FilterOp::Eq,
                value: "1".into(),
            }],
            ..RowQuery::default()
        };
        assert_eq!(ids(&page(&filtered, 0, 50)), ["1", "2"]);
        // LIKE's own characters are matched literally, in any case.
        let search = RowQuery {
            search: "RUSH 100%".into(),
            ..RowQuery::default()
        };
        assert_eq!(ids(&page(&search, 0, 50)), ["3"]);
        let not_null = RowQuery {
            filters: vec![RowFilter {
                column: "note".into(),
                op: FilterOp::NotContains,
                value: "i".into(),
            }],
            ..RowQuery::default()
        };
        assert_eq!(ids(&page(&not_null, 0, 50)), ["2", "3"]);
        let count = with_read(&path, |c| row_count(c, "orders", &filtered)).unwrap();
        assert_eq!(
            count,
            RowCount {
                rows: 2,
                exact: true
            }
        );
    }

    #[test]
    fn exports_matching_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = shop(dir.path(), true);
        let out = dir.path().join("orders.csv");
        let query = RowQuery {
            sort: Some(("total".into(), SortDir::Asc)),
            ..RowQuery::default()
        };
        let summary = with_read(&path, |c| {
            export_rows(
                c,
                "orders",
                &["id".into()],
                &query,
                ExportFormat::Csv,
                &[],
                &out,
                &AtomicU64::new(0),
                &AtomicBool::new(false),
            )
        })
        .unwrap();
        assert_eq!(summary.rows, 4);
        let text = std::fs::read_to_string(&out).unwrap();
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            [
                "id,customer_id,total,note",
                "2,1,3.0,",
                "4,3,7.25,gift",
                "1,1,12.5,first",
                "3,2,40.0,Rush 100%"
            ]
        );
    }

    #[test]
    fn read_only_refuses_writes_and_leaves_the_folder_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = shop(dir.path(), true);
        let before = files(dir.path());
        assert_eq!(
            before,
            ["shop.sqlite", "shop.sqlite-shm", "shop.sqlite-wal"]
        );
        let tried = with_read(&path, |c| run_if_read_only(c, "DELETE FROM orders", 10)).unwrap();
        assert!(matches!(tried, ReadOnlyTry::Writes(_)), "{tried:?}");
        let e = with_read(&path, |c| {
            run_read_only(c, "UPDATE orders SET total = 0", 10)
        })
        .unwrap_err();
        assert!(e.to_string().contains("read-only"), "{e}");
        // Even past the screening, the connection itself refuses.
        let e = with_read(&path, |c| {
            c.execute_batch("PRAGMA query_only = 0; DELETE FROM orders")
                .map_err(lite_error)
        })
        .unwrap_err();
        assert!(e.to_string().contains("readonly"), "{e}");
        assert!(
            with_read(&path, |c| run_read_only(c, "SELECT 1; SELECT 2", 10)).is_err(),
            "several statements"
        );
        let read = with_read(&path, |c| {
            run_read_only(c, "SELECT count(*) FROM orders", 10)
        })
        .unwrap();
        assert_eq!(read.results[0].rows[0][0].as_deref(), Some("4"));
        assert_eq!(files(dir.path()), before);
    }

    #[test]
    fn a_checkpointed_wal_database_reads_without_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = shop(dir.path(), false);
        assert_eq!(files(dir.path()), ["shop.sqlite"]);
        let n = with_read(&path, |c| row_count(c, "customers", &RowQuery::default())).unwrap();
        assert_eq!(n.rows, 3);
        assert_eq!(files(dir.path()), ["shop.sqlite"]);
    }

    #[test]
    fn writes_keep_a_wal_that_was_there_and_remove_one_they_made() {
        let dir = tempfile::tempdir().unwrap();
        let kept = shop(dir.path(), true);
        let out = with_write(&kept, |c| {
            run_write(c, "UPDATE orders SET total = total + 1 WHERE id = 2", 10)
        })
        .unwrap();
        assert_eq!(out.affected, [1]);
        assert_eq!(
            files(dir.path()),
            ["shop.sqlite", "shop.sqlite-shm", "shop.sqlite-wal"]
        );
        let total = with_read(&kept, |c| {
            run_read_only(c, "SELECT total FROM orders WHERE id = 2", 1)
        })
        .unwrap();
        assert_eq!(total.results[0].rows[0][0].as_deref(), Some("4.0"));

        let other = tempfile::tempdir().unwrap();
        let clean = shop(other.path(), false);
        with_write(&clean, |c| {
            run_write(c, "DELETE FROM orders WHERE id = 4", 10)
        })
        .unwrap();
        assert_eq!(files(other.path()), ["shop.sqlite"]);
        // A failing batch changes nothing.
        let e = with_write(&clean, |c| {
            run_write(c, "DELETE FROM orders; INSERT INTO nowhere VALUES (1)", 10)
        })
        .unwrap_err();
        assert!(e.to_string().contains("nowhere"), "{e}");
        let n = with_read(&clean, |c| row_count(c, "orders", &RowQuery::default())).unwrap();
        assert_eq!(n.rows, 3);
        assert_eq!(files(other.path()), ["shop.sqlite"]);
    }

    #[test]
    fn bulk_changes_use_sqlite_literals() {
        let target = RowTarget::Keys {
            key: vec!["id".into()],
            rows: vec![vec!["1".into()], vec!["3".into()]],
        };
        let change = BulkChange::Update {
            column: "note".into(),
            value: Some("it's \\ fine".into()),
        };
        assert_eq!(
            bulk_sql("orders", &[], &target, &change),
            r#"UPDATE "orders" SET "note" = 'it''s \ fine' WHERE ("id" = '1') OR ("id" = '3')"#
        );
        let dir = tempfile::tempdir().unwrap();
        let path = shop(dir.path(), true);
        let cols = vec!["id".into(), "note".into()];
        let sql = bulk_sql("orders", &cols, &target, &BulkChange::Delete);
        let out = with_write(&path, |c| run_write(c, &sql, 1)).unwrap();
        assert_eq!(out.affected, [2]);
        let none = RowTarget::Keys {
            key: vec!["id".into()],
            rows: vec![],
        };
        assert!(bulk_sql("orders", &cols, &none, &BulkChange::Delete).ends_with("WHERE 0"));
    }

    #[test]
    fn missing_files_are_never_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("none.sqlite");
        assert!(with_read(&path, load_schema).is_err());
        assert!(with_write(&path, |c| run_write(c, "CREATE TABLE t (x)", 1)).is_err());
        assert!(files(dir.path()).is_empty());
    }
}
