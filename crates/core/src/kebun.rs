//! The `sawit` command's terminal tools, which work without the app:
//! `sawit <db> [table] ["sql"]` like psql, `sawit list`, `sawit bakar`
//! ("burn": stop dev servers or instances, like the `kill-dev` script),
//! `sawit panen|backup|tanam` (export, `pg_dump`, CSV import),
//! `sawit pasang|pupuk|cabut` (install, upgrade or uninstall developer
//! tools) and `sawit nyalakan` (start instances). Two need the app:
//! `sawit browse` (a page in the app's browser), which starts it first, and
//! `sawit list keys`, which asks the running app and never starts it.
//! The wording riffs on Indonesia's forest fires (karhutla) and the palm-oil
//! plantations (kebun sawit) behind them.

use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::app_control::{self, Command, Request};
use crate::browser::{self, BrowserOp};
use crate::cli::BrowseTarget;
use crate::control::{self, Preflight, StartOutcome};
use crate::db::{
    self, ConnProfile, Profiles, format_table, parse_database_url, read_dotenv_database_url,
    resolve_password,
};
use crate::instances::{AppPaths, Instance, Registry};
use crate::pgctl::PgCtlStatus;
use crate::ports::PortJob;
use crate::probe;

/// Words `sawit` takes as commands rather than database names.
pub(crate) const SUBCOMMANDS: &[&str] = &[
    "list",
    "ls",
    "bakar",
    "export",
    "ekspor",
    "panen",
    "backup",
    "dump",
    "cadangkan",
    "import",
    "impor",
    "tanam",
    "install",
    "pasang",
    "upgrade",
    "pupuk",
    "uninstall",
    "cabut",
    "start",
    "nyalakan",
    "browse",
    "intip",
    "daftar",
    "stop",
];

/// Rows printed per result set at most.
const MAX_ROWS: usize = 10_000;

/// Rows shown by `sawit <db> <table>`.
const PREVIEW_ROWS: u32 = 100;

/// Command lines `bakar ports` never touches, even when they listen like a
/// dev server (same list as the `kill-dev` script).
pub const PROTECTED: &[&str] = &[
    "claude",
    "mcp-server",
    "language-server",
    "tsserver",
    "copilot",
    "code helper",
    "orca",
    "electron",
];

/// Tables and views of the user's schemas, with estimated row counts.
const LIST_TABLES: &str = "\
SELECT n.nspname AS schema, c.relname AS name,
       CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'matview'
                      WHEN 'f' THEN 'foreign' ELSE 'table' END AS kind,
       CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END AS est_rows
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
  AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%'
ORDER BY 1, 2";

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// `sawit <db> [table] ["sql"] [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryArgs {
    pub db: String,
    pub table: Option<String>,
    pub sql: Option<String>,
    /// Run a statement that writes without asking first (`-y`).
    pub yes: bool,
}

impl QueryArgs {
    /// More than a bare name: the user surely meant a database.
    fn explicit(&self) -> bool {
        self.table.is_some() || self.sql.is_some() || self.yes
    }
}

/// The arguments as `sawit <db> [table] ["sql"] [-y]` when they have that
/// shape: a bare name that isn't a path in `cwd`, then a table and/or SQL
/// (SQL has a space in it), and `-y` (`--yes`, `--ya`) anywhere. Whether
/// `db` really is a database is decided later. Subcommand names (`list`,
/// `bakar`) are never databases.
pub fn query_args(args: &[OsString], cwd: &Path) -> Option<QueryArgs> {
    let args: Vec<&str> = args.iter().map(|a| a.to_str()).collect::<Option<_>>()?;
    let is_yes = |a: &&str| matches!(*a, "-y" | "--yes" | "--ya");
    let yes = args.iter().any(is_yes);
    let args: Vec<&str> = args.into_iter().filter(|a| !is_yes(a)).collect();
    if !(1..=3).contains(&args.len()) || args.iter().any(|a| a.starts_with('-') || a.is_empty()) {
        return None;
    }
    let db = args[0];
    if SUBCOMMANDS.contains(&db) {
        return None;
    }
    let bare = db
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if !bare || cwd.join(db).exists() {
        return None;
    }
    let is_table = |t: &str| !t.contains(char::is_whitespace) && !t.contains('/');
    let (table, sql) = match &args[1..] {
        [] => (None, None),
        [one] if one.contains(char::is_whitespace) => (None, Some(*one)),
        [one] if is_table(one) => (Some(*one), None),
        [table, sql] if is_table(table) => (Some(*table), Some(*sql)),
        _ => return None,
    };
    Some(QueryArgs {
        db: db.into(),
        table: table.map(Into::into),
        sql: sql.map(Into::into),
        yes,
    })
}

/// What `sawit list` shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListWhat {
    All,
    Instances,
    Databases,
    Ports,
    Connections,
    /// Developer tools (only when asked: detecting them takes a moment).
    Tools,
    /// The MCP page: the catalog's servers and the AI clients that have
    /// them (only when asked, like tools).
    Mcp,
    /// The Skills page: the skill packs and the agents that have them (only
    /// when asked, like tools).
    Skills,
    /// The Config Files panel: shell startup files and coding agents' config
    /// files (only when asked, like tools).
    Config,
    /// Key bindings (only when asked: they need the running app).
    Keys,
    /// Cloudflare Local Explorer: the ones running, and the Workers of this folder's project
    /// (only when asked: it asks every dev server).
    Explorer,
}

impl ListWhat {
    pub fn parse(word: Option<&str>) -> Result<ListWhat, String> {
        Ok(match word {
            None | Some("all" | "semua") => ListWhat::All,
            Some("instances" | "instance" | "kebun") => ListWhat::Instances,
            Some("db" | "dbs" | "database" | "databases" | "blok") => ListWhat::Databases,
            Some("ports" | "port" | "lahan") => ListWhat::Ports,
            Some("connections" | "connection" | "conn" | "koneksi" | "konsesi") => {
                ListWhat::Connections
            }
            Some("tools" | "tool" | "alat" | "perkakas") => ListWhat::Tools,
            Some("mcp" | "colokan") => ListWhat::Mcp,
            Some("skills" | "skill" | "bibit") => ListWhat::Skills,
            Some("config" | "configs" | "konfig") => ListWhat::Config,
            Some("keys" | "key" | "keymap" | "shortcuts" | "pintasan") => ListWhat::Keys,
            Some("explorer" | "explorers" | "cloudflare" | "cf" | "jelajah" | "penjelajah") => {
                ListWhat::Explorer
            }
            Some(other) => {
                return Err(format!(
                    "`sawit list {other}`? Tidak ada di peta. Pilih: instances, db, ports, connections, tools, mcp, skills, config, keys, explorer."
                ));
            }
        })
    }

    fn shows(self, part: ListWhat) -> bool {
        self == part
            || (self == ListWhat::All
                && !matches!(
                    part,
                    ListWhat::Tools
                        | ListWhat::Mcp
                        | ListWhat::Skills
                        | ListWhat::Config
                        | ListWhat::Keys
                        | ListWhat::Explorer
                ))
    }
}

/// `sawit bakar ports|instances [names…] [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakarArgs {
    pub what: BakarWhat,
    /// Ports or instance names; empty means every candidate.
    pub targets: Vec<String>,
    /// Don't ask first.
    pub yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BakarWhat {
    Ports,
    Instances,
}

impl BakarArgs {
    pub fn parse(args: &[&str]) -> Result<BakarArgs, String> {
        let mut words = Vec::new();
        let mut yes = false;
        for &arg in args {
            match arg {
                "-y" | "--yes" | "--ya" => yes = true,
                _ if arg.starts_with('-') => {
                    return Err(format!("opsi tak dikenal: {arg} (yang ada cuma -y)"));
                }
                _ => words.push(arg),
            }
        }
        let what = match words.first().copied() {
            Some("ports" | "port" | "lahan") => BakarWhat::Ports,
            Some("instances" | "instance" | "kebun") => BakarWhat::Instances,
            Some(other) => {
                return Err(format!(
                    "`sawit bakar {other}`? Yang boleh dibakar cuma ports dan instances."
                ));
            }
            None => {
                return Err("bakar apa? `sawit bakar ports` atau `sawit bakar instances`.".into());
            }
        };
        let targets: Vec<String> = words[1..].iter().map(|w| w.to_string()).collect();
        if what == BakarWhat::Ports
            && let Some(bad) = targets.iter().find(|t| t.parse::<u16>().is_err())
        {
            return Err(format!("“{bad}” bukan nomor port."));
        }
        Ok(BakarArgs { what, targets, yes })
    }
}

/// `sawit nyalakan [name…] [--ulang] [-y]` (`start`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyalakanArgs {
    /// Instance names; empty means every stopped one (with `again`, every one).
    pub names: Vec<String>,
    /// `--ulang` (`--restart`): restart the running ones too.
    pub again: bool,
    /// Restart running ones without asking.
    pub yes: bool,
}

impl NyalakanArgs {
    pub fn parse(args: &[&str]) -> Result<NyalakanArgs, String> {
        let mut out = NyalakanArgs::default();
        for &arg in args {
            match arg {
                "--ulang" | "--restart" => out.again = true,
                "-y" | "--yes" | "--ya" => out.yes = true,
                _ if arg.starts_with('-') => {
                    return Err(format!(
                        "opsi tak dikenal: {arg} (yang ada cuma --ulang dan -y)"
                    ));
                }
                _ if !out.names.iter().any(|n| n == arg) => out.names.push(arg.to_string()),
                _ => {}
            }
        }
        Ok(out)
    }
}

/// What `sawit panen|backup|tanam` does with a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbJobKind {
    /// A table or query to CSV/Excel.
    Export,
    /// `pg_dump` of the database, a schema or a table.
    Backup,
    /// A CSV file into a table.
    Import,
}

/// `sawit panen <db> <table|"sql"> <file>`, `sawit backup <db>
/// [schema|schema.table] <file>`, `sawit tanam <db> <table> <file.csv>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbJob {
    pub kind: DbJobKind,
    pub db: String,
    /// The table, SQL, schema or `schema.table`.
    pub object: Option<String>,
    pub path: PathBuf,
    /// Replace an existing file without asking.
    pub yes: bool,
}

impl DbJob {
    /// The job a subcommand word names, if it names one.
    pub fn kind(word: &str) -> Option<DbJobKind> {
        match word {
            "export" | "ekspor" | "panen" => Some(DbJobKind::Export),
            "backup" | "dump" | "cadangkan" => Some(DbJobKind::Backup),
            "import" | "impor" | "tanam" => Some(DbJobKind::Import),
            _ => None,
        }
    }

    pub fn parse(kind: DbJobKind, args: &[&str], cwd: &Path) -> Result<DbJob, String> {
        let mut words = Vec::new();
        let mut yes = false;
        for &arg in args {
            match arg {
                "-y" | "--yes" | "--ya" => yes = true,
                _ if arg.starts_with('-') && arg.len() > 1 => {
                    return Err(format!("opsi tak dikenal: {arg} (yang ada cuma -y)"));
                }
                _ => words.push(arg),
            }
        }
        let usage = match kind {
            DbJobKind::Export => "sawit panen <db> <tabel|\"sql\"> <file.csv|file.xlsx>",
            DbJobKind::Backup => "sawit backup <db> [schema|schema.tabel] <file.sql|file.dump>",
            DbJobKind::Import => "sawit tanam <db> <tabel> <file.csv>",
        };
        let (db, object, file) = match (kind, words.as_slice()) {
            (DbJobKind::Backup, [db, file]) => (*db, None, *file),
            (_, [db, object, file]) => (*db, Some(*object), *file),
            _ => return Err(format!("cara pakainya: {usage}")),
        };
        let path = crate::cli::absolute(cwd, Path::new(file));
        if kind == DbJobKind::Export && db::ExportFormat::for_path(&path).is_err() {
            return Err(format!(
                "“{file}”: hasil panen cuma bisa .csv atau .xlsx. Karung lain belum ada izinnya."
            ));
        }
        Ok(DbJob {
            kind,
            db: db.into(),
            object: object.map(Into::into),
            path,
            yes,
        })
    }
}

/// `sawit pasang <tool…> [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasangArgs {
    pub tools: Vec<String>,
    pub yes: bool,
}

impl PasangArgs {
    pub fn parse(args: &[&str]) -> Result<PasangArgs, String> {
        let mut tools = Vec::new();
        let mut yes = false;
        for &arg in args {
            match arg {
                "-y" | "--yes" | "--ya" => yes = true,
                _ if arg.starts_with('-') => {
                    return Err(format!("opsi tak dikenal: {arg} (yang ada cuma -y)"));
                }
                _ if crate::dev_tools::find(arg).is_none() => {
                    return Err(format!(
                        "“{arg}” tidak ada di katalog bibit. Lihat `sawit list tools`."
                    ));
                }
                _ => tools.push(arg.to_string()),
            }
        }
        if tools.is_empty() {
            return Err(
                "pasang apa? Contoh: `sawit pasang bun uv`. Lihat `sawit list tools`.".into(),
            );
        }
        Ok(PasangArgs { tools, yes })
    }
}

/// `sawit pupuk [tool…] [-y]`: no tools means every outdated one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PupukArgs {
    pub tools: Vec<String>,
    pub yes: bool,
}

impl PupukArgs {
    pub fn parse(args: &[&str]) -> Result<PupukArgs, String> {
        let mut tools = Vec::new();
        let mut yes = false;
        for &arg in args {
            match arg {
                "-y" | "--yes" | "--ya" => yes = true,
                _ if arg.starts_with('-') => {
                    return Err(format!("opsi tak dikenal: {arg} (yang ada cuma -y)"));
                }
                _ if crate::dev_tools::find(arg).is_none() => {
                    return Err(format!(
                        "“{arg}” tidak ada di katalog bibit. Lihat `sawit list tools`."
                    ));
                }
                _ => tools.push(arg.to_string()),
            }
        }
        Ok(PupukArgs { tools, yes })
    }
}

