//! One handle for every kind of database, so the sidebar, table grid, SQL
//! editor, diagrams, exports and `sawit-mcp` call the same operations
//! whether the profile is PostgreSQL ([`Conn::Pg`], the code in `query.rs`
//! as it was) or SQLite underneath ([`Conn::Lite`]: a `.sqlite` file, a D1
//! binding's local file, or a remote D1 database through Wrangler).
//!
//! Reads of a local file run on a short-lived read-only connection. Writes
//! take the route [`Lite::write_route`] finds: a running dev server's Local
//! Explorer (workerd owns the file then), the file itself (nothing has it
//! open), or Wrangler for a remote database. Every outcome says which.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use tokio_postgres::{CancelToken, Client};

use super::export::{ExportFormat, ExportSummary, RowWriter, kinds_for, write_file};
use super::profile::{ConnProfile, DbKind};
use super::query::{ColumnInfo, QueryOutcome, ReadOnlyTry, ResultSet, RowCount, SchemaInfo};
use super::rows::{BulkChange, RowQuery, RowTarget};
use super::{explorer, remote, sqlite};
use crate::{CoreError, Result};

/// A live handle on a database.
#[derive(Clone)]
pub enum Conn {
    Pg(Arc<Client>),
    Lite(Arc<Lite>),
}

/// A SQLite-backed database: what it is and where, nothing held open.
pub struct Lite {
    pub profile: ConnProfile,
    /// The browsing handle: never writes, whatever it is asked.
    pub read_only: bool,
    /// Addresses to look for the Local Explorer at, instead of the dev servers running in the
    /// project (tests).
    pub explorer_addrs: Option<Vec<SocketAddr>>,
    /// The remote schema's statements, once fetched (remote D1).
    remote_schema: Mutex<Option<Vec<(String, String)>>>,
}

impl Lite {
    pub fn new(profile: ConnProfile, read_only: bool) -> Lite {
        Lite {
            profile,
            read_only,
            explorer_addrs: None,
            remote_schema: Mutex::new(None),
        }
    }

    fn lite_source(&self) -> super::LiteSource {
        self.profile.lite.clone().unwrap_or_default()
    }

    fn file(&self) -> Result<PathBuf> {
        self.profile
            .file()
            .map(Path::to_path_buf)
            .ok_or_else(|| CoreError::Config("This database has no file.".into()))
    }

    fn project(&self) -> PathBuf {
        self.lite_source().project.unwrap_or_default()
    }

    fn remote_name(&self) -> String {
        self.lite_source()
            .database_name
            .unwrap_or_else(|| self.profile.database.clone())
    }

    fn wrangler(&self) -> Result<PathBuf> {
        remote::wrangler(&self.project()).ok_or_else(|| {
            CoreError::Config(
                "Wrangler isn't installed in this project or on this Mac. Install it (npm i -D wrangler), then sign in with `wrangler login`.".into(),
            )
        })
    }

    async fn remote_run(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
        let wrangler = self.wrangler()?;
        remote::execute(
            &wrangler,
            &self.project(),
            &self.remote_name(),
            sql,
            max_rows,
        )
        .await
    }

    /// The remote schema's statements, fetched once.
    async fn remote_schema(&self, reload: bool) -> Result<Vec<(String, String)>> {
        if !reload
            && let Some(cached) = self
                .remote_schema
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        {
            return Ok(cached);
        }
        let out = self.remote_run(remote::SCHEMA_SQL, usize::MAX).await?;
        let rows: Vec<(String, String)> = out
            .results
            .first()
            .map(|set| {
                set.rows
                    .iter()
                    .filter_map(|r| Some((r.first()?.clone()?, r.get(2)?.clone()?)))
                    .collect()
            })
            .unwrap_or_default();
        *self.remote_schema.lock().unwrap_or_else(|e| e.into_inner()) = Some(rows.clone());
        Ok(rows)
    }

