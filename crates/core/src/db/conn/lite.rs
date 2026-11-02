//! A SQLite-backed database (a `.sqlite` file, a D1 binding's local file, or a remote D1
//! database through Wrangler), and the route its writes take.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::db::profile::{ConnProfile, DbKind};
use crate::db::query::{QueryOutcome, ReadOnlyTry};
use crate::db::{explorer, remote, sqlite};
use crate::{CoreError, Result};

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

    fn lite_source(&self) -> crate::db::LiteSource {
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

    pub(super) async fn remote_run(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
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
    pub(super) async fn remote_schema(&self, reload: bool) -> Result<Vec<(String, String)>> {
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

    pub(super) async fn remote_columns(&self, table: &str) -> Result<Vec<String>> {
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
    pub(super) async fn read<T: Send + 'static>(
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