/// `sawit cabut <tool…> [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CabutArgs {
    pub tools: Vec<String>,
    pub yes: bool,
}

impl CabutArgs {
    pub fn parse(args: &[&str]) -> Result<CabutArgs, String> {
        let mut tools = Vec::new();
        let mut yes = false;
        for &arg in args {
            match arg {
                "-y" | "--yes" | "--ya" => yes = true,
                _ if arg.starts_with('-') => {
                    return Err(format!("opsi tak dikenal: {arg} (yang ada cuma -y)"));
                }
                _ if crate::dev_tools::find(arg).is_none() => {
                    return Err(format!(
                        "“{arg}” tidak ada di katalog bibit. Lihat `sawit list tools`."
                    ));
                }
                _ => tools.push(arg.to_string()),
            }
        }
        if tools.is_empty() {
            return Err("cabut apa? Contoh: `sawit cabut bun`. Lihat `sawit list tools`.".into());
        }
        Ok(CabutArgs { tools, yes })
    }
}

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

/// First words of complete statements; anything else after a table name is
/// a filter.
const SQL_STARTS: &[&str] = &[
    "select",
    "with",
    "insert",
    "update",
    "delete",
    "merge",
    "explain",
    "show",
    "table",
    "values",
    "create",
    "alter",
    "drop",
    "truncate",
    "copy",
    "set",
    "reset",
    "begin",
    "start",
    "commit",
    "rollback",
    "grant",
    "revoke",
    "analyze",
    "vacuum",
    "refresh",
    "call",
    "do",
    "comment",
    "lock",
    "listen",
    "notify",
    "prepare",
    "execute",
    "deallocate",
    "discard",
    "reindex",
    "cluster",
    "declare",
    "fetch",
    "close",
];

/// Clauses that may follow `SELECT * FROM table` as they are.
const CLAUSE_STARTS: &[&str] = &["where", "order", "limit", "offset", "group", "fetch"];

/// The statement for `sawit <db> [table] ["sql"]`: the table list, a preview,
/// SQL as given, or a filter on the table (`"phone is null"` becomes
/// `SELECT * FROM leads WHERE phone is null`).
pub fn plan_sql(table: Option<&str>, sql: Option<&str>) -> String {
    let first_word = |s: &str| {
        s.trim_start()
            .trim_start_matches('(')
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    match (table, sql) {
        (None, None) => LIST_TABLES.into(),
        (None, Some(sql)) => sql.into(),
        (Some(table), None) => {
            format!("SELECT * FROM {} LIMIT {PREVIEW_ROWS}", table_ref(table))
        }
        (Some(table), Some(sql)) => {
            let word = first_word(sql);
            if SQL_STARTS.contains(&word.as_str()) {
                sql.into()
            } else if CLAUSE_STARTS.contains(&word.as_str()) {
                format!("SELECT * FROM {} {}", table_ref(table), sql.trim())
            } else {
                format!("SELECT * FROM {} WHERE {}", table_ref(table), sql.trim())
            }
        }
    }
}

/// `leads` → `"leads"`, `crm.leads` → `"crm"."leads"`; already quoted
/// names are left alone.
fn table_ref(table: &str) -> String {
    if table.contains('"') {
        return table.into();
    }
    table
        .split('.')
        .map(db::quote_ident)
        .collect::<Vec<_>>()
        .join(".")
}

// ---------------------------------------------------------------------------
// Finding the database
// ---------------------------------------------------------------------------

/// A database `sawit <db>` can connect to, and where it was found.
#[derive(Debug, Clone)]
pub struct Kebun {
    pub profile: ConnProfile,
    pub password: Option<String>,
    /// "instance shop", "koneksi tersimpan", ".env (…)", "Postgres localhost:5432".
    pub via: String,
}

impl Kebun {
    fn same_place(&self, other: &Kebun) -> bool {
        let (a, b) = (&self.profile, &other.profile);
        a.host == b.host && a.port == b.port && a.database == b.database
    }
}

/// Finds `name`: a Sawit instance or saved connection with that name, else
/// a database called `name` in a saved connection, a `.env` DATABASE_URL in
/// `cwd` or above, or a running local Postgres. Failing that, `kyzn` finds
/// `kyzn_crm` when no other database starts with it.
pub async fn find(name: &str, paths: &AppPaths, cwd: &Path) -> Result<Option<Kebun>, String> {
    let registry = Registry::load(&paths.instances_file()).unwrap_or_default();
    if let Some(inst) = registry.get(name) {
        return Ok(Some(Kebun {
            profile: ConnProfile::new("localhost", inst.port, "postgres", "postgres"),
            password: None,
            via: format!("instance {}", inst.name),
        }));
    }
    let profiles = saved_profiles(paths);
    if let Some(found) = profiles.iter().find(|k| k.profile.name == name) {
        return Ok(Some(found.clone()));
    }
    let mut candidates = profiles;
    candidates.extend(dotenv_kebuns(cwd));
    if let Some(found) = candidates.iter().find(|k| k.profile.database == name) {
        return Ok(Some(found.clone()));
    }
    candidates.extend(local_kebuns(&registry).await);
    if let Some(found) = candidates.iter().find(|k| k.profile.database == name) {
        return Ok(Some(found.clone()));
    }
    let mut partial: Vec<&Kebun> = Vec::new();
    for k in candidates
        .iter()
        .filter(|k| k.profile.database.starts_with(name))
    {
        if !partial.iter().any(|p| p.same_place(k)) {
            partial.push(k);
        }
    }
    match partial.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        many => Err(format!(
            "“{name}” cocok dengan {} kebun — sengketa lahan:\n{}\nSebut nama lengkapnya. Batas konsesi harus jelas, tidak seperti di peta resmi.",
            many.len(),
            many.iter()
                .map(|k| format!("   · {} ({})", k.profile.database, k.via))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

pub fn saved_profiles(paths: &AppPaths) -> Vec<Kebun> {
    let Ok(profiles) = Profiles::load(&paths.root.join("profiles.toml")) else {
        return Vec::new();
    };
    profiles
        .items
        .into_iter()
        .map(|profile| Kebun {
            password: resolve_password(&profile),
            via: format!("koneksi tersimpan {}", profile.name),
            profile,
        })
        .collect()
}

/// DATABASE_URL from `.env` in `cwd` and every folder above it.
pub fn dotenv_kebuns(cwd: &Path) -> Vec<Kebun> {
    cwd.ancestors()
        .map(|dir| dir.join(".env"))
        .filter_map(|path| {
            let url = read_dotenv_database_url(&path).ok()??;
            let (profile, password) = parse_database_url(&url).ok()?;
            Some(Kebun {
                password: password.or_else(|| resolve_password(&profile)),
                via: format!(".env di {}", tilde(path.parent()?)),
                profile,
            })
        })
        .collect()
}

/// Every database of the Postgres servers running here: Sawit instances,
/// then any other `postgres` listening on a port (Homebrew, Postgres.app).
pub async fn local_kebuns(registry: &Registry) -> Vec<Kebun> {
    let mut servers: Vec<(u16, String)> = registry
        .instances
        .iter()
        .map(|i| (i.port, format!("instance {}", i.name)))
        .collect();
    for job in probe::listening_jobs().await.unwrap_or_default() {
        let postgres = job
            .listeners
            .iter()
            .any(|(_, c)| c.starts_with("postgres") || c.starts_with("postmaster"));
        for (port, _) in job.ports.iter().filter(|_| postgres) {
            if !servers.iter().any(|(p, _)| p == port) {
                servers.push((*port, format!("Postgres localhost:{port}")));
            }
        }
    }
    let user = std::env::var("USER").unwrap_or_default();
    let lookups = servers.into_iter().map(|(port, via)| {
        let user = user.clone();
        async move {
            for user in ["postgres", user.as_str()] {
                let mut admin = ConnProfile::new("localhost", port, "postgres", user);
                admin.connect_timeout = Some(2);
                let password = resolve_password(&admin);
                let Ok(client) = db::connect(&admin, password.as_deref()).await else {
                    continue;
                };
                let names = db::list_databases(&client).await.unwrap_or_default();
                return names
                    .into_iter()
                    .map(|database| Kebun {
                        profile: ConnProfile {
                            name: format!("{user}@localhost:{port}/{database}"),
                            database,
                            ..admin.clone()
                        },
                        password: password.clone(),
                        via: via.clone(),
                    })
                    .collect();
            }
            Vec::new()
        }
    });
    futures::future::join_all(lookups)
        .await
        .into_iter()
        .flatten()
        .collect()
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/// Runs `fut` on a fresh Tokio runtime (the CLI runs before the app's).
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(fut)
}

/// `sawit <db> [table] ["sql"] [-y]`. `None` when `db` isn't a database and
/// the arguments could just as well be files to open. The SQL is tried
/// read-only first, as the `query` MCP tool runs it; SQL that writes is
/// shown, and runs once the user says yes (or with `-y`).
pub async fn query(args: QueryArgs, paths: &AppPaths, cwd: &Path) -> Option<i32> {
    let ink = Ink::stderr();
    let kebun = match find(&args.db, paths, cwd).await {
        Ok(Some(kebun)) => kebun,
        Ok(None) if !args.explicit() => return None,
        Ok(None) => {
            eprintln!(
                "{} Kebun “{}” tidak ada di peta konsesi mana pun.\n   Sudah dicek: instance Sawit, koneksi tersimpan, .env, dan Postgres lokal yang menyala.\n   Mungkin lahannya belum dibuka, atau sudah jadi abu. Lihat `sawit list db`.",
                ink.red("🔥"),
                args.db
            );
            return Some(1);
        }
        Err(e) => {
            eprintln!("{} {e}", ink.red("🔥"));
            return Some(1);
        }
    };
    let p = &kebun.profile;
    eprintln!(
        "{}",
        ink.gray(&format!(
            "🌴 {} · {}:{} · {}",
            p.database, p.host, p.port, kebun.via
        ))
    );
    // Read-only first: SQL that writes shows itself and asks before it runs.
    let reader = match db::connect_read_only(p, kebun.password.as_deref()).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{} Gagal masuk kebun {}: {e}", ink.red("🔥"), p.database);
            if let Some(name) = stopped_instance(p, paths).await {
                eprintln!("   Kebun {name} padam. Nyalakan dulu: `sawit nyalakan {name}`");
            }
            return Some(1);
        }
    };
    let sql = plan_sql(args.table.as_deref(), args.sql.as_deref());
    let why = match db::run_if_read_only(&reader, &sql, MAX_ROWS).await {
        Ok(db::ReadOnlyTry::Read(outcome)) => return Some(harvest(&outcome, &ink)),
        Ok(db::ReadOnlyTry::Writes(refusal)) => format!(
            "Pernyataan ini mengubah kebun; mode baca-saja menolaknya ({}).",
            refusal.lines().next().unwrap_or_default()
        ),
        Ok(db::ReadOnlyTry::Several(_)) => {
            "Ada beberapa pernyataan sekaligus, dan Sawit tidak bisa mencobanya baca-saja dulu."
                .into()
        }
        Err(e) => {
            eprintln!("{} Gagal panen: {e}", ink.red("🔥"));
            return Some(1);
        }
    };
    if !may_run_writes(&sql, &why, &p.database, args.yes, &ink) {
        return Some(1);
    }
    let writer = match db::connect(p, kebun.password.as_deref()).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{} Gagal masuk kebun {}: {e}", ink.red("🔥"), p.database);
            return Some(1);
        }
    };
    match db::run_sql(&writer, &sql, MAX_ROWS).await {
        Ok(outcome) => Some(harvest(&outcome, &ink)),
        Err(e) => {
            eprintln!("{} Gagal panen: {e}", ink.red("🔥"));
            Some(1)
        }
    }
}

/// The name of the Sawit instance `profile` connects to, when it is one and it is stopped: a
/// connection that failed then needs `sawit nyalakan <name>`.
async fn stopped_instance(profile: &ConnProfile, paths: &AppPaths) -> Option<String> {
    if !matches!(profile.host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return None;
    }
    let registry = Registry::load(&paths.instances_file()).ok()?;
    let inst = registry
        .instances
        .iter()
        .find(|inst| inst.port == profile.port)?;
    let stopped = matches!(control::status(inst).await, Ok(PgCtlStatus::Stopped));
    stopped.then(|| inst.name.clone())
}

/// Whether `sql`, which the read-only try refused (`why`), may run in `database` as it is: with
/// `-y`, or when the user says yes. The statement and why it asks come first, on stderr, where
/// `sawit <db>` says all but its tables. Without a terminal to ask on, and without `-y`, it may not.
fn may_run_writes(sql: &str, why: &str, database: &str, yes: bool, ink: &Ink) -> bool {
    eprintln!("{} {why}", ink.yellow("!"));
    for line in statement_preview(sql) {
        eprintln!("     {}", ink.gray(&line));
    }
    if yes {
        return true;
    }
    match confirm_on_stderr(&format!(
        "Jalankan di {database}? Lahan yang terbakar tidak tumbuh lagi."
    )) {
        Some(true) => true,
        Some(false) => {
            eprintln!("{}", ink.gray("🌾 Batal. Kebun tidak disentuh."));
            false
        }
        None => {
            eprintln!(
                "{} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau menjalankannya.",
                ink.red("🔥")
            );
            false
        }
    }
}

/// The lines of `sql` shown before `sawit <db>` asks to run it: the first 20 (each cut at 200
/// characters), then how many more there are.
fn statement_preview(sql: &str) -> Vec<String> {
    const SHOWN: usize = 20;
    let lines: Vec<&str> = sql.trim().lines().collect();
    let mut shown: Vec<String> = lines
        .iter()
        .take(SHOWN)
        .map(|line| clip(line, 200))
        .collect();
    if lines.len() > SHOWN {
        shown.push(format!("… ({} baris lagi)", lines.len() - SHOWN));
    }
    shown
}

