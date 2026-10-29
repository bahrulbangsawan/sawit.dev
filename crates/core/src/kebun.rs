//! The `sawit` command's terminal tools, which work without the app:
//! `sawit <db> [table] ["sql"]` like psql, `sawit list`, and `sawit bakar`
//! ("burn": stop dev servers or instances, like the `kill-dev` script).
//! The wording riffs on Indonesia's forest fires (karhutla) and the palm-oil
//! plantations (kebun sawit) behind them.

use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::control;
use crate::db::{
    self, ConnProfile, Profiles, format_table, parse_database_url, read_dotenv_database_url,
    resolve_password,
};
use crate::instances::{AppPaths, Registry};
use crate::pgctl::PgCtlStatus;
use crate::ports::PortJob;
use crate::probe;

/// Words `sawit` takes as commands rather than database names.
const SUBCOMMANDS: &[&str] = &["list", "ls", "bakar"];

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

/// `sawit <db> [table] ["sql"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryArgs {
    pub db: String,
    pub table: Option<String>,
    pub sql: Option<String>,
}

impl QueryArgs {
    /// More than a bare name: the user surely meant a database.
    fn explicit(&self) -> bool {
        self.table.is_some() || self.sql.is_some()
    }
}

/// The arguments as `sawit <db> [table] ["sql"]` when they have that shape:
/// a bare name that isn't a path in `cwd`, then a table and/or SQL (SQL has
/// a space in it). Whether `db` really is a database is decided later.
/// Subcommand names (`list`, `bakar`) are never databases.
pub fn query_args(args: &[OsString], cwd: &Path) -> Option<QueryArgs> {
    let args: Vec<&str> = args.iter().map(|a| a.to_str()).collect::<Option<_>>()?;
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
            Some(other) => {
                return Err(format!(
                    "`sawit list {other}`? Tidak ada di peta. Pilih: instances, db, ports, connections."
                ));
            }
        })
    }

    fn shows(self, part: ListWhat) -> bool {
        self == ListWhat::All || self == part
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
            None => return Err("bakar apa? `sawit bakar ports` atau `sawit bakar instances`.".into()),
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

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

/// First words of complete statements; anything else after a table name is
/// a filter.
const SQL_STARTS: &[&str] = &[
    "select", "with", "insert", "update", "delete", "merge", "explain", "show", "table", "values",
    "create", "alter", "drop", "truncate", "copy", "set", "reset", "begin", "start", "commit",
    "rollback", "grant", "revoke", "analyze", "vacuum", "refresh", "call", "do", "comment",
    "lock", "listen", "notify", "prepare", "execute", "deallocate", "discard", "reindex",
    "cluster", "declare", "fetch", "close",
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
    for k in candidates.iter().filter(|k| k.profile.database.starts_with(name)) {
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

/// `sawit <db> [table] ["sql"]`. `None` when `db` isn't a database and the
/// arguments could just as well be files to open.
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
    let client = match db::connect(p, kebun.password.as_deref()).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{} Gagal masuk kebun {}: {e}", ink.red("🔥"), p.database);
            return Some(1);
        }
    };
    let sql = plan_sql(args.table.as_deref(), args.sql.as_deref());
    let outcome = match db::run_sql(&client, &sql, MAX_ROWS).await {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("{} Gagal panen: {e}", ink.red("🔥"));
            return Some(1);
        }
    };
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
    Some(0)
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
        for inst in &registry.instances {
            let status = match control::status(inst).await {
                Ok(PgCtlStatus::Running { pid }) => ink.green(&format!("menyala (PID {pid})")),
                Ok(PgCtlStatus::Stopped) => ink.gray("padam"),
                Ok(other) => ink.yellow(&format!("{other:?}")),
                Err(e) => ink.red(&format!("error: {e}")),
            };
            println!(
                "  {:<16} :{:<6} {status}  {}",
                inst.name,
                inst.port,
                ink.gray(&format!("postgres://postgres@localhost:{}/postgres", inst.port))
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
                        println!("  {:<14} {}", "", ink.gray(&format!("└ {}", tilde(Path::new(cwd)))));
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
    println!();
    0
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
        let Some(job) = jobs.iter().find(|j| j.ports.iter().any(|(p, _)| *p == port)) else {
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
            println!("       {}", ink.gray(&format!("└ {}", tilde(Path::new(cwd)))));
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

    let pids: Vec<u32> = victims.iter().flat_map(|j| j.pids.iter().copied()).collect();
    let mut refused = Vec::new();
    for &pid in &pids {
        if let Err(e) = probe::terminate(pid, false) {
            refused.push(e);
        }
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let survivors: Vec<u32> = pids.iter().copied().filter(|&p| probe::is_alive(p)).collect();
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

    fn q(args: &[&str]) -> Option<QueryArgs> {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        query_args(&args, Path::new("/nonexistent-sawit-test"))
    }

    fn qa(db: &str, table: Option<&str>, sql: Option<&str>) -> Option<QueryArgs> {
        Some(QueryArgs {
            db: db.into(),
            table: table.map(Into::into),
            sql: sql.map(Into::into),
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
        assert_eq!(
            q(&["kyzn", "select 1"]),
            qa("kyzn", None, Some("select 1"))
        );
        assert_eq!(q(&["kyzn", "crm.leads"]), qa("kyzn", Some("crm.leads"), None));
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

    #[test]
    fn subcommands_are_not_databases() {
        assert_eq!(q(&["list"]), None);
        assert_eq!(q(&["bakar", "ports"]), None);
        assert_eq!(q(&["ls", "db"]), None);
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
            plan_sql(Some("leads"), Some("select * from leads where phone is null")),
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
            plan_sql(Some("\"Leads\""), Some("WITH x AS (select 1) select * from x")),
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
}