    async fn remote_columns(&self, table: &str) -> Result<Vec<String>> {
        let schema = self.remote_schema(false).await?;
        let copy = remote::shadow(&schema)?;
        Ok(sqlite::load_schema(&copy)?
            .into_iter()
            .flat_map(|s| s.relations)
            .find(|r| r.name == table)
            .map(|r| r.columns.into_iter().map(|c| c.name).collect())
            .unwrap_or_default())
    }

    /// Runs `f` on a read-only connection to the file, off the async threads.
    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let file = self.file()?;
        tokio::task::spawn_blocking(move || sqlite::with_read(&file, f))
            .await
            .map_err(|_| CoreError::Sql("The SQLite reader stopped unexpectedly.".into()))?
    }

    /// Where a write would go right now.
    pub async fn write_route(&self) -> WriteRoute {
        let lite = self.lite_source();
        match self.profile.kind {
            DbKind::D1Remote => WriteRoute::Remote {
                database: self.remote_name(),
            },
            DbKind::Postgres | DbKind::Sqlite => match self.file() {
                Ok(file) => WriteRoute::File(file),
                Err(_) => WriteRoute::Busy { holders: vec![] },
            },
            DbKind::D1Local => {
                let project = lite.project.clone().unwrap_or_default();
                let id = lite.d1_id.clone().unwrap_or_default();
                let binding = lite.binding.clone().unwrap_or_default();
                let worker = lite.worker.as_deref();
                let found = match &self.explorer_addrs {
                    Some(addrs) => explorer::find_among(addrs, worker, &binding, &id).await,
                    None => explorer::find_for(&project, worker, &binding, &id).await,
                };
                if let Some(addr) = found {
                    return WriteRoute::Explorer { addr, id };
                }
                let Ok(file) = self.file() else {
                    return WriteRoute::Busy { holders: vec![] };
                };
                let holders = holders(&file).await;
                if holders.is_empty() {
                    WriteRoute::File(file)
                } else {
                    WriteRoute::Busy { holders }
                }
            }
        }
    }

    /// Runs `sql` (one or more statements) where [`Self::write_route`] says, and answers what
    /// happened and which way it went. Refused on the browsing handle.
    pub async fn write(&self, sql: &str, max_rows: usize) -> Result<(QueryOutcome, WriteRoute)> {
        if self.read_only {
            return Err(CoreError::Sql(
                "This connection only reads. Nothing was changed.".into(),
            ));
        }
        let route = self.write_route().await;
        let outcome = match &route {
            WriteRoute::Explorer { addr, id } => {
                explorer::raw(*addr, id, &explorer::split_statements(sql)).await?
            }
            WriteRoute::File(file) => {
                let (file, sql) = (file.clone(), sql.to_string());
                tokio::task::spawn_blocking(move || {
                    sqlite::with_write(&file, |c| sqlite::run_write(c, &sql, max_rows))
                })
                .await
                .map_err(|_| CoreError::Sql("The SQLite writer stopped unexpectedly.".into()))??
            }
            WriteRoute::Busy { .. } => return Err(CoreError::Sql(route.describe())),
            WriteRoute::Remote { .. } => self.remote_run(sql, max_rows).await?,
        };
        Ok((outcome, route))
    }

    /// What `sql` would do, run only if it just reads.
    pub async fn run_if_read_only(&self, sql: &str, max_rows: usize) -> Result<ReadOnlyTry> {
        if self.profile.kind == DbKind::D1Remote {
            let schema = self.remote_schema(false).await?;
            let copy = remote::shadow(&schema)?;
            if let Err(why) = remote::classify(&copy, sql) {
                return Ok(ReadOnlyTry::Writes(why));
            }
            return Ok(ReadOnlyTry::Read(self.remote_run(sql, max_rows).await?));
        }
        let sql = sql.to_string();
        self.read(move |c| sqlite::run_if_read_only(c, &sql, max_rows))
            .await
    }
}