/// Prints the result sets of `outcome` on stdout, and its tally on stderr; the exit code.
fn harvest(outcome: &db::QueryOutcome, ink: &Ink) -> i32 {
    let mut out = std::io::stdout().lock();
    for (ix, set) in outcome.results.iter().enumerate() {
        let gap = if ix > 0 { "\n" } else { "" };
        let _ = writeln!(out, "{gap}{}", format_table(set));
    }
    let _ = out.flush();
    let rows: usize = outcome.results.iter().map(|s| s.rows.len()).sum();
    let mut summary = vec![format!("panen {rows} baris")];
    if let Some(&n) = outcome.affected.last() {
        summary.push(format!("{n} baris terdampak"));
    }
    if outcome.results.iter().any(|s| s.truncated) {
        summary.push(format!(
            "dibatasi {MAX_ROWS} baris, sisanya lahan gambut, jangan disentuh"
        ));
    }
    summary.push(format!("{} ms", outcome.duration.as_millis()));
    eprintln!("{}", ink.gray(&format!("🌾 {}", summary.join(" · "))));
    0
}

/// `sawit list [what]`.
pub async fn list(what: ListWhat, paths: &AppPaths, cwd: &Path) -> i32 {
    let ink = Ink::stdout();
    let registry = Registry::load(&paths.instances_file()).unwrap_or_default();
    if what.shows(ListWhat::Instances) {
        ink.section("🌴 Kebun (instances)");
        if registry.instances.is_empty() {
            ink.empty("belum ada kebun. Hutannya masih utuh, untuk sementara.");
        }
        let mut padam = false;
        for inst in &registry.instances {
            let status = match control::status(inst).await {
                Ok(PgCtlStatus::Running { pid }) => ink.green(&format!("menyala (PID {pid})")),
                Ok(PgCtlStatus::Stopped) => {
                    padam = true;
                    ink.gray("padam")
                }
                Ok(other) => ink.yellow(&format!("{other:?}")),
                Err(e) => ink.red(&format!("error: {e}")),
            };
            println!(
                "  {:<16} :{:<6} {status}  {}",
                inst.name,
                inst.port,
                ink.gray(&format!(
                    "postgres://postgres@localhost:{}/postgres",
                    inst.port
                ))
            );
        }
        if padam {
            println!(
                "  {}",
                ink.gray("Yang padam dinyalakan lagi dengan `sawit nyalakan <nama>`.")
            );
        }
    }
    if what.shows(ListWhat::Databases) {
        ink.section("🌾 Blok tanam (databases)");
        let mut kebuns = local_kebuns(&registry).await;
        kebuns.extend(dotenv_kebuns(cwd));
        kebuns.extend(saved_profiles(paths));
        if kebuns.is_empty() {
            ink.empty("tidak ada Postgres yang menyala. Udara bersih, ISPU hijau.");
        }
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for k in &kebuns {
            let place = format!("{}:{}", k.profile.host, k.profile.port);
            let key = match k.via.contains(&place) {
                true => k.via.clone(),
                false => format!("{} · {place}", k.via),
            };
            match groups.iter_mut().find(|(g, _)| *g == key) {
                Some((_, dbs)) => dbs.push(k.profile.database.clone()),
                None => groups.push((key, vec![k.profile.database.clone()])),
            }
        }
        for (place, dbs) in groups {
            println!("  {}  {}", dbs.join(", "), ink.gray(&place));
        }
    }
    if what.shows(ListWhat::Ports) {
        ink.section("🔥 Lahan terbuka (ports)");
        match probe::listening_jobs().await {
            Ok(jobs) if jobs.is_empty() => ink.empty("tidak ada port terbuka. Nihil titik api."),
            Ok(jobs) => {
                let instance_ports: Vec<u16> = registry.instances.iter().map(|i| i.port).collect();
                for job in &jobs {
                    let tag = if burnable(job, &instance_ports) {
                        ink.red("titik api")
                    } else if job.is_docker() {
                        ink.gray("docker")
                    } else if job.is_daemon() {
                        ink.gray("server")
                    } else {
                        ink.gray("aplikasi")
                    };
                    println!(
                        "  {:<14} {}  {}  {}",
                        ports_of(job),
                        clip(&job.label, 60),
                        ink.gray(&format!("PID {}", job.root)),
                        tag
                    );
                    if let Some(cwd) = &job.cwd {
                        println!(
                            "  {:<14} {}",
                            "",
                            ink.gray(&format!("└ {}", tilde(Path::new(cwd))))
                        );
                    }
                }
                println!(
                    "  {}",
                    ink.gray("“titik api” = yang dibakar oleh `sawit bakar ports`.")
                );
            }
            Err(e) => println!("  {}", ink.red(&format!("lsof gagal: {e}"))),
        }
    }
    if what.shows(ListWhat::Connections) {
        ink.section("📜 Konsesi (connections)");
        let profiles = saved_profiles(paths);
        if profiles.is_empty() {
            ink.empty("belum ada izin konsesi. Belum ada yang “diatur”.");
        }
        for k in profiles {
            println!("  {}  {}", k.profile.name, ink.gray(&k.profile.url()));
        }
    }
    if what.shows(ListWhat::Tools) {
        let (detection, latest) = futures::join!(
            crate::dev_tools::detect(),
            crate::dev_tools::latest_versions()
        );
        let found = detection.found;
        if let Some(problem) = &detection.problem {
            ink.warn(&format!(
                "Shell login tidak menjawab ({problem}): hanya alat di tempat biasanya yang terdeteksi."
            ));
        }
        for category in crate::dev_tools::Category::ALL {
            ink.section(&format!("🧰 {}", category.title()));
            for t in crate::dev_tools::TOOLS
                .iter()
                .filter(|t| t.category == category)
            {
                let state = match found.get(t.id) {
                    Some(f) => {
                        let installed = ink.green(&format!(
                            "terpasang{}",
                            f.version
                                .as_ref()
                                .map(|v| format!(" {v}"))
                                .unwrap_or_default()
                        ));
                        match (crate::dev_tools::newer(t.id, &found, &latest), f.outside) {
                            (Some(v), _) => {
                                format!("{installed} {}", ink.yellow(&format!("→ {v}")))
                            }
                            // Sawit neither upgrades nor removes it.
                            (None, Some(crate::dev_tools::Outside::Homebrew)) => {
                                format!("{installed} {}", ink.gray("(di luar Homebrew)"))
                            }
                            (None, Some(crate::dev_tools::Outside::InstallScript)) => {
                                format!("{installed} {}", ink.gray("(di luar skrip instalnya)"))
                            }
                            (None, None) => installed,
                        }
                    }
                    None => ink.gray("belum"),
                };
                let place = found
                    .get(t.id)
                    .map(|f| format!("  {}", ink.gray(&tilde(&f.path))))
                    .unwrap_or_default();
                println!("  {:<14} {:<22} {state}{place}", t.id, clip(t.name, 22));
            }
        }
        println!(
            "  {}",
            ink.gray("Pasang dengan `sawit pasang <id…>`; prasyaratnya ikut ditanam duluan.")
        );
        println!(
            "  {}",
            ink.gray("Yang bertanda → ada versi baru: `sawit pupuk` memupuk semuanya.")
        );
        println!(
            "  {}",
            ink.gray("Cabut dengan `sawit cabut <id…>`: hanya salinan dari installer-nya, Homebrew atau npm.")
        );
    }
    if what.shows(ListWhat::Mcp) {
        list_mcp(&ink);
    }
    if what.shows(ListWhat::Skills) {
        list_skills(&ink, paths).await;
    }
    if what.shows(ListWhat::Config) {
        list_config(&ink, cwd);
    }
    if what.shows(ListWhat::Explorer) {
        list_explorer(&ink, cwd).await;
    }
    if what.shows(ListWhat::Keys) {
        let code = list_keys(&ink, paths);
        if code != 0 {
            return code;
        }
    }
    println!();
    0
}

/// `sawit list explorer`: the Local Explorers Workers dev servers serve (`wrangler dev`, `vite`
/// with the Cloudflare plugin, `cf dev`), as `sawit-mcp`'s `local_explorer` finds them, and the
/// Workers of the project in `cwd` with what their dev tool needs for one.
async fn list_explorer(ink: &Ink, cwd: &Path) {
    use crate::cloudflare::{self, explorer};
    ink.section("🧭 Local Explorer (Cloudflare)");
    let explorers = match probe::listening_jobs().await {
        Ok(jobs) => explorer::find(&jobs).await,
        Err(e) => {
            println!("  {}", ink.red(&format!("lsof gagal: {e}")));
            Vec::new()
        }
    };
    if explorers.is_empty() {
        ink.empty(
            "tidak ada yang menyala. Jalankan dev server Workers-nya (wrangler dev, vite, cf dev).",
        );
    }
    for e in &explorers {
        println!(
            "  {:<7} {}  {}  {}",
            format!(":{}", e.port),
            ink.green(&e.url()),
            clip(&e.label, 40),
            ink.gray(&format!("PID {}", e.pid))
        );
        if let Some(dir) = &e.cwd {
            println!(
                "  {:<7} {}",
                "",
                ink.gray(&format!("└ {}", tilde(Path::new(dir))))
            );
        }
    }
    let workers = cloudflare::detect_project(cwd);
    if workers.is_empty() {
        return;
    }
    ink.section("☁️ Workers di folder ini");
    for w in &workers {
        let tool = match &w.version {
            Some(v) => format!("{} {v}", w.tool.name()),
            None => w.tool.name().to_string(),
        };
        let rel = w
            .root
            .strip_prefix(cwd)
            .ok()
            .filter(|r| !r.as_os_str().is_empty())
            .map(|r| format!("{}/", r.display()))
            .unwrap_or_default();
        let bindings = w
            .bindings
            .iter()
            .map(|b| format!("{} {}", b.kind.label(), b.binding))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "  {:<22} {}  {}",
            clip(w.name.as_deref().unwrap_or("(tanpa nama)"), 22),
            ink.gray(&format!("{rel}{} · {tool}", w.config_name())),
            bindings
        );
        let running = explorers.iter().any(|e| e.runs_in(&w.root));
        if let Some(note) = cloudflare::requirement_note(w, cwd) {
            println!("  {:<22} {}", "", ink.yellow(&format!("└ {note}")));
        }
        if !running && let Some(script) = cloudflare::dev_scripts(w).first() {
            println!(
                "  {:<22} {}",
                "",
                ink.gray(&format!("└ nyalakan: {} ({})", script.name, script.body))
            );
        }
    }
}

/// `sawit list keys`: every key binding, as the Keymap Editor lists it, from the running app
/// (`sawit-mcp`'s `keymap_list` asks the same). The defaults are bound in the app's code, so only
/// the running app knows them: when it isn't running, this says so rather than start it.
fn list_keys(ink: &Ink, paths: &AppPaths) -> i32 {
    let problem = Ink::stderr();
    let keymap = tilde(&paths.keymap_file());
    if !crate::app_control::is_running(paths) {
        eprintln!(
            "{} Sawit sedang padam, jadi peta pintasannya ikut gelap: pintasan bawaan hanya hidup di dalam aplikasi.\n   Nyalakan dulu dengan `sawit`, lalu ulangi `sawit list keys`. Pintasanmu sendiri ada di {keymap}.",
            problem.red("🔥")
        );
        return 1;
    }
    let request = crate::app_control::Request {
        project: None,
        agent_folder: None,
        agent: None,
        command: crate::app_control::Command::Keymap {
            op: crate::keymap::KeymapOp::List {
                filter: None,
                keys: None,
                unbound: false,
            },
        },
    };
    let list = crate::app_control::send(paths, &request, Duration::from_secs(10)).and_then(|v| {
        serde_json::from_value::<crate::keymap::KeymapList>(v)
            .map_err(|e| format!("Sawit menjawab dengan peta yang tidak terbaca: {e}"))
    });
    let list = match list {
        Ok(list) => list,
        Err(e) => {
            eprintln!("{} Gagal membaca peta pintasan: {e}", problem.red("🔥"));
            return 1;
        }
    };
    ink.section("⌨️  Pintasan (keys)");
    if list.rows.is_empty() {
        ink.empty("tidak ada satu pun pintasan. Lahannya kosong melompong.");
    }
    for row in &list.rows {
        println!("{}", key_line(ink, row));
    }
    for error in &list.errors {
        ink.warn(&format!("keymap.json: {error}"));
    }
    let count = |source: crate::keymap::KeySource| {
        list.rows.iter().filter(|row| row.source == source).count()
    };
    let clashing = list.rows.iter().filter(|row| row.conflict).count();
    println!(
        "  {}",
        ink.gray(&format!(
            "{} pintasan · {} milikmu · {} dicabut · {clashing} bentrok",
            list.rows.len(),
            count(crate::keymap::KeySource::User),
            count(crate::keymap::KeySource::Removed),
        ))
    );
    println!(
        "  {}",
        ink.gray(&format!(
            "“dicabut” = bawaan yang kamu matikan; “bentrok” = tombol yang sama di konteks yang sama, cuma satu yang jalan. Ubah di Keymap Editor (⌘K ⌘S) atau di {keymap}."
        ))
    );
    0
}

/// One binding as `sawit list keys` prints it: the keys as a Mac menu writes them and as
/// keymap.json does, what they run, where, and whose binding it is.
fn key_line(ink: &Ink, row: &crate::keymap::KeymapRow) -> String {
    use crate::keymap::KeySource;
    let source = match row.source {
        KeySource::Default => ink.gray("bawaan"),
        KeySource::User => ink.green("milikmu"),
        KeySource::Removed => ink.yellow("dicabut"),
        KeySource::Unbound => ink.gray("tanpa pintasan"),
    };
    let clash = match row.conflict {
        true => format!(" {}", ink.red("bentrok")),
        false => String::new(),
    };
    let context = row.context.as_deref().unwrap_or("di mana saja");
    format!(
        "  {:<12} {:<44} {}  {source}{clash}",
        row.display,
        clip(&row.label, 44),
        ink.gray(&format!("{} · {}", row.keys, clip(context, 40))),
    )
}

