//! The words `sawit` takes: the shape of `sawit <db> [table] ["sql"]`, and each subcommand's
//! arguments.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::db;

use super::SUBCOMMANDS;

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
    pub(super) fn explicit(&self) -> bool {
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

    pub(super) fn shows(self, part: ListWhat) -> bool {
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