/// Processes other than this one that have `file` (or its `-wal`/`-shm`) open, as
/// "workerd (pid 4242)".
pub async fn holders(file: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec!["-F".into(), "pc".into(), "--".into()];
    for suffix in ["", "-wal", "-shm"] {
        let path = sqlite::sidecar(file, suffix);
        if path.exists() {
            args.push(path.to_string_lossy().into_owned());
        }
    }
    if args.len() == 3 {
        return Vec::new();
    }
    let Ok(out) = crate::cmd::run("lsof", &args, std::time::Duration::from_secs(3)).await else {
        return Vec::new();
    };
    let me = std::process::id();
    let mut found: Vec<String> = Vec::new();
    let mut pid: Option<u32> = None;
    for line in out.stdout.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let (Some(c), Some(p)) = (line.strip_prefix('c'), pid)
            && p != me
        {
            let who = format!("{c} (pid {p})");
            if !found.contains(&who) {
                found.push(who);
            }
        }
    }
    found
}

/// Which way a write to a SQLite or D1 database goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRoute {
    /// Through the dev server's Local Explorer, so workerd, which owns the file while it runs,
    /// makes the change.
    Explorer { addr: SocketAddr, id: String },
    /// Straight to the file: no dev server has it open.
    File(PathBuf),
    /// A dev server has the file open but no Local Explorer answered; writing to the file now
    /// could fight workerd for it, so nothing is written.
    Busy { holders: Vec<String> },
    /// To Cloudflare, through Wrangler: remote (production).
    Remote { database: String },
}

impl WriteRoute {
    /// A few words for a badge.
    pub fn label(&self) -> String {
        match self {
            WriteRoute::Explorer { addr, .. } => format!("via Local Explorer :{}", addr.port()),
            WriteRoute::File(_) => "straight to the file".into(),
            WriteRoute::Busy { .. } => "writes blocked".into(),
            WriteRoute::Remote { .. } => "remote (production)".into(),
        }
    }

    /// One sentence for confirmations and tooltips.
    pub fn describe(&self) -> String {
        match self {
            WriteRoute::Explorer { addr, .. } => format!(
                "Writes go through the running dev server's Local Explorer (port {}), so workerd makes the change.",
                addr.port()
            ),
            WriteRoute::File(file) => format!(
                "No dev server has this database open, so writes go straight to {}.",
                file.display()
            ),
            WriteRoute::Busy { holders } if holders.is_empty() => {
                "This database can't be written to: it has no file.".into()
            }
            WriteRoute::Busy { holders } => format!(
                "{} has this database open, but its Local Explorer didn't answer, so nothing was written. Stop the dev server (or run it with the Local Explorer on), then try again.",
                holders.join(", ")
            ),
            WriteRoute::Remote { database } => format!(
                "Writes go to {database} on Cloudflare: remote (production), through your Wrangler login."
            ),
        }
    }
}

impl Conn {
    /// A SQLite-backed handle for `profile`.
    pub fn lite(profile: ConnProfile, read_only: bool) -> Conn {
        Conn::Lite(Arc::new(Lite::new(profile, read_only)))
    }

    pub fn kind(&self) -> DbKind {
        match self {
            Conn::Pg(_) => DbKind::Postgres,
            Conn::Lite(l) => l.profile.kind,
        }
    }

    /// The link dropped (PostgreSQL); a file never "drops".
    pub fn is_closed(&self) -> bool {
        match self {
            Conn::Pg(c) => c.is_closed(),
            Conn::Lite(_) => false,
        }
    }

    /// A request holds a clone of this handle right now.
    pub fn in_use(&self) -> bool {
        match self {
            Conn::Pg(c) => Arc::strong_count(c) > 1,
            Conn::Lite(l) => Arc::strong_count(l) > 1,
        }
    }

    /// Cancels the statement in flight (PostgreSQL only; SQLite reads are short).
    pub fn cancel_token(&self) -> Option<CancelToken> {
        match self {
            Conn::Pg(c) => Some(c.cancel_token()),
            Conn::Lite(_) => None,
        }
    }