/// The `sawit-mcp` next to this program, which the MCP page installs as
/// Sawit's own server: the app's binary, also when `sawit` is reached
/// through the `/usr/local/bin/sawit` link.
fn own_mcp_server() -> PathBuf {
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sawit-mcp")))
        .unwrap_or_else(|| PathBuf::from("sawit-mcp"))
}

/// A client as `sawit list mcp` names it: "Codex", "Claude Code (profil work)".
fn client_name(target: &crate::mcp_clients::Target) -> String {
    match target.profile.as_str() {
        "default" => target.client.name().to_string(),
        profile => format!("{} (profil {profile})", target.client.name()),
    }
}

/// `sawit list mcp`: the MCP page's servers and the clients that have them.
fn list_mcp(ink: &Ink) {
    use crate::mcp_catalog::{self, Auth, DEFAULT_AWS_REGION, Source};
    use crate::mcp_clients::Status;
    let report = mcp_catalog::report(&own_mcp_server());
    ink.section("🔌 Colokan MCP (servers)");
    let clients: Vec<String> = report.targets.iter().map(client_name).collect();
    if clients.is_empty() {
        ink.empty("belum ada klien AI di Mac ini. Kebunnya masih digarap tangan manusia.");
    } else {
        println!("  {} {}", ink.gray("klien:"), clients.join(", "));
    }
    // A config that can't be read, or that Sawit doesn't edit (YAML, a
    // script), is so for every server: said once, here, not under each.
    let mut noted: Vec<usize> = Vec::new();
    for (ix, status) in report.servers.iter().flat_map(|l| &l.installed) {
        let note = match status {
            Status::Unreadable(why) => format!("config tak terbaca ({why}): periksa sendiri"),
            Status::Manual => "config-nya YAML atau skrip: server ditambahkan sendiri".to_string(),
            _ => continue,
        };
        if !noted.contains(ix) {
            noted.push(*ix);
            ink.warn(&format!("{}: {note}", client_name(&report.targets[*ix])));
        }
    }
    for listed in &report.servers {
        let server = listed.server;
        let address = match server.source {
            Source::Bundled => "bawaan Sawit".to_string(),
            _ => server.address(DEFAULT_AWS_REGION),
        };
        println!(
            "  {:<24} {:<20} {}",
            server.id,
            clip(server.name, 20),
            ink.gray(&address)
        );
        let pad = " ".repeat(24);
        // Only names: what a key holds never comes near this list.
        let sign_in = match server.auth {
            Auth::None => None,
            Auth::Browser => Some("masuk lewat browser dari kliennya".to_string()),
            Auth::EnvKey(var) => Some(format!("butuh variabel {var} sebelum kliennya dibuka")),
            Auth::Program => Some("pakai login programnya sendiri".to_string()),
        };
        if let Some(sign_in) = sign_in {
            println!("  {pad} {}", ink.gray(&sign_in));
        }
        if let Some(program) = listed.missing_program {
            println!(
                "  {pad} {}",
                ink.yellow(&format!("! {program} belum terpasang"))
            );
        }
        // The clients that have it; the ones noted above are left out.
        let installed: Vec<String> = listed
            .installed
            .iter()
            .filter_map(|(ix, status)| {
                let name = client_name(&report.targets[*ix]);
                match status {
                    Status::Installed => Some(name),
                    Status::Outdated(sig) => Some(match server.region_in(sig) {
                        Some(region) => format!("{name} (region {region})"),
                        None => format!("{name} (menunjuk ke tempat lain; pasang ulang)"),
                    }),
                    _ => None,
                }
            })
            .collect();
        if installed.is_empty() {
            println!("  {pad} {}", ink.gray("belum di klien mana pun"));
        } else {
            println!("  {pad} {} {}", ink.green("✓"), installed.join(", "));
        }
    }
    println!(
        "  {}",
        ink.gray("Pasang dari halaman MCP di app; agen pakai `install_mcp_server`.")
    );
}

/// `sawit list skills`: the Skills page's packs, how each installs (the
/// skills CLI with the package manager picked last, a Claude Code plugin, a
/// clone and copy) and the agents that have it.
async fn list_skills(ink: &Ink, paths: &AppPaths) {
    use crate::package_manager::{self, PackageManager, Use};
    use crate::skills::{self, Status};
    let available = package_manager::detect().await;
    let pm = package_manager::choose(
        &available,
        None,
        package_manager::remembered(paths),
        Use::Run,
    )
    .unwrap_or(PackageManager::Npm);
    let report = skills::report();
    ink.section("🌱 Bibit (skills)");
    if report.targets.is_empty() {
        ink.empty("belum ada agen dengan folder skills di Mac ini.");
    } else {
        let agents: Vec<String> = report
            .targets
            .iter()
            .map(|t| format!("{} ({})", t.label, tilde(&t.dir)))
            .collect();
        println!("  {} {}", ink.gray("agen:"), agents.join("; "));
    }
    for pack in skills::PACKS {
        println!(
            "  {:<28} {:<26} {}",
            pack.id,
            clip(pack.name, 26),
            ink.gray(&format!("{} · {}", pack.by, pack.repo))
        );
        let pad = " ".repeat(28);
        for method in pack.methods {
            println!("  {pad} {}", ink.gray(&skills::sample(pack, *method, pm)));
        }
        let have: Vec<String> = (0..report.targets.len())
            .filter_map(|t| {
                let label = &report.targets[t].label;
                match report.status(pack, t) {
                    Status::Installed { plugin: true } => Some(format!("{label} (plugin)")),
                    Status::Installed { plugin: false } => Some(label.clone()),
                    Status::Partial { have, of } => Some(format!("{label} ({have}/{of})")),
                    Status::Missing => None,
                }
            })
            .collect();
        if have.is_empty() {
            println!("  {pad} {}", ink.gray("belum di agen mana pun"));
        } else {
            println!("  {pad} {} {}", ink.green("✓"), have.join(", "));
        }
    }
    println!(
        "  {}",
        ink.gray(&format!(
            "Pasang dari halaman Skills di app (skills CLI lewat {}); agen pakai `install_skill`.",
            pm.runner()
        ))
    );
}

/// `sawit list config`: the Config Files panel, for the current folder's
/// project. Paths only: what a file holds (API keys, say) isn't shown.
fn list_config(ink: &Ink, cwd: &Path) {
    use crate::{agent_config, shell_config};
    let login = shell_config::login_shell();
    let shells = shell_config::detect();
    ink.section(&match &login {
        Some(login) => format!("🐚 Startup shell (login: {login})"),
        None => "🐚 Startup shell".to_string(),
    });
    if shells.is_empty() {
        ink.empty("tidak ada berkas startup shell. Lahan bersih, tanpa dokumen AMDAL.");
    }
    for c in &shells {
        let read = shell_config::reads(login.as_deref(), c.shell);
        let mut notes = vec![c.note.to_string()];
        if !read {
            notes.push(format!(
                "tidak dibaca {}",
                login.as_deref().unwrap_or("shell login")
            ));
        }
        if !c.writable {
            notes.push("simpan butuh sandi admin".into());
        }
        let notes = notes.join(" · ");
        if read {
            println!("  {:<34} {}", tilde(&c.path), ink.gray(&notes));
        } else {
            // Dimmed, like the panel's row: the login shell never reads it.
            println!("  {}", ink.gray(&format!("{:<34} {notes}", tilde(&c.path))));
        }
    }
    // The current folder is the project, unless it is the home folder or `/`.
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let projects: Vec<PathBuf> = (cwd.parent().is_some() && Some(cwd) != home.as_deref())
        .then(|| cwd.to_path_buf())
        .into_iter()
        .collect();
    let agents = agent_config::detect(&projects);
    ink.section("🤖 Agen AI (prioritas tertinggi duluan)");
    if agents.is_empty() {
        ink.empty("tidak ada agen AI terpasang. Kebun ini masih dikerjakan tangan manusia.");
    }
    for (client, profile, files) in agent_config::groups(&agents) {
        println!("  {}", agent_config::group_title(client, profile));
        for f in files {
            // A project's file by the project's name: `app/CLAUDE.md`.
            let name = match &f.project {
                Some(project) => format!(
                    "{}/{}",
                    project
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    f.path.strip_prefix(project).unwrap_or(&f.path).display()
                ),
                None => tilde(&f.path),
            };
            let mut notes = format!("{} · {}", f.layer.label(), f.note);
            if !f.writable {
                notes.push_str(" · simpan butuh sandi admin");
            }
            println!("    {:<40} {}", name, ink.gray(&notes));
        }
    }
    println!(
        "  {}",
        ink.gray("Buka dengan `sawit <path>`. Isinya tidak ditampilkan: bisa ada kunci API.")
    );
}

/// What `bakar ports` takes when no port is named: dev servers, never
/// databases, Docker, Sawit's instances or protected tools.
pub fn burnable(job: &PortJob, instance_ports: &[u16]) -> bool {
    job.dev
        && !job.is_docker()
        && !job.is_daemon()
        && !protected(job)
        && !job.ports.iter().any(|(p, _)| instance_ports.contains(p))
}

pub fn protected(job: &PortJob) -> bool {
    let cmd = job.cmdline.to_lowercase();
    PROTECTED.iter().any(|p| cmd.contains(p))
}

fn ports_of(job: &PortJob) -> String {
    job.ports
        .iter()
        .map(|(p, _)| format!(":{p}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `sawit bakar …`.
pub async fn bakar(args: BakarArgs, paths: &AppPaths) -> i32 {
    match args.what {
        BakarWhat::Ports => bakar_ports(args, paths).await,
        BakarWhat::Instances => bakar_instances(args, paths).await,
    }
}

async fn bakar_ports(args: BakarArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    ink.header(
        "🔥 Bakar Lahan (ports)",
        if args.targets.is_empty() {
            "hanya dev server. Database, Docker & proses penting dilindungi (kali ini)"
        } else {
            "port pilihan sendiri. Izinnya, seperti biasa, menyusul"
        },
    );
    let jobs = match probe::listening_jobs().await {
        Ok(jobs) => jobs,
        Err(e) => {
            println!("  {} lsof gagal: {e}", ink.red("✗"));
            return 1;
        }
    };
    let registry = Registry::load(&paths.instances_file()).unwrap_or_default();
    let instance_ports: Vec<u16> = registry.instances.iter().map(|i| i.port).collect();
    let mut victims: Vec<&PortJob> = Vec::new();
    if args.targets.is_empty() {
        victims.extend(jobs.iter().filter(|j| burnable(j, &instance_ports)));
    }
    for port in args.targets.iter().filter_map(|t| t.parse::<u16>().ok()) {
        let Some(job) = jobs
            .iter()
            .find(|j| j.ports.iter().any(|(p, _)| *p == port))
        else {
            ink.warn(&format!(":{port} tidak ada api di sana."));
            continue;
        };
        if job.is_docker() {
            ink.warn(&format!(
                ":{port} milik Docker. Padamkan dari Docker; membakar lahan tetangga itu urusan korporasi."
            ));
        } else if protected(job) {
            ink.warn(&format!(":{port} ({}) dilindungi.", job.label));
        } else if instance_ports.contains(&port) {
            ink.warn(&format!(
                ":{port} itu instance Sawit. Pakai `sawit bakar instances`."
            ));
        } else if !victims.iter().any(|v| v.root == job.root) {
            victims.push(job);
        }
    }
    if victims.is_empty() {
        ink.empty("tidak ada titik api. Mungkin sudah dibakar duluan oleh oknum.");
        ink.footer("Tidak ada yang dibakar.");
        return 0;
    }
    ink.section(&format!("Titik api ({})", victims.len()));
    for (ix, job) in victims.iter().enumerate() {
        let kids = job.pids.len() - 1;
        let family = if kids > 0 {
            format!(" (+{kids} anak)")
        } else {
            String::new()
        };
        println!(
            "  {} {:<14} {}  {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            ports_of(job),
            clip(&job.label, 60),
            ink.gray(&format!("PID {}{family}", job.root))
        );
        if let Some(cwd) = &job.cwd {
            println!(
                "       {}",
                ink.gray(&format!("└ {}", tilde(Path::new(cwd))))
            );
        }
    }
    println!();
    if !args.yes {
        match confirm(&format!(
            "Bakar {} lahan di atas? Asapnya tanggung sendiri.",
            victims.len()
        )) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Lahan selamat, untuk musim ini.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau membakar.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }

    let pids: Vec<u32> = victims
        .iter()
        .flat_map(|j| j.pids.iter().copied())
        .collect();
    let mut refused = Vec::new();
    for &pid in &pids {
        if let Err(e) = probe::terminate(pid, false) {
            refused.push(e);
        }
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let survivors: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|&p| probe::is_alive(p))
        .collect();
    if !survivors.is_empty() {
        ink.warn(&format!(
            "{} proses tidak padam disiram SIGTERM. Kirim water bombing (SIGKILL).",
            survivors.len()
        ));
        for &pid in &survivors {
            let _ = probe::terminate(pid, true);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let burned = victims
        .iter()
        .filter(|j| j.pids.iter().all(|&p| !probe::is_alive(p)))
        .count();
    println!();
    ink.kv("Hangus", &format!("{burned} / {} lahan", victims.len()));
    if burned < victims.len() {
        if let Some(e) = refused.first() {
            ink.warn(e);
        }
        ink.warn("Sisanya milik user lain atau butuh sudo. Pelakunya, seperti biasa, tak terjangkau hukum.");
    }
    ink.footer("Selesai. Asap menyebar, status resmi: “terkendali”.");
    i32::from(burned < victims.len())
}

async fn bakar_instances(args: BakarArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    ink.header(
        "🧯 Padamkan Kebun (instances)",
        "pg_ctl stop -m fast. Datanya tidak ikut terbakar, tenang",
    );
    let registry = match Registry::load(&paths.instances_file()) {
        Ok(registry) => registry,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    if let Some(unknown) = args.targets.iter().find(|t| registry.get(t).is_none()) {
        println!(
            "  {} Kebun “{unknown}” tidak terdaftar. Ilegal, ya? Lihat `sawit list instances`.",
            ink.red("✗")
        );
        return 1;
    }
    let mut burning = Vec::new();
    for inst in &registry.instances {
        if !args.targets.is_empty() && !args.targets.contains(&inst.name) {
            continue;
        }
        if let Ok(PgCtlStatus::Running { pid }) = control::status(inst).await {
            burning.push((inst, pid));
        }
    }
    if burning.is_empty() {
        ink.empty("semua kebun sudah padam. Hujan buatan berhasil, katanya.");
        ink.footer("Tidak ada yang dipadamkan.");
        return 0;
    }
    ink.section(&format!("Kebun menyala ({})", burning.len()));
    for (ix, (inst, pid)) in burning.iter().enumerate() {
        println!(
            "  {} {:<16} :{:<6} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            inst.name,
            inst.port,
            ink.gray(&format!("PID {pid}"))
        );
    }
    println!();
    if !args.yes {
        match confirm(&format!("Padamkan {} kebun di atas?", burning.len())) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Kebun tetap menyala, asap tetap mengepul.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y untuk memadamkan.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    let mut out = 0;
    for (inst, _) in &burning {
        match control::stop(inst).await {
            Ok(()) => println!("  {} {} padam", ink.green("✓"), inst.name),
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), inst.name);
                out += 1;
            }
        }
    }
    println!();
    ink.kv(
        "Padam",
        &format!("{} / {} kebun", burning.len() - out, burning.len()),
    );
    ink.footer("Selesai. Datanya aman, tidak seperti hutannya.");
    i32::from(out > 0)
}

/// `sawit nyalakan …`: starts Sawit instances as their Start button and the `start_instance` tool
/// do, never over a port another process holds (`Preflight::Check`): that one's report says who
/// holds it, and the exit code is 1. No names: every stopped instance. `--ulang` restarts the
/// running ones too, as `restart_instance` does, which drops their connections, so it asks first
/// as `bakar instances` does (`-y` doesn't; without a terminal it needs `-y`).
pub async fn nyalakan(args: NyalakanArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    ink.header(
        if args.again {
            "🔥 Nyalakan Ulang Kebun (instances)"
        } else {
            "🔥 Nyalakan Kebun (instances)"
        },
        if args.again {
            "pg_ctl stop lalu start, port diperiksa dulu: lahan orang tidak diserobot (kali ini)"
        } else {
            "pg_ctl start, port diperiksa dulu: lahan orang tidak diserobot (kali ini)"
        },
    );
    let registry = match Registry::load(&paths.instances_file()) {
        Ok(registry) => registry,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    if let Some(unknown) = args.names.iter().find(|n| registry.get(n).is_none()) {
        println!(
            "  {} Kebun “{unknown}” tidak terdaftar. Ilegal, ya? Lihat `sawit list instances`.",
            ink.red("✗")
        );
        return 1;
    }
    let mut running = Vec::new();
    let mut stopped = Vec::new();
    for inst in &registry.instances {
        if !args.names.is_empty() && !args.names.contains(&inst.name) {
            continue;
        }
        // One whose status can't be read is tried: starting it says why it can't.
        match control::status(inst).await {
            Ok(PgCtlStatus::Running { pid }) => running.push((inst, pid)),
            _ => stopped.push(inst),
        }
    }
    if running.is_empty() && stopped.is_empty() {
        ink.empty("belum ada kebun. Hutannya masih utuh, untuk sementara.");
        ink.footer("Tidak ada yang dinyalakan.");
        return 0;
    }
    // The question is only about the running ones: a no leaves them be, and the stopped ones
    // still start, as without `--ulang`.
    let mut again = args.again;
    if !args.again {
        for (inst, pid) in &running {
            ink.empty(&format!("{} sudah menyala, PID {pid}", inst.name));
        }
        if stopped.is_empty() {
            ink.footer("Semua sudah menyala. Asapnya sudah sampai negeri tetangga.");
            return 0;
        }
    } else if !running.is_empty() {
        ink.section(&format!(
            "Kebun menyala, dinyalakan ulang ({})",
            running.len()
        ));
        for (ix, (inst, pid)) in running.iter().enumerate() {
            println!(
                "  {} {:<16} :{:<6} {}",
                ink.gray(&format!("{:>3}.", ix + 1)),
                inst.name,
                inst.port,
                ink.gray(&format!("PID {pid}"))
            );
        }
        println!();
        if !args.yes {
            match confirm(&format!(
                "Nyalakan ulang {} kebun di atas? Koneksinya putus sebentar.",
                running.len()
            )) {
                Some(true) => {}
                Some(false) if stopped.is_empty() => {
                    ink.footer("Batal. Kebun tetap menyala seperti tadi.");
                    return 0;
                }
                Some(false) => {
                    ink.empty("yang menyala dibiarkan menyala; yang padam tetap dinyalakan.");
                    again = false;
                }
                None => {
                    println!(
                        "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y untuk menyalakan ulang.",
                        ink.red("✗")
                    );
                    return 1;
                }
            }
        }
    }
    let restarts = running
        .iter()
        .filter(|_| again)
        .map(|(inst, _)| (*inst, true));
    let plan: Vec<(&Instance, bool)> = restarts
        .chain(stopped.iter().map(|inst| (*inst, false)))
        .collect();
    let mut failed = 0;
    for &(inst, restart) in &plan {
        let managed = registry.data_dirs();
        let outcome = match restart {
            true => control::restart(inst, paths, managed, Preflight::Check).await,
            false => control::start(inst, paths, managed, Preflight::Check).await,
        };
        match outcome {
            Ok(StartOutcome::Started) => println!(
                "  {} {} menyala  {}",
                ink.green("✓"),
                inst.name,
                ink.gray(&format!(
                    "postgres://postgres@localhost:{}/postgres",
                    inst.port
                ))
            ),
            Ok(StartOutcome::AlreadyRunning { pid }) => {
                ink.empty(&format!("{} sudah menyala, PID {pid}", inst.name));
            }
            Ok(StartOutcome::Blocked(report)) => {
                failed += 1;
                println!(
                    "  {} {} tidak dinyalakan: lahan :{} sudah ada yang menggarap.",
                    ink.red("✗"),
                    inst.name,
                    inst.port
                );
                for line in report.explain().lines() {
                    println!("      {line}");
                }
                println!(
                    "      {}",
                    ink.gray(
                        "Lihat siapa di sana: `sawit list ports`. Padamkan dulu, atau pakai Start Anyway di Sawit."
                    )
                );
            }
            Err(e) => {
                failed += 1;
                println!("  {} {}: {e}", ink.red("✗"), inst.name);
            }
        }
    }
    println!();
    ink.kv(
        "Menyala",
        &format!("{} / {} kebun", plan.len() - failed, plan.len()),
    );
    ink.footer(if failed == 0 {
        "Selesai. Kebun menyala; asapnya urusan musim depan."
    } else {
        "Sebagian tidak menyala; lihat di atas."
    });
    i32::from(failed > 0)
}

/// How long `sawit browse` waits for each answer of the app, which gives a page op 30 s.
const BROWSE_TIMEOUT: Duration = Duration::from_secs(35);

/// `sawit browse [url|port]` (`intip`), and `sawit <url>`: opens the page in Sawit's browser and
/// brings Sawit forward. It sends what `browser_navigate` sends for an agent working in `cwd`, so
/// the browser is that of the open project that holds `cwd` (else the front window's). With no
/// address, the dev server running in `cwd`, else in that project ([`dev_server_in`]).
/// `start_app` starts Sawit when it isn't running, as `sawit <path>` does.
pub fn browse(
    target: BrowseTarget,
    paths: &AppPaths,
    cwd: &Path,
    start_app: impl FnOnce() -> Result<(), String>,
) -> i32 {
    let ink = Ink::stderr();
    if !app_control::is_running(paths)
        && let Err(e) = start_app()
    {
        eprintln!("{} Sawit tidak mau menyala: {e}", ink.red("🔥"));
        return 1;
    }
    let folder = own_folder(cwd);
    let ask = |command| {
        let request = Request {
            project: None,
            agent_folder: folder.clone(),
            agent: None,
            command,
        };
        app_control::send(paths, &request, BROWSE_TIMEOUT)
    };
    let url = match target {
        BrowseTarget::Address(address) => browser::normalize_address(&address),
        BrowseTarget::DevServer => {
            let found = ask(Command::State).and_then(|state| {
                let folders = dev_server_folders(&state, folder.as_deref())?;
                let jobs =
                    block_on(probe::listening_jobs()).map_err(|e| format!("lsof gagal: {e}"))?;
                dev_server_in(&jobs, &folders)
            });
            match found {
                Ok(url) => url,
                Err(e) => {
                    eprintln!("{} {e}", ink.red("🔥"));
                    return 1;
                }
            }
        }
    };
    let op = BrowserOp::Navigate { url: url.clone() };
    if let Err(e) = ask(Command::Browser { op }) {
        eprintln!("{} {url} tidak bisa diintip: {e}", ink.red("🔥"));
        return 1;
    }
    let _ = ask(Command::Focus);
    eprintln!(
        "{}",
        ink.gray(&format!("🌐 {url} · terbuka di browser Sawit"))
    );
    0
}

/// The folder `sawit browse` works in, as an agent's would be: its own, unless that is `/` or
/// the home folder, which hold no one project.
fn own_folder(cwd: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    (cwd.parent().is_some() && Some(cwd) != home.as_deref()).then(|| cwd.to_path_buf())
}

/// Where `sawit browse` looks for the dev server, nearest first, from the app's `state`: `folder`,
/// then the open project that holds it; with no folder of its own, the front window's project,
/// whose browser shows the page.
fn dev_server_folders(
    state: &serde_json::Value,
    folder: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    let Some(folder) = folder else {
        return browser::front_project(state).map(|project| vec![project]).ok_or_else(|| {
            "Jendela Sawit di depan tidak punya proyek, jadi tidak ada dev server-nya. Sebut alamatnya: `sawit browse 5173`."
                .to_string()
        });
    };
    let mut folders = vec![folder.to_path_buf()];
    if let Some(project) = browser::project_holding(state, folder)
        && !folders.contains(&project)
    {
        folders.push(project);
    }
    Ok(folders)
}

/// The address of the dev server running in the first of `folders` that runs any
/// ([`browser::dev_servers`]); several there, or none in any, is an error that says what to type.
fn dev_server_in(jobs: &[PortJob], folders: &[PathBuf]) -> Result<String, String> {
    for folder in folders {
        let found = browser::dev_servers(jobs, folder);
        match found.as_slice() {
            [] => continue,
            [job] => {
                if let Some(url) = browser::dev_server_url(job) {
                    return Ok(url);
                }
            }
            several => {
                let named: Vec<String> = several
                    .iter()
                    .map(|job| format!("{} ({})", ports_of(job), clip(&job.label, 30)))
                    .collect();
                let port = several[0]
                    .ports
                    .iter()
                    .map(|(p, _)| *p)
                    .min()
                    .unwrap_or_default();
                return Err(format!(
                    "Ada {} dev server di {}: {}. Sebut salah satu: `sawit browse {port}`.",
                    several.len(),
                    tilde(folder),
                    named.join(", ")
                ));
            }
        }
    }
    let place = folders.last().map(|f| tilde(f)).unwrap_or_default();
    Err(format!(
        "Tidak ada dev server yang menyala di {place}. Jalankan dulu (`npm run dev`, `bun dev`, …), atau sebut alamatnya: `sawit browse 5173`. Lihat `sawit list ports`."
    ))
}

/// Whether `path` may be written: new, or replacing it was allowed.
fn may_write(path: &Path, yes: bool, ink: &Ink) -> bool {
    if yes || !path.exists() {
        return true;
    }
    match confirm(&format!("{} sudah ada. Timpa?", tilde(path))) {
        Some(answer) => answer,
        None => {
            eprintln!(
                "{} {} sudah ada dan tidak ada saksi (stdin bukan terminal). Tambahkan -y untuk menimpa.",
                ink.red("🔥"),
                tilde(path)
            );
            false
        }
    }
}

/// `sawit panen|backup|tanam …`.
pub async fn db_job(job: DbJob, paths: &AppPaths, cwd: &Path) -> i32 {
    let ink = Ink::stderr();
    if job.kind != DbJobKind::Import && !may_write(&job.path, job.yes, &ink) {
        return 1;
    }
    let kebun = match find(&job.db, paths, cwd).await {
        Ok(Some(kebun)) => kebun,
        Ok(None) => {
            eprintln!(
                "{} Kebun “{}” tidak ada di peta. Lihat `sawit list db`.",
                ink.red("🔥"),
                job.db
            );
            return 1;
        }
        Err(e) => {
            eprintln!("{} {e}", ink.red("🔥"));
            return 1;
        }
    };
    let p = &kebun.profile;
    eprintln!(
        "{}",
        ink.gray(&format!(
            "🌴 {} · {}:{} · {}",
            p.database, p.host, p.port, kebun.via
        ))
    );
    let client = match db::connect(p, kebun.password.as_deref()).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{} Gagal masuk kebun {}: {e}", ink.red("🔥"), p.database);
            if let Some(name) = stopped_instance(p, paths).await {
                eprintln!("   Kebun {name} padam. Nyalakan dulu: `sawit nyalakan {name}`");
            }
            return 1;
        }
    };
    let started = std::time::Instant::now();
    let object = job.object.as_deref();
    let split = |o: &str| match o.split_once('.') {
        Some((schema, table)) => (Some(schema.to_string()), table.to_string()),
        None => (None, o.to_string()),
    };
    let done = match job.kind {
        DbJobKind::Export => {
            let object = object.unwrap_or_default();
            let result = if object.contains(char::is_whitespace) {
                db::export_query(&client, object, &job.path).await
            } else {
                let (schema, table) = split(object);
                db::export_table(&client, schema.as_deref(), &table, &job.path).await
            };
            result.map(|s| {
                let mut text = format!("dipanen {} baris ke {}", s.rows, tilde(&job.path));
                if s.truncated {
                    text.push_str(
                        " · karung Excel penuh (1.048.576 baris), pakai .csv untuk semuanya",
                    );
                }
                text
            })
        }
        DbJobKind::Backup => {
            let scope = match object {
                None => Ok(db::BackupScope::Database),
                Some(o) => match db::load_schema(&client).await {
                    Ok(schemas) if !o.contains('.') && schemas.iter().any(|s| s.name == o) => {
                        Ok(db::BackupScope::Schema(o.into()))
                    }
                    Ok(schemas) => {
                        let (schema, table) = split(o);
                        db::find_relation(&schemas, schema.as_deref(), &table).map(|(s, r)| {
                            db::BackupScope::Table {
                                schema: s.into(),
                                table: r.name.clone(),
                            }
                        })
                    }
                    Err(e) => Err(e),
                },
            };
            match scope {
                Ok(scope) => {
                    let roots =
                        crate::binaries::SearchRoots::detect(paths.managed_bin_root()).await;
                    let binaries = crate::binaries::discover(&roots).await;
                    db::backup(
                        &client,
                        p,
                        kebun.password.as_deref(),
                        &binaries,
                        &scope,
                        &job.path,
                    )
                    .await
                    .map(|()| {
                        let size = std::fs::metadata(&job.path).map(|m| m.len()).unwrap_or(0);
                        format!(
                            "bibit cadangan disimpan di {} ({} KB)",
                            tilde(&job.path),
                            size.div_ceil(1024)
                        )
                    })
                }
                Err(e) => Err(e),
            }
        }
        DbJobKind::Import => {
            let (schema, table) = split(object.unwrap_or_default());
            match db::load_schema(&client).await.and_then(|schemas| {
                db::find_relation(&schemas, schema.as_deref(), &table)
                    .map(|(s, r)| (s.to_string(), r.name.clone()))
            }) {
                Ok((schema, table)) => db::import_csv(&client, &schema, &table, &job.path)
                    .await
                    .map(|n| format!("ditanam {n} baris di {schema}.{table}")),
                Err(e) => Err(e),
            }
        }
    };
    match done {
        Ok(text) => {
            eprintln!(
                "{}",
                ink.gray(&format!("🌾 {text} · {} ms", started.elapsed().as_millis()))
            );
            0
        }
        Err(e) => {
            eprintln!("{} Gagal: {e}", ink.red("🔥"));
            1
        }
    }
}

/// `sawit pasang <tool…>`.
pub async fn pasang(args: PasangArgs) -> i32 {
    use crate::dev_tools;
    let ink = Ink::stdout();
    ink.header(
        "🌱 Pasang Bibit (tools)",
        "skrip instal resmi tiap alat (curl … | sh). Prasyarat ditanam duluan",
    );
    let detection = dev_tools::detect().await;
    if let Some(problem) = &detection.problem {
        // Which tools are missing is unknown: don't run an installer over a
        // copy that's there.
        println!(
            "  {} Shell login tidak menjawab ({problem}), jadi tak ada yang dipasang. Coba lagi.",
            ink.red("✗")
        );
        return 1;
    }
    let found = detection.found;
    for id in args.tools.iter().filter(|t| found.contains_key(t.as_str())) {
        ink.empty(&format!("{id} sudah terpasang"));
    }
    let wanted: Vec<&str> = args.tools.iter().map(|t| t.as_str()).collect();
    let plan: Vec<_> = dev_tools::plan(&wanted, &|id| found.contains_key(id))
        .into_iter()
        .filter(|t| !found.contains_key(t.id))
        .collect();
    if plan.is_empty() {
        ink.footer("Tidak ada yang perlu dipasang.");
        return 0;
    }
    ink.section(&format!("Bibit ({})", plan.len()));
    for (ix, t) in plan.iter().enumerate() {
        println!(
            "  {} {:<14} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            t.id,
            ink.gray(t.install)
        );
    }
    println!();
    if !args.yes {
        match confirm(&format!("Jalankan {} skrip instal di atas?", plan.len())) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Lahan dibiarkan kosong.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau memasang.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    for tool in plan {
        if tool.interactive {
            // It asks for a password, which the installer can't read here.
            ink.warn(&format!(
                "{} minta kata sandi. Jalankan sendiri, lalu ulangi:\n      {}",
                tool.id, tool.install
            ));
            ink.footer("Berhenti di sini; yang berikutnya mungkin butuh dia.");
            return 1;
        }
        println!("  {} {}", ink.yellow("⏳"), tool.name);
        let gray = |line: String| println!("     {}", ink.gray(&clip(&line, 100)));
        match dev_tools::install(tool, gray).await {
            Ok(()) => println!("  {} {} terpasang", ink.green("✓"), tool.id),
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), tool.id);
                ink.footer("Gagal tumbuh. Sisanya tidak ditanam.");
                return 1;
            }
        }
    }
    ink.footer("Selesai. Buka terminal baru supaya PATH-nya ikut segar.");
    0
}

/// Why `sawit pupuk` leaves alone a tool it was asked to upgrade: an app
/// installed outside what upgrades it (PureMac from its disk image), one
/// already up to date, or one not installed.
fn pupuk_skip(
    id: &str,
    found: &std::collections::HashMap<&'static str, crate::dev_tools::Found>,
) -> String {
    use crate::dev_tools::{Found, Outside};
    match found.get(id) {
        Some(Found {
            outside: Some(outside),
            path,
            ..
        }) => format!(
            "{id} di {} dipasang di luar {}: tidak dipupuk Sawit",
            tilde(path),
            match outside {
                Outside::Homebrew => "Homebrew",
                Outside::InstallScript => "skrip instalnya",
            }
        ),
        Some(_) => format!("{id} sudah versi terbaru"),
        None => format!("{id} belum terpasang. Tanam dulu: `sawit pasang {id}`"),
    }
}

/// What `sawit cabut` warns before removing `tool`, which the installed
/// tools `needed_by` need: those that run through it wither with it; an app
/// from a Homebrew cask keeps running, but Homebrew no longer updates it.
fn cabut_warnings(
    tool: &crate::dev_tools::Tool,
    needed_by: &[&crate::dev_tools::Tool],
) -> Vec<String> {
    use crate::dev_tools::{Impact, impact};
    let names = |which: Impact| -> Vec<&str> {
        needed_by
            .iter()
            .filter(|t| impact(t) == which)
            .map(|t| t.name)
            .collect()
    };
    let mut out = Vec::new();
    let wither = names(Impact::StopsWorking);
    if !wither.is_empty() {
        out.push(format!(
            "{} butuh {}; ikut layu kalau dicabut.",
            wither.join(", "),
            tool.name
        ));
    }
    let apps = names(Impact::NoHomebrewUpdates);
    if !apps.is_empty() {
        out.push(format!(
            "{} tetap jalan, tapi tak lagi diperbarui lewat {}.",
            apps.join(", "),
            tool.name
        ));
    }
    out
}

/// `sawit pupuk [tool…]`.
pub async fn pupuk(args: PupukArgs) -> i32 {
    use crate::dev_tools;
    let ink = Ink::stdout();
    ink.header(
        "🧪 Pupuk Bibit (upgrade tools)",
        "perintah upgrade tiap alat, atau skrip instalnya lagi",
    );
    let (detection, latest) = futures::join!(dev_tools::detect(), dev_tools::latest_versions());
    let found = detection.found;
    if let Some(problem) = &detection.problem {
        ink.warn(&format!(
            "Shell login tidak menjawab ({problem}): hanya alat di tempat biasanya yang dicek."
        ));
    }
    let wanted: Vec<&str> = args.tools.iter().map(|t| t.as_str()).collect();
    let plan = dev_tools::upgrade_plan(&wanted, &found, &latest);
    for id in wanted
        .iter()
        .filter(|id| !plan.iter().any(|t| t.id == **id))
    {
        ink.empty(&pupuk_skip(id, &found));
    }
    if plan.is_empty() {
        ink.footer("Tidak ada yang perlu dipupuk. Kebun sudah subur.");
        return 0;
    }
    ink.section(&format!("Bibit ({})", plan.len()));
    for (ix, t) in plan.iter().enumerate() {
        let from = found
            .get(t.id)
            .and_then(|f| f.version.clone())
            .unwrap_or_else(|| "?".into());
        let to = dev_tools::newer(t.id, &found, &latest).unwrap_or("?");
        println!(
            "  {} {:<14} {:<22} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            t.id,
            clip(&format!("{from} → {to}"), 22),
            ink.gray(t.upgrade_command())
        );
    }
    println!();
    if !args.yes {
        match confirm(&format!(
            "Jalankan {} perintah upgrade di atas?",
            plan.len()
        )) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Pupuknya disimpan dulu.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau memupuk.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    let mut code = 0;
    for tool in plan {
        if tool.interactive {
            // It may ask for a password, which can't be read here.
            ink.warn(&format!(
                "{} mungkin minta kata sandi. Jalankan sendiri:\n      {}",
                tool.id,
                tool.upgrade_command()
            ));
            code = 1;
            continue;
        }
        println!("  {} {}", ink.yellow("⏳"), tool.name);
        let gray = |line: String| println!("     {}", ink.gray(&clip(&line, 100)));
        match dev_tools::run(tool, tool.upgrade_command(), gray).await {
            Ok(()) => println!("  {} {} dipupuk", ink.green("✓"), tool.id),
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), tool.id);
                code = 1;
            }
        }
    }
    ink.footer(if code == 0 {
        "Selesai. Buka terminal baru supaya versinya ikut segar."
    } else {
        "Sebagian gagal tumbuh; lihat di atas."
    });
    code
}