    /// The PostgreSQL client, for what only PostgreSQL has (backups, imports, roles).
    pub fn pg(&self) -> Option<&Arc<Client>> {
        match self {
            Conn::Pg(c) => Some(c),
            Conn::Lite(_) => None,
        }
    }

    pub async fn load_schema(&self) -> Result<Vec<SchemaInfo>> {
        match self {
            Conn::Pg(c) => super::load_schema(c).await,
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let schema = l.remote_schema(true).await?;
                sqlite::load_schema(&remote::shadow(&schema)?)
            }
            Conn::Lite(l) => l.read(sqlite::load_schema).await,
        }
    }

    pub async fn primary_key(&self, schema: &str, table: &str) -> Result<Vec<String>> {
        match self {
            Conn::Pg(c) => super::primary_key(c, schema, table).await,
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let copy = remote::shadow(&l.remote_schema(false).await?)?;
                sqlite::primary_key(&copy, table)
            }
            Conn::Lite(l) => {
                let table = table.to_string();
                l.read(move |c| sqlite::primary_key(c, &table)).await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn fetch_rows(
        &self,
        schema: &str,
        table: &str,
        order_by: &[String],
        query: &RowQuery,
        offset: u64,
        limit: u64,
    ) -> Result<ResultSet> {
        match self {
            Conn::Pg(c) => {
                super::fetch_rows(c, schema, table, order_by, query, offset, limit).await
            }
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let columns = l.remote_columns(table).await?;
                let sql =
                    sqlite::select_sql(table, &columns, order_by, query, Some((offset, limit)));
                let mut out = l.remote_run(&sql, limit as usize).await?;
                Ok(out.results.pop().unwrap_or(ResultSet {
                    columns,
                    ..ResultSet::default()
                }))
            }
            Conn::Lite(l) => {
                let (table, order_by, query) =
                    (table.to_string(), order_by.to_vec(), query.clone());
                l.read(move |c| sqlite::fetch_rows(c, &table, &order_by, &query, offset, limit))
                    .await
            }
        }
    }

    /// Rows in the relation: PostgreSQL's estimate-aware count, SQLite's exact one.
    pub async fn row_count_estimated(
        &self,
        schema: &str,
        table: &str,
        estimate: Option<i64>,
    ) -> Result<RowCount> {
        match self {
            Conn::Pg(c) => super::row_count_estimated(c, schema, table, estimate).await,
            Conn::Lite(_) => Ok(self
                .row_count_matching(schema, table, &RowQuery::default())
                .await?
                .unwrap_or(RowCount {
                    rows: 0,
                    exact: false,
                })),
        }
    }

    pub async fn row_count_matching(
        &self,
        schema: &str,
        table: &str,
        query: &RowQuery,
    ) -> Result<Option<RowCount>> {
        match self {
            Conn::Pg(c) => super::row_count_matching(c, schema, table, query).await,
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let columns = l.remote_columns(table).await?;
                let filter = sqlite::where_sql(query, &columns)
                    .map(|w| format!(" WHERE {w}"))
                    .unwrap_or_default();
                let sql = format!("SELECT count(*) AS n FROM {}{filter}", sqlite::quote(table));
                let out = l.remote_run(&sql, 1).await?;
                Ok(out
                    .results
                    .first()
                    .and_then(|s| s.rows.first()?.first()?.as_deref()?.parse().ok())
                    .map(|rows| RowCount { rows, exact: true }))
            }
            Conn::Lite(l) => {
                let (table, query) = (table.to_string(), query.clone());
                l.read(move |c| sqlite::row_count(c, &table, &query))
                    .await
                    .map(Some)
            }
        }
    }

    /// Runs one or more statements. On SQLite, SQL that only reads runs read-only; anything
    /// else takes the write route (callers ask first, see [`Self::run_if_read_only`]).
    pub async fn run_sql(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
        match self {
            Conn::Pg(c) => super::run_sql(c, sql, max_rows).await,
            Conn::Lite(l) => match l.run_if_read_only(sql, max_rows).await? {
                ReadOnlyTry::Read(outcome) => Ok(outcome),
                ReadOnlyTry::Writes(_) | ReadOnlyTry::Several(_) => {
                    Ok(l.write(sql, max_rows).await?.0)
                }
            },
        }
    }

    /// One statement, read-only; writes are refused.
    pub async fn run_read_only(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
        match self {
            Conn::Pg(c) => super::run_read_only(c, sql, max_rows).await,
            Conn::Lite(l) => {
                if explorer::split_statements(sql).len() > 1 {
                    return Err(CoreError::Sql(
                        "Only one statement can run read-only at a time.".into(),
                    ));
                }
                match l.run_if_read_only(sql, max_rows).await? {
                    ReadOnlyTry::Read(outcome) => Ok(outcome),
                    ReadOnlyTry::Writes(why) | ReadOnlyTry::Several(why) => {
                        Err(CoreError::Sql(why))
                    }
                }
            }
        }
    }

    pub async fn run_if_read_only(&self, sql: &str, max_rows: usize) -> Result<ReadOnlyTry> {
        match self {
            Conn::Pg(c) => super::run_if_read_only(c, sql, max_rows).await,
            Conn::Lite(l) => l.run_if_read_only(sql, max_rows).await,
        }
    }

    /// Runs a data-changing statement; answers the rows it changed.
    pub async fn run_bulk(&self, sql: &str) -> Result<u64> {
        match self {
            Conn::Pg(c) => super::run_bulk(c, sql).await,
            Conn::Lite(l) => Ok(l.write(sql, 0).await?.0.affected.iter().sum()),
        }
    }

    /// Every row of `schema.table` matching `query`, streamed to `path`.
    #[allow(clippy::too_many_arguments)]
    pub async fn export_rows(
        &self,
        schema: &str,
        table: &str,
        order_by: &[String],
        query: &RowQuery,
        format: ExportFormat,
        types: &[ColumnInfo],
        path: &Path,
        progress: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
    ) -> Result<ExportSummary> {
        match self {
            Conn::Pg(c) => {
                super::export_rows(
                    c, schema, table, order_by, query, format, types, path, &progress, &stop,
                )
                .await
            }
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let columns = l.remote_columns(table).await?;
                let sql = sqlite::select_sql(table, &columns, order_by, query, None);
                let out = l.remote_run(&sql, usize::MAX).await?;
                let set = out.results.into_iter().next().unwrap_or(ResultSet {
                    columns,
                    ..ResultSet::default()
                });
                let (types, table, path) = (types.to_vec(), table.to_string(), path.to_path_buf());
                tokio::task::spawn_blocking(move || {
                    write_file(&path, |out| {
                        let mut w = RowWriter::new(
                            out,
                            format,
                            &table,
                            &set.columns,
                            kinds_for(&set.columns, &types),
                        )?;
                        for row in &set.rows {
                            let values: Vec<Option<&str>> =
                                row.iter().map(|v| v.as_deref()).collect();
                            w.row(&values)?;
                        }
                        Ok(w.finish()?)
                    })
                })
                .await
                .map_err(|_| CoreError::Sql("The export stopped unexpectedly.".into()))?
            }
            Conn::Lite(l) => {
                let (table, order_by, query, types, path) = (
                    table.to_string(),
                    order_by.to_vec(),
                    query.clone(),
                    types.to_vec(),
                    path.to_path_buf(),
                );
                l.read(move |c| {
                    sqlite::export_rows(
                        c, &table, &order_by, &query, format, &types, &path, &progress, &stop,
                    )
                })
                .await
            }
        }
    }
}