/// `sawit cabut <tool…>`: what the Dev Tools page's trash button runs, and
/// only that ([`crate::dev_tools::uninstall_plan`]).
pub async fn cabut(args: CabutArgs) -> i32 {
    use crate::dev_tools::{self, Removal};
    let ink = Ink::stdout();
    ink.header(
        "🪓 Cabut Bibit (uninstall tools)",
        "hanya salinan dari installer-nya sendiri, Homebrew atau npm. Sisanya dibiarkan tumbuh",
    );
    let detection = dev_tools::detect().await;
    if let Some(problem) = &detection.problem {
        // Which copy is the one in use is unknown: don't remove one the user
        // didn't mean.
        println!(
            "  {} Shell login tidak menjawab ({problem}), jadi tak ada yang dicabut. Coba lagi.",
            ink.red("✗")
        );
        return 1;
    }
    let wanted: Vec<&str> = args.tools.iter().map(|t| t.as_str()).collect();
    let plan = dev_tools::uninstall_plan(&wanted, &detection.found);
    let mut code = 0;
    for step in &plan {
        let id = step.tool.id;
        let at = step.found.as_deref().map(tilde).unwrap_or_default();
        match &step.removal {
            Removal::Missing => ink.empty(&format!("{id} belum terpasang")),
            Removal::LeftAlone => ink.empty(&format!(
                "{id} di {at} bukan tanaman installer-nya, Homebrew atau npm: dibiarkan"
            )),
            Removal::Bundled(with) => ink.empty(&format!(
                "{id} tumbuh bersama {}: ikut tercabut kalau itu dicabut, tidak sendirian",
                with.join(", ")
            )),
            Removal::Terminal(command) => {
                // It asks for a password, which the uninstaller can't read here.
                ink.warn(&format!(
                    "{id} minta kata sandi. Jalankan sendiri:\n      {}",
                    command.replace('\n', "\n      ")
                ));
                code = 1;
            }
            Removal::Run(_) => {}
        }
    }
    let runs: Vec<(&dev_tools::Uninstall, &str)> = plan
        .iter()
        .filter_map(|step| match &step.removal {
            Removal::Run(command) => Some((step, command.as_str())),
            _ => None,
        })
        .collect();
    if runs.is_empty() {
        ink.footer("Tidak ada yang perlu dicabut.");
        return code;
    }
    ink.section(&format!("Bibit ({})", runs.len()));
    for (ix, (step, command)) in runs.iter().enumerate() {
        let at = step.found.as_deref().map(tilde).unwrap_or_default();
        println!(
            "  {} {:<14} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            step.tool.id,
            ink.gray(&at)
        );
        for line in command.lines() {
            println!("       {}", ink.gray(line));
        }
        for warning in cabut_warnings(step.tool, &step.needed_by) {
            ink.warn(&warning);
        }
    }
    println!();
    if !args.yes {
        match confirm(&format!(
            "Cabut {} bibit di atas sampai ke akarnya? Tidak bisa ditanam balik tanpa pasang ulang.",
            runs.len()
        )) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Bibitnya tetap tumbuh, untuk musim ini.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau mencabut.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    let mut removed = Vec::new();
    for (step, command) in &runs {
        println!("  {} {}", ink.yellow("⏳"), step.tool.name);
        let gray = |line: String| println!("     {}", ink.gray(&clip(&line, 100)));
        match dev_tools::uninstall(step.tool, command, gray).await {
            Ok(()) => {
                println!("  {} {} dicabut", ink.green("✓"), step.tool.id);
                removed.push(step.tool.id);
            }
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), step.tool.id);
                code = 1;
            }
        }
    }
    if !removed.is_empty() {
        // As the page does: a copy installed another way is still found.
        let after = dev_tools::detect().await;
        for id in removed {
            if let Some(f) = after.found.get(id) {
                ink.warn(&format!(
                    "{id} masih tumbuh di {}: salinan itu dipasang dengan cara lain (Homebrew, misalnya).",
                    tilde(&f.path)
                ));
                code = 1;
            }
        }
    }
    ink.footer(if code == 0 {
        "Selesai. Lahan kosong lagi; buka terminal baru supaya PATH-nya ikut segar."
    } else {
        "Sebagian masih berakar; lihat di atas."
    });
    code
}

/// Asks on the terminal; `None` when stdin isn't one.
fn confirm(question: &str) -> Option<bool> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    let ink = Ink::stdout();
    print!("  {} {question} [y/N] ", ink.yellow("?"));
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).ok()?;
    Some(matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "ya" | "yes"
    ))
}

/// [`confirm`], asked on stderr: `sawit <db>` keeps stdout for its tables.
fn confirm_on_stderr(question: &str) -> Option<bool> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    let ink = Ink::stderr();
    eprint!("  {} {question} [y/N] ", ink.yellow("?"));
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).ok()?;
    Some(matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "ya" | "yes"
    ))
}

/// `text` cut to `max` characters, with `…` when shortened.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max - 1) {
        Some((ix, _)) if text.chars().count() > max => format!("{}…", &text[..ix]),
        _ => text.into(),
    }
}

/// `/Users/me/x` → `~/x`.
fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// ANSI colors, off when the stream isn't a terminal or `NO_COLOR` is set
/// (the same look as the `~/.zsh_functions` scripts).
struct Ink {
    on: bool,
}

impl Ink {
    fn stdout() -> Ink {
        Ink::new(std::io::stdout().is_terminal())
    }

    fn stderr() -> Ink {
        Ink::new(std::io::stderr().is_terminal())
    }