/// The statement that applies `change` to `target`, in `kind`'s dialect; `columns` are the
/// table's (SQLite's row search names each).
pub fn bulk_sql_for(
    kind: DbKind,
    schema: &str,
    table: &str,
    columns: &[String],
    target: &RowTarget,
    change: &BulkChange,
) -> String {
    match kind {
        DbKind::Postgres => super::bulk_sql(schema, table, target, change),
        _ => sqlite::bulk_sql(table, columns, target, change),
    }
}

/// The relation as `kind`'s SQL names it: `"schema"."table"`, or SQLite's `"table"`.
pub fn relation_sql_for(kind: DbKind, schema: &str, table: &str) -> String {
    match kind {
        DbKind::Postgres => format!(
            "{}.{}",
            super::quote_ident(schema),
            super::quote_ident(table)
        ),
        _ => sqlite::quote(table),
    }
}

/// The statement that empties `table`: PostgreSQL's TRUNCATE, SQLite's DELETE (it has no
/// TRUNCATE).
pub fn empty_sql_for(kind: DbKind, schema: &str, table: &str) -> String {
    match kind {
        DbKind::Postgres => super::truncate_sql(schema, table),
        _ => format!("DELETE FROM {}", sqlite::quote(table)),
    }
}

/// The statement that deletes a table or view; SQLite has no CASCADE, so `cascade` is
/// PostgreSQL's only.
pub fn drop_sql_for(
    kind: DbKind,
    schema: &str,
    name: &str,
    rel: super::RelKind,
    cascade: bool,
) -> String {
    match kind {
        DbKind::Postgres => super::drop_sql(schema, name, rel, cascade),
        _ => format!(
            "DROP {} {}",
            if rel == super::RelKind::View {
                "VIEW"
            } else {
                "TABLE"
            },
            sqlite::quote(name)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::d1::LocalD1;
    use crate::db::explorer::tests::fake_explorer;

    fn local_d1(dir: &Path) -> (ConnProfile, PathBuf) {
        let file = dir.join("db.sqlite");
        let conn = rusqlite::Connection::open(&file).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; CREATE TABLE t (id INTEGER PRIMARY KEY, x TEXT); INSERT INTO t (x) VALUES ('a'), ('b');",
        )
        .unwrap();
        drop(conn);
        let profile = ConnProfile::d1_local(&LocalD1 {
            project: dir.to_path_buf(),
            worker: Some("api".into()),
            binding: "DB".into(),
            database_name: Some("shop".into()),
            miniflare_id: "5d0c".into(),
            file: file.clone(),
            exists: true,
        });
        (profile, file)
    }

    fn handle(profile: &ConnProfile, read_only: bool, addrs: Vec<SocketAddr>) -> Conn {
        let mut lite = Lite::new(profile.clone(), read_only);
        lite.explorer_addrs = Some(addrs);
        Conn::Lite(Arc::new(lite))
    }

    #[tokio::test]
    async fn writes_go_through_a_running_explorer_and_never_touch_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (profile, file) = local_d1(dir.path());
        let before = std::fs::read(&file).unwrap();
        let (addr, seen) = fake_explorer("api", "5d0c").await;
        let rw = handle(&profile, false, vec![addr]);
        let Conn::Lite(lite) = &rw else {
            unreachable!()
        };
        assert_eq!(
            lite.write_route().await,
            WriteRoute::Explorer {
                addr,
                id: "5d0c".into()
            }
        );
        let (out, route) = lite.write("DELETE FROM t", 10).await.unwrap();
        assert!(route.label().contains("Local Explorer"));
        assert_eq!(out.affected, [2]);
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|(line, body)| line.contains("/d1/database/5d0c/raw")
                    && body.contains("DELETE FROM t"))
        );
        // workerd (here, the fake) made the change; the file is as it was.
        assert_eq!(std::fs::read(&file).unwrap(), before);
        let n = rw
            .row_count_matching("main", "t", &RowQuery::default())
            .await
            .unwrap();
        assert_eq!(n.unwrap().rows, 2);
    }

    #[tokio::test]
    async fn without_a_dev_server_writes_go_to_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (profile, _) = local_d1(dir.path());
        let rw = handle(&profile, false, vec![]);
        let Conn::Lite(lite) = &rw else {
            unreachable!()
        };
        assert!(matches!(lite.write_route().await, WriteRoute::File(_)));
        assert_eq!(rw.run_bulk("DELETE FROM t WHERE x = 'a'").await.unwrap(), 1);
        // Reads take the read-only path, even on the read-write handle.
        let read = rw.run_sql("SELECT count(*) FROM t", 10).await.unwrap();
        assert_eq!(read.results[0].rows[0][0].as_deref(), Some("1"));
        let ro = handle(&profile, true, vec![]);
        let e = ro.run_bulk("DELETE FROM t").await.unwrap_err();
        assert!(e.to_string().contains("only reads"), "{e}");
        let e = ro.run_read_only("DELETE FROM t", 10).await.unwrap_err();
        assert!(e.to_string().contains("read-only"), "{e}");
    }

    /// By hand only, against a real `wrangler dev` in a scratch project:
    /// `SAWIT_D1_LIVE=<project folder> cargo test -p sawit-core live_wrangler_dev -- --ignored`.
    /// The project's `DB` binding needs a `products` table.
    #[tokio::test]
    #[ignore = "needs a running wrangler dev in a scratch project"]
    async fn live_wrangler_dev() {
        let project = PathBuf::from(std::env::var("SAWIT_D1_LIVE").unwrap());
        let d1 = crate::db::d1::find_local(&project, "DB").expect("a DB binding");
        assert!(d1.exists, "{}", d1.file.display());
        let profile = ConnProfile::d1_local(&d1);
        let rw = Conn::lite(profile.clone(), false);
        let Conn::Lite(lite) = &rw else {
            unreachable!()
        };
        let route = lite.write_route().await;
        assert!(matches!(route, WriteRoute::Explorer { .. }), "{route:?}");
        let (out, _) = lite
            .write(
                "INSERT INTO products (name, price) VALUES ('Sawit live', 1.0)",
                10,
            )
            .await
            .unwrap();
        assert_eq!(out.affected, [1]);
        // Read back from the file, read-only, while workerd holds it.
        let ro = Conn::lite(profile, true);
        let n = ro
            .run_read_only("SELECT count(*) FROM products WHERE name = 'Sawit live'", 1)
            .await
            .unwrap();
        assert_ne!(n.results[0].rows[0][0].as_deref(), Some("0"));
        assert!(!holders(&d1.file).await.is_empty());
    }

    #[test]
    fn statements_follow_the_kind() {
        assert_eq!(
            relation_sql_for(DbKind::Postgres, "public", "t"),
            r#""public"."t""#
        );
        assert_eq!(relation_sql_for(DbKind::D1Local, "main", "t"), r#""t""#);
        assert_eq!(
            empty_sql_for(DbKind::D1Remote, "main", "t"),
            r#"DELETE FROM "t""#
        );
        assert_eq!(
            drop_sql_for(DbKind::Sqlite, "main", "v", crate::db::RelKind::View, true),
            r#"DROP VIEW "v""#
        );
        let all = RowTarget::Matching(RowQuery::default());
        assert_eq!(
            bulk_sql_for(DbKind::D1Local, "main", "t", &[], &all, &BulkChange::Delete),
            r#"DELETE FROM "t""#
        );
    }

    #[test]
    fn routes_say_where_writes_go() {
        let remote = WriteRoute::Remote {
            database: "shop".into(),
        };
        assert_eq!(remote.label(), "remote (production)");
        assert!(remote.describe().contains("remote (production)"));
        let busy = WriteRoute::Busy {
            holders: vec!["workerd (pid 7)".into()],
        };
        assert!(
            busy.describe()
                .starts_with("workerd (pid 7) has this database open")
        );
    }
}