    fn new(terminal: bool) -> Ink {
        Ink {
            on: terminal && std::env::var_os("NO_COLOR").is_none(),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        match self.on {
            true => format!("\x1b[{code}m{text}\x1b[0m"),
            false => text.to_string(),
        }
    }

    fn red(&self, t: &str) -> String {
        self.paint("1;31", t)
    }

    fn green(&self, t: &str) -> String {
        self.paint("1;32", t)
    }

    fn yellow(&self, t: &str) -> String {
        self.paint("1;33", t)
    }

    fn gray(&self, t: &str) -> String {
        self.paint("90", t)
    }

    fn rule(&self) {
        println!("{}", self.gray(&"─".repeat(60)));
    }

    fn header(&self, title: &str, subtitle: &str) {
        println!();
        println!("{}", self.paint("1;34", title));
        println!("{}", self.gray(subtitle));
        self.rule();
    }

    fn section(&self, title: &str) {
        println!();
        println!("{}", self.paint("1;33", &format!("▸ {title}")));
    }

    fn empty(&self, text: &str) {
        println!("  {}", self.gray(&format!("({text})")));
    }

    fn warn(&self, text: &str) {
        println!("  {} {text}", self.yellow("!"));
    }

    fn kv(&self, key: &str, value: &str) {
        println!("  {} {value}", self.gray(&format!("{key:<20}")));
    }

    fn footer(&self, text: &str) {
        self.rule();
        println!("{}", self.green(text));
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sawit pupuk puremac` on PureMac from its disk image says it's outside Homebrew and leaves
    /// it, as the page and upgrade_dev_tools do; `sawit cabut homebrew` says a cask's app only
    /// stops getting updates.
    #[test]
    fn pupuk_and_cabut_word_an_app_outside_homebrew() {
        use crate::dev_tools::{Found, Outside, find, upgrade_plan};
        let at = |path: &str, outside| Found {
            path: PathBuf::from(path),
            version: Some("3.1.0".into()),
            outside,
        };
        let found = std::collections::HashMap::from([
            (
                "puremac",
                at("/Applications/PureMac.app", Some(Outside::Homebrew)),
            ),
            (
                "zed",
                at("/Applications/Zed.app", Some(Outside::InstallScript)),
            ),
            ("bun", at("/nonexistent-sawit-test/.bun/bin/bun", None)),
        ]);
        let latest = std::collections::HashMap::from([("puremac", "3.2.0".to_string())]);
        assert!(upgrade_plan(&["puremac", "zed"], &found, &latest).is_empty());
        assert_eq!(
            pupuk_skip("puremac", &found),
            "puremac di /Applications/PureMac.app dipasang di luar Homebrew: tidak dipupuk Sawit"
        );
        assert_eq!(
            pupuk_skip("zed", &found),
            "zed di /Applications/Zed.app dipasang di luar skrip instalnya: tidak dipupuk Sawit"
        );
        assert_eq!(pupuk_skip("bun", &found), "bun sudah versi terbaru");
        assert!(pupuk_skip("deno", &found).contains("belum terpasang"));

        let tool = |id: &str| find(id).unwrap();
        assert_eq!(
            cabut_warnings(tool("homebrew"), &[tool("puremac")]),
            ["PureMac tetap jalan, tapi tak lagi diperbarui lewat Homebrew."]
        );
        assert_eq!(
            cabut_warnings(tool("node"), &[tool("npm"), tool("gemini")]),
            ["npm (latest), Gemini CLI butuh Node.js; ikut layu kalau dicabut."]
        );
        assert!(cabut_warnings(tool("bun"), &[]).is_empty());
    }

    fn q(args: &[&str]) -> Option<QueryArgs> {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        query_args(&args, Path::new("/nonexistent-sawit-test"))
    }

    fn qa(db: &str, table: Option<&str>, sql: Option<&str>) -> Option<QueryArgs> {
        Some(QueryArgs {
            db: db.into(),
            table: table.map(Into::into),
            sql: sql.map(Into::into),
            yes: false,
        })
    }

    #[test]
    fn query_shapes() {
        assert_eq!(q(&["kyzn"]), qa("kyzn", None, None));
        assert_eq!(q(&["kyzn", "leads"]), qa("kyzn", Some("leads"), None));
        assert_eq!(
            q(&["kyzn", "leads", "select * from leads where phone is null"]),
            qa(
                "kyzn",
                Some("leads"),
                Some("select * from leads where phone is null")
            )
        );
        assert_eq!(q(&["kyzn", "select 1"]), qa("kyzn", None, Some("select 1")));
        assert_eq!(
            q(&["kyzn", "crm.leads"]),
            qa("kyzn", Some("crm.leads"), None)
        );
    }

    #[test]
    fn not_query_shapes() {
        assert_eq!(q(&[]), None);
        assert_eq!(q(&["-n", "kyzn"]), None);
        assert_eq!(q(&["src/main.rs"]), None);
        assert_eq!(q(&["main.rs"]), None);
        assert_eq!(q(&["a", "b", "c", "d"]), None);
        assert_eq!(q(&["kyzn", "a b", "select 1"]), None);
        // An existing folder is opened, not queried.
        let args = [OsString::from("tmp")];
        assert_eq!(query_args(&args, Path::new("/")), None);
    }

    /// `-y` (`--yes`, `--ya`) anywhere runs SQL that writes without asking; it isn't a table,
    /// and with it a name that is no database is said to be none, not opened as a file.
    #[test]
    fn a_query_takes_yes_anywhere() {
        let yes = |db: &str, table: Option<&str>, sql: Option<&str>| {
            qa(db, table, sql).map(|q| QueryArgs { yes: true, ..q })
        };
        assert_eq!(
            q(&["kyzn", "delete from leads", "-y"]),
            yes("kyzn", None, Some("delete from leads"))
        );
        assert_eq!(
            q(&["--yes", "kyzn", "leads", "id = 3"]),
            yes("kyzn", Some("leads"), Some("id = 3"))
        );
        assert_eq!(q(&["kyzn", "--ya"]), yes("kyzn", None, None));
        assert!(q(&["kyzn", "-y"]).unwrap().explicit());
        // Other options are still not a query's.
        assert_eq!(q(&["-y"]), None);
        assert_eq!(q(&["kyzn", "-n"]), None);
        assert_eq!(q(&["kyzn", "-y", "-r"]), None);
    }

    /// Before it asks, `sawit <db>` shows the statement: up to 20 lines, each cut at 200
    /// characters, then how many more lines there are.
    #[test]
    fn a_statement_is_shown_before_it_runs() {
        assert_eq!(
            statement_preview("  delete from leads\n where id = 3\n"),
            ["delete from leads", " where id = 3"]
        );
        let long = (1..=25)
            .map(|n| format!("insert into t values ({n});"))
            .collect::<Vec<_>>()
            .join("\n");
        let shown = statement_preview(&long);
        assert_eq!(shown.len(), 21);
        assert_eq!(shown[19], "insert into t values (20);");
        assert_eq!(shown[20], "… (5 baris lagi)");
        let wide = statement_preview(&"x".repeat(300));
        assert_eq!(wide[0].chars().count(), 200);
        assert!(wide[0].ends_with('…'));
    }

    #[test]
    fn subcommands_are_not_databases() {
        assert_eq!(q(&["list"]), None);
        assert_eq!(q(&["bakar", "ports"]), None);
        assert_eq!(q(&["ls", "db"]), None);
        assert_eq!(q(&["daftar"]), None);
        assert_eq!(q(&["stop", "ports"]), None);
    }

    #[test]
    fn clipping() {
        assert_eq!(clip("bun run dev", 60), "bun run dev");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("abcd", 4), "abcd");
    }

    #[test]
    fn sql_plans() {
        assert_eq!(plan_sql(None, None), LIST_TABLES);
        assert_eq!(
            plan_sql(Some("leads"), None),
            "SELECT * FROM \"leads\" LIMIT 100"
        );
        assert_eq!(
            plan_sql(
                Some("leads"),
                Some("select * from leads where phone is null")
            ),
            "select * from leads where phone is null"
        );
        assert_eq!(
            plan_sql(Some("leads"), Some("phone is null")),
            "SELECT * FROM \"leads\" WHERE phone is null"
        );
        assert_eq!(
            plan_sql(Some("crm.leads"), Some(" order by id desc limit 5")),
            "SELECT * FROM \"crm\".\"leads\" order by id desc limit 5"
        );
        assert_eq!(
            plan_sql(Some("leads"), Some("(select 1) union (select 2)")),
            "(select 1) union (select 2)"
        );
        assert_eq!(
            plan_sql(
                Some("\"Leads\""),
                Some("WITH x AS (select 1) select * from x")
            ),
            "WITH x AS (select 1) select * from x"
        );
        assert_eq!(plan_sql(None, Some("select 1")), "select 1");
    }

    #[test]
    fn list_words() {
        assert_eq!(ListWhat::parse(None), Ok(ListWhat::All));
        assert_eq!(ListWhat::parse(Some("db")), Ok(ListWhat::Databases));
        assert_eq!(ListWhat::parse(Some("lahan")), Ok(ListWhat::Ports));
        assert_eq!(ListWhat::parse(Some("kebun")), Ok(ListWhat::Instances));
        assert_eq!(ListWhat::parse(Some("konsesi")), Ok(ListWhat::Connections));
        assert_eq!(ListWhat::parse(Some("explorer")), Ok(ListWhat::Explorer));
        assert_eq!(ListWhat::parse(Some("jelajah")), Ok(ListWhat::Explorer));
        // Asking every dev server takes a moment: only when asked.
        assert!(!ListWhat::All.shows(ListWhat::Explorer));
        assert!(ListWhat::parse(Some("hutan")).is_err());
    }

    #[test]
    fn bakar_words() {
        assert_eq!(
            BakarArgs::parse(&["ports", "3000", "-y"]),
            Ok(BakarArgs {
                what: BakarWhat::Ports,
                targets: vec!["3000".into()],
                yes: true
            })
        );
        assert_eq!(
            BakarArgs::parse(&["instances"]),
            Ok(BakarArgs {
                what: BakarWhat::Instances,
                targets: vec![],
                yes: false
            })
        );
        assert!(BakarArgs::parse(&[]).is_err());
        assert!(BakarArgs::parse(&["hutan"]).is_err());
        assert!(BakarArgs::parse(&["ports", "dev"]).is_err());
        assert!(BakarArgs::parse(&["ports", "--force"]).is_err());
    }

    #[test]
    fn list_tools_only_when_asked() {
        assert_eq!(ListWhat::parse(Some("perkakas")), Ok(ListWhat::Tools));
        assert_eq!(ListWhat::parse(Some("tools")), Ok(ListWhat::Tools));
        assert!(ListWhat::Tools.shows(ListWhat::Tools));
        assert!(!ListWhat::All.shows(ListWhat::Tools));
        assert!(ListWhat::All.shows(ListWhat::Ports));
    }

    /// `sawit list keys` and its aliases list the key bindings, and a bare `sawit list` leaves
    /// them out: they need the running app.
    #[test]
    fn list_keys_only_when_asked() {
        for word in ["keys", "key", "keymap", "shortcuts", "pintasan"] {
            assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Keys), "{word}");
        }
        assert!(ListWhat::Keys.shows(ListWhat::Keys));
        assert!(!ListWhat::Keys.shows(ListWhat::Ports));
        assert!(!ListWhat::All.shows(ListWhat::Keys));
        let unknown = ListWhat::parse(Some("tombol")).unwrap_err();
        assert!(unknown.contains("keys"), "{unknown}");
    }

    /// A Sawit home of the test's own, whose socket is in the test's folder: no app listens
    /// there but the one a test stands in.
    fn scratch_home(dir: &Path) -> AppPaths {
        let paths = AppPaths {
            root: dir.join("home"),
        };
        assert!(
            paths.cli_socket().starts_with(dir),
            "the socket left the test's folder"
        );
        paths
    }

    /// The defaults are bound in the app's code, so without the app `sawit list keys` fails and
    /// says so, rather than start it.
    #[test]
    fn list_keys_without_the_app_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let paths = scratch_home(dir.path());
        assert_eq!(list_keys(&Ink::new(false), &paths), 1);
    }

    /// With the app running, `sawit list keys` asks it for the keymap editor's rows, as
    /// `keymap_list` does.
    #[test]
    fn list_keys_asks_the_running_app() {
        use std::io::Read as _;
        let dir = tempfile::tempdir().unwrap();
        let paths = scratch_home(dir.path());
        std::fs::create_dir_all(&paths.root).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
        let list = crate::keymap::KeymapList {
            path: paths.keymap_file(),
            writable: true,
            errors: vec!["\"cmd-e\": no such action".into()],
            rows: vec![crate::keymap::KeymapRow {
                action: "workspace::CloseAllTabs".into(),
                label: "workspace: close all tabs".into(),
                args: None,
                keys: "cmd-k w".into(),
                display: "⌘K W".into(),
                context: Some("Workspace".into()),
                source: crate::keymap::KeySource::Default,
                conflict: false,
            }],
        };
        let answer = crate::app_control::reply_ok(&serde_json::to_value(&list).unwrap());
        let (asked, requests) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut bytes = Vec::new();
                // `is_running` only connects, and sends nothing.
                if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                    continue;
                }
                let Some(Ok(request)) = crate::app_control::decode(&bytes) else {
                    break;
                };
                let _ = asked.send(request.command);
                let _ = writeln!(stream, "{answer}");
                break;
            }
        });
        assert_eq!(list_keys(&Ink::new(false), &paths), 0);
        let asked = requests
            .recv_timeout(Duration::from_secs(5))
            .expect("the app was asked");
        assert_eq!(
            asked,
            crate::app_control::Command::Keymap {
                op: crate::keymap::KeymapOp::List {
                    filter: None,
                    keys: None,
                    unbound: false,
                },
            }
        );
    }

    /// A binding's line: the keys as a Mac menu writes them, in a column, then what they run,
    /// then the keys as keymap.json writes them, where they work and whose binding it is.
    #[test]
    fn a_key_line_says_what_runs_where_and_whose_it_is() {
        let ink = Ink::new(false);
        let row = crate::keymap::KeymapRow {
            action: "workspace::CloseAllTabs".into(),
            label: "workspace: close all tabs".into(),
            args: None,
            keys: "cmd-k w".into(),
            display: "⌘K W".into(),
            context: Some("Workspace".into()),
            source: crate::keymap::KeySource::User,
            conflict: true,
        };
        let line = key_line(&ink, &row);
        assert!(line.starts_with("  ⌘K W "), "{line}");
        let label: String = line.chars().skip(15).collect();
        assert!(label.starts_with("workspace: close all tabs "), "{line}");
        assert!(
            line.ends_with("cmd-k w · Workspace  milikmu bentrok"),
            "{line}"
        );
        let anywhere = crate::keymap::KeymapRow {
            context: None,
            source: crate::keymap::KeySource::Removed,
            conflict: false,
            ..row
        };
        let line = key_line(&ink, &anywhere);
        assert!(line.ends_with("cmd-k w · di mana saja  dicabut"), "{line}");
    }

    #[test]
    fn list_mcp_and_config_only_when_asked() {
        for word in ["mcp", "colokan"] {
            assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Mcp), "{word}");
        }
        for word in ["config", "configs", "konfig"] {
            assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Config), "{word}");
        }
        for what in [ListWhat::Mcp, ListWhat::Config] {
            assert!(what.shows(what));
            assert!(!what.shows(ListWhat::Ports), "{what:?}");
            assert!(!ListWhat::All.shows(what), "{what:?}");
        }
        let unknown = ListWhat::parse(Some("hutan")).unwrap_err();
        assert!(unknown.contains("mcp, skills, config"), "{unknown}");
    }

    #[test]
    fn list_skills_only_when_asked() {
        for word in ["skills", "skill", "bibit"] {
            assert_eq!(ListWhat::parse(Some(word)), Ok(ListWhat::Skills), "{word}");
        }
        assert!(ListWhat::Skills.shows(ListWhat::Skills));
        assert!(!ListWhat::All.shows(ListWhat::Skills));
        assert!(!ListWhat::Skills.shows(ListWhat::Mcp));
    }

    #[test]
    fn db_job_words() {
        let cwd = Path::new("/w");
        assert_eq!(DbJob::kind("panen"), Some(DbJobKind::Export));
        assert_eq!(DbJob::kind("dump"), Some(DbJobKind::Backup));
        assert_eq!(DbJob::kind("tanam"), Some(DbJobKind::Import));
        assert_eq!(DbJob::kind("hutan"), None);
        assert_eq!(
            DbJob::parse(DbJobKind::Export, &["kyzn", "leads", "out.xlsx", "-y"], cwd),
            Ok(DbJob {
                kind: DbJobKind::Export,
                db: "kyzn".into(),
                object: Some("leads".into()),
                path: PathBuf::from("/w/out.xlsx"),
                yes: true,
            })
        );
        assert!(DbJob::parse(DbJobKind::Export, &["kyzn", "leads", "out.json"], cwd).is_err());
        assert!(DbJob::parse(DbJobKind::Export, &["kyzn", "out.csv"], cwd).is_err());
        let backup = DbJob::parse(DbJobKind::Backup, &["kyzn", "/b/kyzn.dump"], cwd).unwrap();
        assert_eq!(backup.object, None);
        assert_eq!(backup.path, PathBuf::from("/b/kyzn.dump"));
        let table = DbJob::parse(DbJobKind::Backup, &["kyzn", "crm.leads", "x.sql"], cwd).unwrap();
        assert_eq!(table.object.as_deref(), Some("crm.leads"));
        assert!(DbJob::parse(DbJobKind::Import, &["kyzn", "leads.csv"], cwd).is_err());
        assert!(
            DbJob::parse(
                DbJobKind::Import,
                &["kyzn", "leads", "a.csv", "--force"],
                cwd
            )
            .is_err()
        );
    }

    #[test]
    fn pasang_words() {
        let id = crate::dev_tools::TOOLS[0].id;
        assert_eq!(
            PasangArgs::parse(&[id, "-y"]),
            Ok(PasangArgs {
                tools: vec![id.into()],
                yes: true
            })
        );
        assert!(PasangArgs::parse(&[]).is_err());
        assert!(PasangArgs::parse(&["-y"]).is_err());
        assert!(PasangArgs::parse(&["sawit-palsu"]).is_err());
        assert!(PasangArgs::parse(&[id, "--force"]).is_err());
    }

    #[test]
    fn pupuk_words() {
        let id = crate::dev_tools::TOOLS[0].id;
        assert_eq!(
            PupukArgs::parse(&[]),
            Ok(PupukArgs {
                tools: vec![],
                yes: false
            })
        );
        assert_eq!(
            PupukArgs::parse(&["--ya", id]),
            Ok(PupukArgs {
                tools: vec![id.into()],
                yes: true
            })
        );
        assert!(PupukArgs::parse(&["sawit-palsu"]).is_err());
        assert!(PupukArgs::parse(&["--force"]).is_err());
    }

    #[test]
    fn cabut_words() {
        let id = crate::dev_tools::TOOLS[0].id;
        assert_eq!(
            CabutArgs::parse(&[id, "-y"]),
            Ok(CabutArgs {
                tools: vec![id.into()],
                yes: true
            })
        );
        assert_eq!(
            CabutArgs::parse(&["--ya", id, "bun"]),
            Ok(CabutArgs {
                tools: vec![id.into(), "bun".into()],
                yes: true
            })
        );
        assert_eq!(CabutArgs::parse(&["bun"]).map(|a| a.yes), Ok(false));
        // Something to uproot must be named: no "every tool".
        assert!(CabutArgs::parse(&[]).is_err());
        assert!(CabutArgs::parse(&["-y"]).is_err());
        assert!(CabutArgs::parse(&["sawit-palsu"]).is_err());
        assert!(CabutArgs::parse(&[id, "--force"]).is_err());
    }

    #[test]
    fn nyalakan_words() {
        assert_eq!(NyalakanArgs::parse(&[]), Ok(NyalakanArgs::default()));
        assert_eq!(
            NyalakanArgs::parse(&["kyzn", "--ulang", "shop", "-y", "kyzn"]),
            Ok(NyalakanArgs {
                names: vec!["kyzn".into(), "shop".into()],
                again: true,
                yes: true,
            })
        );
        assert_eq!(
            NyalakanArgs::parse(&["--restart", "--ya"]),
            Ok(NyalakanArgs {
                names: vec![],
                again: true,
                yes: true,
            })
        );
        assert!(NyalakanArgs::parse(&["kyzn", "--force"]).is_err());
        assert!(NyalakanArgs::parse(&["-u"]).is_err());
        // Its words are never databases.
        assert_eq!(q(&["nyalakan", "kyzn"]), None);
        assert_eq!(q(&["start", "kyzn"]), None);
        assert_eq!(q(&["nyalakan"]), None);
    }

    /// A stand-in for the running app, listening where `paths` says the app does: it keeps each
    /// request, and answers `state` for `state` and `{}` for the others.
    fn fake_app(
        paths: &AppPaths,
        state: serde_json::Value,
    ) -> std::sync::Arc<std::sync::Mutex<Vec<Request>>> {
        use std::io::Read as _;
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        // In the test's own folder, never the shared one a long path falls back to.
        assert!(paths.cli_socket().starts_with(&paths.root));
        std::fs::create_dir_all(&paths.root).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut bytes = Vec::new();
                // `is_running` only connects.
                if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                    continue;
                }
                let Some(Ok(request)) = app_control::decode(&bytes) else {
                    continue;
                };
                let answer = match request.command {
                    Command::State => state.clone(),
                    _ => serde_json::json!({}),
                };
                kept.lock().unwrap().push(request);
                let _ = writeln!(stream, "{}", app_control::reply_ok(&answer));
            }
        });
        seen
    }

    /// `sawit browse` sends what browser_navigate sends for an agent working in its folder, so
    /// the page opens in the browser of the project that holds it, then brings Sawit forward. A
    /// port is localhost's; an address is read as the address field reads it. Sawit is started
    /// first when it isn't running, and when it won't start nothing is sent.
    #[test]
    fn browse_opens_the_page_in_the_folders_browser() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            root: dir.path().join("home"),
        };
        let cwd = dir.path().join("shop");
        let navigate = |url: &str| Command::Browser {
            op: BrowserOp::Navigate { url: url.into() },
        };
        let sent = |seen: &std::sync::Mutex<Vec<Request>>| -> Vec<(Option<PathBuf>, Command)> {
            let seen = seen.lock().unwrap();
            seen.iter()
                .map(|r| (r.agent_folder.clone(), r.command.clone()))
                .collect()
        };
        let page = |a: &str| BrowseTarget::Address(a.into());

        assert_eq!(
            browse(
                page("localhost:5173"),
                &paths,
                &cwd,
                || Err("no app".into())
            ),
            1
        );
        let mut started = None;
        let code = browse(page("localhost:5173"), &paths, &cwd, || {
            started = Some(fake_app(&paths, serde_json::json!({})));
            Ok(())
        });
        assert_eq!(code, 0);
        let seen = started.expect("Sawit was started");
        assert_eq!(
            sent(&seen),
            [
                (Some(cwd.clone()), navigate("http://localhost:5173")),
                (Some(cwd.clone()), Command::Focus),
            ]
        );
        seen.lock().unwrap().clear();
        let code = browse(page("https://example.com/a"), &paths, &cwd, || {
            panic!("started while it runs")
        });
        assert_eq!(code, 0);
        assert_eq!(sent(&seen)[0].1, navigate("https://example.com/a"));
    }

    /// With no address, `sawit browse` looks for the dev server in its folder, then in the open
    /// project that holds it: the nearest one wins, two in one place are named for the user to
    /// pick, and none says how to start one or give an address. In `/` or the home folder it
    /// has no folder of its own, as an agent hasn't: the front window's project is used.
    #[test]
    fn browse_finds_the_nearest_dev_server() {
        let job = |cwd: &str, port: u16| PortJob {
            root: 1,
            pids: vec![1],
            label: "bun run dev".into(),
            tool: "bun".into(),
            cmdline: "bun run dev".into(),
            listeners: vec![(2, "bun".into())],
            ports: vec![(port, crate::ports::BindAddr::LoopbackV4)],
            cwd: Some(cwd.into()),
            dev: true,
        };
        let jobs = [job("/code/shop/web", 5173), job("/code/shop/api", 3000)];
        let folders = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(
            dev_server_in(&jobs, &folders(&["/code/shop/web", "/code/shop"])),
            Ok("http://localhost:5173".into())
        );
        let two = dev_server_in(&jobs, &folders(&["/code/shop/docs", "/code/shop"])).unwrap_err();
        assert!(
            two.starts_with("Ada 2 dev server di /code/shop: :5173 (bun run dev), :3000")
                && two.ends_with("`sawit browse 5173`."),
            "{two}"
        );
        let none = dev_server_in(&jobs, &folders(&["/code/blog"])).unwrap_err();
        assert!(
            none.starts_with("Tidak ada dev server yang menyala di /code/blog."),
            "{none}"
        );

        let state = serde_json::json!({"windows": [
            {"project": "/code/shop", "projects": ["/code/shop"], "frontmost": true},
        ]});
        assert_eq!(
            dev_server_folders(&state, Some(Path::new("/code/shop/docs"))),
            Ok(folders(&["/code/shop/docs", "/code/shop"]))
        );
        assert_eq!(
            dev_server_folders(&state, Some(Path::new("/code/shop"))),
            Ok(folders(&["/code/shop"]))
        );
        assert_eq!(
            dev_server_folders(&state, None),
            Ok(folders(&["/code/shop"]))
        );
        assert!(dev_server_folders(&serde_json::json!({"windows": []}), None).is_err());
        assert_eq!(own_folder(Path::new("/")), None);
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(own_folder(Path::new(&home)), None);
        }
        assert_eq!(
            own_folder(Path::new("/code/shop")),
            Some(PathBuf::from("/code/shop"))
        );
        // Its words are never databases.
        assert_eq!(q(&["browse"]), None);
        assert_eq!(q(&["intip", "5173"]), None);
    }

    #[test]
    fn job_words_are_not_databases() {
        assert_eq!(q(&["panen", "kyzn", "a.csv"]), None);
        assert_eq!(q(&["backup", "kyzn"]), None);
        assert_eq!(q(&["pasang", "bun"]), None);
        assert_eq!(q(&["pupuk", "bun"]), None);
        assert_eq!(q(&["cabut", "bun"]), None);
        assert_eq!(q(&["uninstall", "bun"]), None);
    }
}
